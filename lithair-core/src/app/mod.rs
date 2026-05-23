//! Lithair Server - Unified multi-model server with RBAC and Sessions
//!
//! The LithairServer provides a complete HTTP server with:
//! - Multiple models on a single server
//! - Global RBAC and session management
//! - Automatic configuration loading
//! - Hot-reload support
//! - Admin panel and metrics
//!
//! # Example
//!
//! ```no_run
//! use lithair_core::LithairServer;
//! use lithair_core::session::{SessionManager, MemorySessionStore};
//!
//! # async fn example() -> anyhow::Result<()> {
//! LithairServer::new()
//!     .with_port(8080)
//!     .with_sessions(SessionManager::new(MemorySessionStore::new()))
//!     .with_admin_panel(true)
//!     .serve()
//!     .await?;
//! # Ok(())
//! # }
//! ```

use crate::cluster::RaftLeadershipState;
use crate::config::LithairConfig;
use anyhow::{Context, Result};
use bytes::Bytes;
#[cfg(feature = "tls")]
use sha2::Digest;
use std::sync::Arc;

pub mod builder;
pub mod declarative_serve;
pub mod model_handler;
mod ops_endpoints;
pub mod request;
pub mod response;
pub mod router;
mod schema_handlers;

pub use builder::LithairServerBuilder;
pub use declarative_serve::DeclarativeServe;
pub use model_handler::{DeclarativeModelHandler, ModelHandler, ModelStats};

// ============================================================================
// Public route-handler type aliases (issue #59)
// ============================================================================
//
// Consumers registering custom routes via `LithairServerBuilder::with_route`
// only need to spell out the request and response types in their handler
// signatures. Exposing them as aliases — together with the underlying
// `http::Method` and `http::StatusCode` re-exports — lets downstream crates
// drop direct dependencies on `bytes`, `http`, `http-body-util`, and `hyper`
// from their `Cargo.toml` when they don't otherwise interact with those
// crates. See `lithair/lithair#59` for the motivating use case (kovre's
// custom dashboard routes).

/// Request passed to handlers registered via
/// [`LithairServerBuilder::with_route`], [`LithairServerBuilder::with_route_async`],
/// and [`LithairServerBuilder::with_not_found_handler`].
///
/// This is a type alias for `hyper::Request<hyper::body::Incoming>` — no
/// wrapping, no overhead. The alias exists so consumers can type their
/// handlers without depending on `hyper` directly.
pub type RouteRequest = hyper::Request<hyper::body::Incoming>;

/// Response returned by handlers registered via
/// [`LithairServerBuilder::with_route`], [`LithairServerBuilder::with_route_async`],
/// and [`LithairServerBuilder::with_not_found_handler`], and by every helper
/// in [`response`] (`json`, `json_value`, `json_serialize`, `text`, `html`,
/// `redirect`, `empty`).
///
/// This is a type alias for `hyper::Response<http_body_util::Full<bytes::Bytes>>`
/// — no wrapping, no overhead. The alias exists so consumers can type their
/// handlers and return values without depending on `hyper`, `http-body-util`,
/// or `bytes` directly.
pub type RouteResponse = hyper::Response<http_body_util::Full<bytes::Bytes>>;

/// HTTP method re-exported from the `http` crate.
///
/// `LithairServerBuilder::with_route` accepts a `http::Method`; re-exporting
/// it lets consumers write `use lithair_core::app::Method;` instead of pulling
/// in the `http` crate as a direct dependency.
pub use http::Method;

/// HTTP status code re-exported from the `http` crate.
///
/// All response helpers in [`response`] accept a `http::StatusCode`;
/// re-exporting it lets consumers write `use lithair_core::app::StatusCode;`
/// instead of pulling in the `http` crate as a direct dependency.
///
/// Note: this is the `http` crate's `StatusCode`, **not** the Lithair custom
/// `lithair_core::http::StatusCode` enum (which is used by the lithair-native
/// `HttpServer` / `Route` abstraction). The two types are distinct — this
/// alias matches the one used by `with_route` and the `response::*` helpers.
pub use http::StatusCode;

// ============================================================================
// TLS support types
// ============================================================================

/// A TCP stream that may or may not be wrapped in TLS.
#[cfg(feature = "tls")]
enum MaybeTlsStream {
    Plain(tokio::net::TcpStream),
    Tls(Box<tokio_rustls::server::TlsStream<tokio::net::TcpStream>>),
}

#[cfg(feature = "tls")]
impl tokio::io::AsyncRead for MaybeTlsStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            MaybeTlsStream::Plain(s) => std::pin::Pin::new(s).poll_read(cx, buf),
            MaybeTlsStream::Tls(s) => std::pin::Pin::new(s).poll_read(cx, buf),
        }
    }
}

#[cfg(feature = "tls")]
impl tokio::io::AsyncWrite for MaybeTlsStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match self.get_mut() {
            MaybeTlsStream::Plain(s) => std::pin::Pin::new(s).poll_write(cx, buf),
            MaybeTlsStream::Tls(s) => std::pin::Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            MaybeTlsStream::Plain(s) => std::pin::Pin::new(s).poll_flush(cx),
            MaybeTlsStream::Tls(s) => std::pin::Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            MaybeTlsStream::Plain(s) => std::pin::Pin::new(s).poll_shutdown(cx),
            MaybeTlsStream::Tls(s) => std::pin::Pin::new(s).poll_shutdown(cx),
        }
    }
}

/// Load TLS certificates from a PEM file.
#[cfg(feature = "tls")]
fn load_tls_certs(path: &str) -> Result<Vec<rustls::pki_types::CertificateDer<'static>>> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open TLS certificate file: {}", path))?;
    let mut reader = std::io::BufReader::new(file);
    let certs: Vec<_> = rustls_pemfile::certs(&mut reader)
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("Failed to parse TLS certificates from: {}", path))?;
    if certs.is_empty() {
        anyhow::bail!("No certificates found in {}", path);
    }
    Ok(certs)
}

/// Load a TLS private key from a PEM file.
#[cfg(feature = "tls")]
fn load_tls_key(path: &str) -> Result<rustls::pki_types::PrivateKeyDer<'static>> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open TLS key file: {}", path))?;
    let mut reader = std::io::BufReader::new(file);
    rustls_pemfile::private_key(&mut reader)
        .with_context(|| format!("Failed to parse TLS key from: {}", path))?
        .ok_or_else(|| anyhow::anyhow!("No private key found in {}", path))
}

/// Model registration with handler
pub struct ModelRegistration {
    pub name: String,
    pub base_path: String,
    pub data_path: String,
    pub handler: Arc<dyn ModelHandler>,
    pub schema_extractor: Option<SchemaSpecExtractor>,
}

/// Deferred session-gate applier closure. Registered by `with_handler` paths
/// that bypass the `model_infos` pipeline; invoked at `serve()` time so the
/// `models_require_session` flag is propagated to externally-constructed
/// handlers through interior mutability on `DeclarativeHttpHandler`.
pub(crate) type ExternalHandlerGate = Box<dyn Fn(bool) + Send + Sync>;

/// Deferred SSE-broadcaster wiring closure. Registered by `with_handler` paths
/// (and therefore by `with_model_ref`, which delegates to `with_handler`) that
/// bypass the `model_infos` pipeline; invoked at `serve()` time to install the
/// builder-level SSE broadcaster onto externally-constructed handlers through
/// `OnceLock` interior mutability on `DeclarativeHttpHandler`. Issue #91.
pub(crate) type ExternalHandlerSseWiring =
    Box<dyn Fn(Arc<crate::http::sse::SseEventBroadcaster>) + Send + Sync>;

/// Lithair multi-model server
pub struct LithairServer {
    config: LithairConfig,
    session_manager: Option<Arc<dyn std::any::Any + Send + Sync>>,
    custom_routes: Vec<CustomRoute>,
    not_found_handler: Option<RouteHandler>,
    route_guards: Vec<crate::http::RouteGuardMatcher>,
    model_infos: Vec<ModelRegistrationInfo>,
    /// Issue #78: when true, every auto-generated `/api/{model}` endpoint
    /// must carry a valid session, otherwise the request is rejected with
    /// HTTP 401. Wired from
    /// `LithairServerBuilder::with_models_require_session(...)` and applied
    /// uniformly to every model handler in `serve()` after each model's
    /// factory has produced its handler.
    models_require_session: bool,

    /// Issue #86: deferred session-gate appliers for models registered via
    /// `LithairServerBuilder::with_handler(...)`. Each closure captures an
    /// `Arc<DeclarativeHttpHandler<T>>` clone and calls `set_require_session`
    /// on it. Executed in `serve()` after the boot-time session-store shape
    /// check, only when `models_require_session` is `true`. Without this, the
    /// `with_handler` path silently bypassed the gate even when the operator
    /// asked for it via `with_models_require_session(true)`.
    external_handler_gates: Vec<ExternalHandlerGate>,

    /// Issue #91: deferred SSE-broadcaster wirings for models registered via
    /// `LithairServerBuilder::with_handler(...)` (including `with_model_ref`,
    /// which delegates to it). Each closure captures an
    /// `Arc<DeclarativeHttpHandler<T>>` clone and calls
    /// `set_sse_broadcaster_shared` on it. Executed in `serve()` only when a
    /// builder-level broadcaster is configured (i.e. the user called
    /// `.with_sse(true)`). Without this, the `with_handler` path silently
    /// started with no broadcaster wired, so `apply_replicated_*` broadcasts
    /// from issue #89 became no-ops and `/api/{model}/stream` returned 404.
    external_handler_sse_wirings: Vec<ExternalHandlerSseWiring>,

    models: Arc<tokio::sync::RwLock<Vec<ModelRegistration>>>,

    // Frontend configurations to load (path_prefix -> static_dir)
    frontend_configs: Vec<(String, String)>,

    // Frontend serving (SCC2 memory-first) - Multiple frontends with path prefixes
    // Key: route_prefix (e.g., "/", "/admin"), Value: FrontendEngine
    frontend_engines: std::collections::HashMap<String, Arc<crate::frontend::FrontendEngine>>,

    // Per-vhost frontend configurations declared via
    // `LithairServerBuilder::with_vhost` / `with_default_vhost`. Loaded
    // into `vhost_frontend_router` at startup.
    vhost_frontend_configs: Vec<(crate::app::builder::VhostScope, String, String)>,

    // Host-header based frontend router built at startup from
    // `vhost_frontend_configs`. Each value is a map of route_prefix ->
    // FrontendEngine for that vhost, mirroring `frontend_engines` but
    // scoped to a specific `Host:` header.
    //
    // When empty (no vhosts declared) the server falls back entirely to
    // `frontend_engines`, preserving the pre-feature behaviour.
    vhost_frontend_router: crate::http::HostRouter<
        std::collections::HashMap<String, Arc<crate::frontend::FrontendEngine>>,
    >,

    // Host-to-host 301 redirects declared via
    // [`crate::app::LithairServerBuilder::with_redirect`]. Looked up by
    // the request's normalized `Host:` header *before* any vhost frontend
    // dispatch — see `handle_request`. The value stored is the canonical
    // target host (already normalized).
    //
    // When empty (no redirects declared) the server behaves as before.
    host_redirects: crate::http::HostRouter<String>,

    // HTTP Features
    firewall_config: Option<crate::http::FirewallConfig>,
    anti_ddos_config: Option<crate::security::anti_ddos::AntiDDoSConfig>,
    access_log: bool,
    access_log_capacity: usize,
    openapi_enabled: bool,
    openapi_spec_cache: std::sync::OnceLock<serde_json::Value>,

    // Raft cluster (distributed consensus)
    cluster_peers: Vec<String>,
    node_id: Option<u64>,
    raft_state: Option<Arc<RaftLeadershipState>>,

    // Raft CRUD consensus channel - for submitting CRUD operations through Raft
    // When Some and cluster_peers is non-empty, all writes go through Raft consensus
    #[allow(dead_code)]
    raft_crud_sender: Option<tokio::sync::mpsc::Sender<RaftCrudOperation>>,

    // Consensus log for ordered CRUD operations
    consensus_log: Option<Arc<crate::cluster::ConsensusLog>>,

    // Write-Ahead Log for durability (WAL ensures operations survive crashes)
    wal: Option<Arc<crate::cluster::WriteAheadLog>>,

    // Replication batcher for intelligent batching and follower health tracking
    replication_batcher: Option<Arc<crate::cluster::ReplicationBatcher>>,

    // Snapshot manager for full state snapshots (resync of desynced followers)
    snapshot_manager: Option<Arc<tokio::sync::RwLock<crate::cluster::SnapshotManager>>>,

    // Migration manager for rolling upgrades
    migration_manager: Option<Arc<crate::cluster::MigrationManager>>,

    // Resync statistics for observability
    resync_stats: Arc<crate::cluster::ResyncStats>,

    // Schema synchronization state for cluster-wide schema consensus
    schema_sync_state: Arc<tokio::sync::RwLock<crate::schema::SchemaSyncState>>,

    // SSE real-time subscriptions broadcaster (shared across all model handlers)
    sse_broadcaster: Option<Arc<crate::http::sse::SseEventBroadcaster>>,

    // Issue #69: builder-driven auto-compaction config. When `Some`,
    // `serve()` spawns one tokio task per registered model that
    // periodically inspects the model's `EventStore::event_count()` and
    // triggers `EventStore::truncate_events()` once the count crosses
    // `events_threshold`. The spawned tasks are fire-and-forget — runtime
    // shutdown aborts them, matching the existing background-flusher
    // lifecycle in `DeclarativeHttpHandler::new`.
    //
    // `None` = feature off (default, no observable behavior change).
    auto_compaction: Option<crate::engine::AutoCompactionConfig>,
}

/// A CRUD operation to be submitted through Raft consensus
#[derive(Debug)]
pub struct RaftCrudOperation {
    pub operation: crate::cluster::CrudOperation,
    pub response_tx: tokio::sync::oneshot::Sender<Result<serde_json::Value, String>>,
}

/// Type alias for async route handlers, as stored internally after a call to
/// [`LithairServerBuilder::with_route`] or
/// [`LithairServerBuilder::with_route_async`].
///
/// The handler input/output types are exposed as [`RouteRequest`] and
/// [`RouteResponse`] for consumers; this alias just bundles the
/// `Arc<dyn Fn(...) -> Pin<Box<dyn Future>>>` machinery used by the dispatcher.
pub type RouteHandler = Arc<
    dyn Fn(
            RouteRequest,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<RouteResponse>> + Send>>
        + Send
        + Sync,
>;

/// Custom route registration
pub struct CustomRoute {
    pub method: http::Method,
    pub path: String,
    pub handler: RouteHandler,
}

/// Type for async model handler factory
pub type ModelFactory = Arc<
    dyn Fn(
            String,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Arc<dyn ModelHandler>>> + Send>,
        > + Send
        + Sync,
>;

/// Type for schema spec extractor function
pub type SchemaSpecExtractor = Arc<dyn Fn() -> crate::schema::ModelSpec + Send + Sync>;

/// Model registration info with factory
pub struct ModelRegistrationInfo {
    pub name: String,
    pub base_path: String,
    pub data_path: String,
    pub factory: ModelFactory,
    /// Optional schema spec extractor for migration detection
    pub schema_extractor: Option<SchemaSpecExtractor>,
    /// Whether the builder-level `with_models_require_session(true)` switch
    /// should apply to this registration (issue #78). Set to `true` for
    /// models registered via `with_model(...)` / `with_declarative_model(...)`,
    /// `false` for `with_model_full(...)` — that path already supports RBAC
    /// via `PermissionChecker` and the issue #78 flag is intentionally
    /// scoped to the simple-CRUD path.
    pub require_session_applies: bool,
}

impl LithairServer {
    /// Create a new Lithair server with default configuration
    ///
    /// Configuration is loaded with full supersedence:
    /// 1. Defaults
    /// 2. Config file (config.toml)
    /// 3. Environment variables
    /// 4. Code (builder methods)
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> LithairServerBuilder {
        LithairServerBuilder::new()
    }

    /// Create a new server with custom configuration
    pub fn with_config(config: LithairConfig) -> LithairServerBuilder {
        LithairServerBuilder::with_config(config)
    }

    /// Validate schemas for all registered models with schema extractors
    ///
    /// This compares stored schema specs with current specs and handles
    /// differences based on the configured migration mode.
    async fn validate_schemas(&self) -> Result<()> {
        use crate::config::SchemaMigrationMode;
        use crate::schema::{
            load_schema_spec, save_schema_spec, AppliedSchemaChange, PendingSchemaChange,
            SchemaChangeDetector,
        };
        use std::path::Path;

        let base_path = Path::new(&self.config.storage.data_dir);
        let mode = self.config.storage.schema_migration_mode;
        let is_cluster = !self.cluster_peers.is_empty();
        let node_id = self.node_id.unwrap_or(0);

        log::info!("Validating model schemas...");
        if is_cluster {
            log::info!("   Cluster mode: schema changes will be synchronized");
        }

        let mut has_breaking_changes = false;

        for info in &self.model_infos {
            // Skip models without schema extractors
            let extractor = match &info.schema_extractor {
                Some(e) => e,
                None => {
                    log::debug!("   {} - no schema extractor, skipping", info.name);
                    continue;
                }
            };

            // Extract current schema
            let current_spec = extractor();

            // Load stored schema (if exists)
            let stored_spec = match load_schema_spec(&info.name, base_path) {
                Ok(spec) => spec,
                Err(e) => {
                    log::warn!("   {} - failed to load stored schema: {}", info.name, e);
                    None
                }
            };

            match stored_spec {
                Some(stored) => {
                    // Compare schemas
                    let changes = SchemaChangeDetector::detect_changes(&stored, &current_spec);

                    if changes.is_empty() {
                        log::info!(
                            "   {} - schema unchanged (v{})",
                            info.name,
                            current_spec.version
                        );

                        // In cluster mode, update local sync state
                        if is_cluster {
                            let mut state = self.schema_sync_state.write().await;
                            state.schemas.insert(info.name.clone(), current_spec.clone());
                        }
                    } else {
                        // Check if schema migrations are locked
                        {
                            let state = self.schema_sync_state.read().await;
                            if state.lock_status.is_locked() {
                                log::error!(
                                    "   {} - schema changes BLOCKED (migrations locked)",
                                    info.name
                                );
                                log::error!(
                                    "      Reason: {}",
                                    state.lock_status.reason.as_deref().unwrap_or("none")
                                );
                                log::error!("      Unlock via: POST /_admin/schema/unlock");
                                has_breaking_changes = true; // Will cause failure in strict mode
                                continue; // Skip this model, check next
                            }
                        }

                        log::warn!(
                            "   {} - {} schema change(s) detected:",
                            info.name,
                            changes.len()
                        );

                        for change in &changes {
                            let field = change.field_name.as_deref().unwrap_or("model");
                            log::warn!(
                                "      - {:?} on '{}' ({:?})",
                                change.change_type,
                                field,
                                change.migration_strategy
                            );

                            if change.requires_consensus {
                                has_breaking_changes = true;
                            }
                        }

                        // Handle based on mode and cluster status
                        if is_cluster {
                            // In cluster mode: create pending change for consensus
                            let pending = PendingSchemaChange::new(
                                info.name.clone(),
                                node_id,
                                changes.clone(),
                                current_spec.clone(),
                                Some(stored.clone()),
                            );

                            let mut state = self.schema_sync_state.write().await;
                            let policy = state.policy.clone();
                            let strategy = policy.strategy_for(&pending.overall_strategy);

                            match strategy {
                                crate::schema::VoteStrategy::AutoAccept => {
                                    log::info!(
                                        "      Cluster: auto-accepting {:?} change",
                                        pending.overall_strategy
                                    );
                                    state.schemas.insert(info.name.clone(), current_spec.clone());
                                    // Record in history
                                    let applied = AppliedSchemaChange {
                                        id: uuid::Uuid::new_v4(),
                                        model_name: info.name.clone(),
                                        changes: changes.clone(),
                                        applied_at: std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_millis()
                                            as u64,
                                        applied_by_node: node_id,
                                    };
                                    state.change_history.push(applied.clone());
                                    // Persist history to disk
                                    if let Err(e) =
                                        crate::schema::append_schema_history(&applied, base_path)
                                    {
                                        log::error!(
                                            "      Failed to persist schema history: {}",
                                            e
                                        );
                                    }
                                    // Also save locally
                                    if let Err(e) = save_schema_spec(&current_spec, base_path) {
                                        log::error!("      Failed to save updated schema: {}", e);
                                    }
                                }
                                crate::schema::VoteStrategy::Reject => {
                                    log::error!(
                                        "      Cluster: rejecting {:?} change (policy)",
                                        pending.overall_strategy
                                    );
                                    has_breaking_changes = true;
                                }
                                _ => {
                                    // Consensus or ManualApproval required
                                    log::info!("      Cluster: change requires {:?}", strategy);
                                    state.add_pending(pending);
                                    // Node should wait or be blocked until approval
                                    // Note: Blocking on pending approval is not yet supported
                                    log::warn!("      ⏳ Schema change pending approval - check /_admin/schema/pending");
                                }
                            }
                        } else {
                            // Non-cluster mode: behavior depends on migration mode
                            match mode {
                                SchemaMigrationMode::Strict => {
                                    // Will fail after logging all changes
                                }
                                SchemaMigrationMode::Auto => {
                                    // Save new schema (actual data migration not implemented yet)
                                    if let Err(e) = save_schema_spec(&current_spec, base_path) {
                                        log::error!("      Failed to save updated schema: {}", e);
                                    } else {
                                        log::info!(
                                            "      Schema updated to v{}",
                                            current_spec.version
                                        );
                                        // Record in history (non-cluster mode)
                                        let applied = AppliedSchemaChange {
                                            id: uuid::Uuid::new_v4(),
                                            model_name: info.name.clone(),
                                            changes: changes.clone(),
                                            applied_at: std::time::SystemTime::now()
                                                .duration_since(std::time::UNIX_EPOCH)
                                                .unwrap_or_default()
                                                .as_millis()
                                                as u64,
                                            applied_by_node: node_id,
                                        };
                                        let mut state = self.schema_sync_state.write().await;
                                        state.change_history.push(applied.clone());
                                        // Persist history to disk
                                        if let Err(e) = crate::schema::append_schema_history(
                                            &applied, base_path,
                                        ) {
                                            log::error!(
                                                "      Failed to persist schema history: {}",
                                                e
                                            );
                                        }
                                    }
                                }
                                SchemaMigrationMode::Manual => {
                                    // Create pending change requiring manual approval (even in standalone)
                                    let pending = PendingSchemaChange::new(
                                        info.name.clone(),
                                        node_id,
                                        changes.clone(),
                                        current_spec.clone(),
                                        Some(stored.clone()),
                                    );
                                    log::info!(
                                        "      Manual mode: change pending approval (id: {})",
                                        pending.id
                                    );
                                    log::warn!(
                                        "      ⏳ Approve via: POST /_admin/schema/approve/{}",
                                        pending.id
                                    );

                                    let mut state = self.schema_sync_state.write().await;
                                    state.add_pending(pending);
                                }
                                SchemaMigrationMode::Warn => {
                                    // Just log, already done
                                }
                            }
                        }
                    }
                }
                None => {
                    // First run - save initial schema
                    log::info!(
                        "   {} - first run, saving schema v{}",
                        info.name,
                        current_spec.version
                    );
                    if let Err(e) = save_schema_spec(&current_spec, base_path) {
                        log::error!("      Failed to save initial schema: {}", e);
                    }

                    // In cluster mode, update local sync state
                    if is_cluster {
                        let mut state = self.schema_sync_state.write().await;
                        state.schemas.insert(info.name.clone(), current_spec.clone());
                    }
                }
            }
        }

        // Fail in strict mode if breaking changes detected
        if mode == SchemaMigrationMode::Strict && has_breaking_changes {
            anyhow::bail!("Schema validation failed: breaking changes detected in strict mode");
        }

        log::info!("Schema validation complete");
        Ok(())
    }

    /// Start the server
    pub async fn serve(mut self) -> Result<()> {
        // Load persisted schema history and lock status
        {
            use crate::schema::{load_lock_status, load_schema_history};
            use std::path::Path;

            let base_path = Path::new(&self.config.storage.data_dir);

            // Load history
            match load_schema_history(base_path) {
                Ok(history) => {
                    let mut state = self.schema_sync_state.write().await;
                    state.change_history = history.changes;
                    if !state.change_history.is_empty() {
                        log::info!(
                            "Loaded {} schema change(s) from history",
                            state.change_history.len()
                        );
                    }
                }
                Err(e) => {
                    log::warn!("Failed to load schema history: {}", e);
                }
            }

            // Load lock status
            match load_lock_status(base_path) {
                Ok(lock) => {
                    let mut state = self.schema_sync_state.write().await;
                    state.lock_status = lock;
                    if state.lock_status.is_locked() {
                        log::warn!("Schema migrations are LOCKED (persisted state)");
                    }
                }
                Err(e) => {
                    log::warn!("Failed to load schema lock status: {}", e);
                }
            }
        }

        // Schema validation for models with schema extractors
        if self.config.storage.schema_validation_enabled {
            self.validate_schemas().await?;
        }

        // Issue #80 — fail-fast at boot when the operator opted into the
        // `with_models_require_session(true)` gate but the registered
        // session manager has a shape the gate can't recognize at request
        // time (`has_valid_session` in `http/declarative.rs` only handles
        // `Arc<PersistentSessionStore>` and
        // `Arc<SessionManager<PersistentSessionStore>>`).
        //
        // The classic mis-wire is `SessionManager::new(arc_store)` where
        // `arc_store: Arc<PersistentSessionStore>` — the generic resolves
        // to `S = Arc<PersistentSessionStore>` and the stored type
        // becomes `Arc<SessionManager<Arc<PersistentSessionStore>>>` (a
        // double-Arc shape the downcast misses). Pre-fix, every request
        // 401'd silently. Now we refuse to start, point the operator at
        // the right constructor (`SessionManager::from_arc`), and never
        // bind the port.
        //
        // Only fires when the gate is actually opt-in (`models_require_session`)
        // AND at least one model registration is opted-in via the
        // simple-CRUD path. `with_model_full` registrations carry their
        // own RBAC story and the gate doesn't cover them, so a mismatched
        // shape doesn't cause a silent auth-bypass for them.
        if self.models_require_session && self.model_infos.iter().any(|i| i.require_session_applies)
        {
            if let Some(ref store_any) = self.session_manager {
                // Single source of truth for which shapes the gate
                // recognizes. Defined in `lithair-core/src/session/mod.rs`
                // and consumed both here and in
                // `http/declarative.rs::has_valid_session`. Adding a new
                // supported shape in one place automatically extends the
                // other — no more drift between constructor surface and
                // runtime downcast (the original cause of issue #80).
                if crate::session::RecognizedSessionStore::recognize(store_any).is_none() {
                    // The stored `Arc<dyn Any>` doesn't carry its concrete
                    // type name as a string. We at least preserve the
                    // `TypeId` so an operator with access to a debug
                    // build can correlate it.
                    let actual_type_id = (**store_any).type_id();
                    anyhow::bail!(
                        "Refusing to start: `with_models_require_session(true)` is set \
                         but the registered session store has an unrecognized shape \
                         (TypeId = {:?}). The gate only recognizes \
                         `Arc<PersistentSessionStore>` and \
                         `Arc<SessionManager<PersistentSessionStore>>`. This usually \
                         means `SessionManager::new(arc_store)` was called with an \
                         already-`Arc`-wrapped store, producing a double-`Arc` shape \
                         that silently 401s every request. Use \
                         `SessionManager::from_arc(arc_store)` instead, or pass the \
                         store by value to `SessionManager::new`. See issue #80.",
                        actual_type_id
                    );
                }
            } else {
                // Flag is on but no session store was ever wired. This is
                // already a noisy 401-on-everything situation
                // (`has_valid_session` returns false unconditionally),
                // but pre-issue-#80 we shipped it silently. Failing fast
                // here matches the operator's intent: they asked for
                // gating, the framework cannot honor it, refuse to start.
                anyhow::bail!(
                    "Refusing to start: `with_models_require_session(true)` is set \
                     but no session store was registered. Add `.with_sessions(...)` \
                     before `.serve()`, or remove the require-session flag."
                );
            }
        }

        // Create model handlers from factories
        for info in &self.model_infos {
            log::info!("Creating handler for model: {}", info.name);
            match (info.factory)(info.data_path.clone()).await {
                Ok(mut handler) => {
                    // Wire SSE broadcaster into each model handler. Works
                    // through `&self` since #91 — `DeclarativeHttpHandler`
                    // stores the broadcaster in a `OnceLock` and the trait
                    // method takes `&self`. The previous `Arc::get_mut`
                    // dance + warn-on-shared-Arc is no longer needed; any
                    // shared Arc still receives the broadcaster cleanly.
                    if let Some(ref broadcaster) = self.sse_broadcaster {
                        handler.set_sse_broadcaster(Arc::clone(broadcaster));
                    }

                    // Wire the session store into every model handler.
                    //
                    // Pre-issue-#78, `with_model(...)` never threaded the
                    // session store through (only `with_model_full` did), so
                    // even when sessions were configured the simple-CRUD
                    // path had no way to look one up. We now plumb it here,
                    // uniformly, after the factory has produced the handler.
                    // This makes builder-method ordering irrelevant.
                    //
                    // `with_model_full` may have already attached its own
                    // store via the owned setter; this call overwrites it
                    // with the builder-level store. That is intentional —
                    // the builder-level configuration is authoritative
                    // (any RBAC checker provided via `with_model_full`
                    // continues to use the same shared store).
                    //
                    // We fail-fast (bail) rather than warn here: if a
                    // factory hands back a shared `Arc`, the session store
                    // wiring would silently drop and RBAC / the #78 gate
                    // would both become no-ops. That's a security-relevant
                    // silent failure — better to refuse to start than
                    // serve unauthenticated traffic that the operator
                    // believed to be gated. (Built-in factories return a
                    // fresh `Arc::new(handler)` so this never triggers in
                    // practice; the bail is a guard against external
                    // factory implementations that misbehave.)
                    if let Some(ref store) = self.session_manager {
                        if let Some(h) = Arc::get_mut(&mut handler) {
                            h.set_session_store_any(Arc::clone(store));
                        } else {
                            anyhow::bail!(
                                "Could not wire session store for model '{}': handler Arc has multiple strong references — refusing to start to avoid a silent auth-bypass",
                                info.name
                            );
                        }
                    }

                    // Apply the issue #78 require-session flag — but only
                    // to registrations that opted in via the simple-CRUD
                    // path. `with_model_full(...)` registrations carry
                    // their own RBAC story (see PermissionChecker) and the
                    // flag is intentionally scoped to NOT cover them.
                    if self.models_require_session && info.require_session_applies {
                        if let Some(h) = Arc::get_mut(&mut handler) {
                            h.set_require_session(true);
                        } else {
                            anyhow::bail!(
                                "Could not enable require-session for model '{}': handler Arc has multiple strong references — refusing to start because the operator-requested gate would silently not engage",
                                info.name
                            );
                        }
                    }
                    let mut models = self.models.write().await;
                    models.push(ModelRegistration {
                        name: info.name.clone(),
                        base_path: info.base_path.clone(),
                        data_path: info.data_path.clone(),
                        handler,
                        schema_extractor: info.schema_extractor.clone(),
                    });
                    log::info!("Handler created for {}", info.name);
                }
                Err(e) => {
                    log::error!("Failed to create handler for {}: {}", info.name, e);
                    return Err(e.context(format!("Failed to create handler for {}", info.name)));
                }
            }
        }
        self.model_infos.clear(); // Clear infos, we have the models now

        // Issue #86: apply the session-presence gate to every model
        // registered via `LithairServerBuilder::with_handler(...)`. Each
        // closure was pushed at registration time with a captured
        // `Arc::clone(&handler)`, so flipping the flag here propagates to
        // every CRUD route closure that was registered through `with_route`
        // — they all dispatch into `handler.handle_request(...)`, which
        // reads `require_session` via `AtomicBool::load`.
        //
        // Pre-fix, this path silently never gated, so a consumer who
        // switched from `.with_model::<T>(...)` to `.with_handler(...)`
        // (to gain a programmatic handle on the handler) lost the gate
        // without warning. See issue #86 for the original repro.
        //
        // Note: we do NOT bail on a missing session store here. The
        // boot-time fail-fast check above (line ~707) already covers the
        // mis-wire case for the `with_model` path; for `with_handler`,
        // the responsibility for wiring the session store onto the
        // handler currently sits with the caller (they constructed the
        // handler externally). Without a store, `has_valid_session`
        // returns `false` on every request, which combined with the gate
        // means the route returns 401 unconditionally — failing closed
        // is the safe direction here.
        let gates = std::mem::take(&mut self.external_handler_gates);
        if self.models_require_session {
            for gate in &gates {
                gate(true);
            }
        }

        // Issue #91: install the builder-level SSE broadcaster onto every
        // handler registered via `with_handler(...)` (including via
        // `with_model_ref`, which delegates to it). Mirrors the wiring loop
        // above for the factory `with_model` path at line ~774 — same
        // production semantics (one broadcaster per server, shared across
        // all model handlers), different installation surface (interior
        // mutability via `OnceLock` instead of `Arc::get_mut`, because
        // `with_handler` consumers already hold an Arc clone).
        //
        // The closure is only invoked when the builder has a broadcaster,
        // i.e. when `.with_sse(true)` was called on the builder. Without
        // a broadcaster, every wiring is a no-op and the handlers stay
        // with their default empty `OnceLock` — backward-compat for every
        // `with_handler` consumer who never opted into SSE.
        let sse_wirings = std::mem::take(&mut self.external_handler_sse_wirings);
        if let Some(ref broadcaster) = self.sse_broadcaster {
            for wire in &sse_wirings {
                wire(Arc::clone(broadcaster));
            }
        }

        // Issue #69: spawn one auto-compaction task per registered model
        // when the feature is enabled. The task body mirrors the one
        // covered in `tests/auto_compaction_test.rs::spawn_auto_compaction`
        // — keep them in sync if either is touched.
        //
        // Lifecycle matches the existing background-flusher pattern in
        // `DeclarativeHttpHandler::new` (line ~180): spawned and forgotten,
        // runtime shutdown aborts. We deliberately do NOT hold JoinHandles
        // on `LithairServer` — that would force a shutdown signal we don't
        // currently have, and it would diverge from the flusher's
        // lifecycle. If/when graceful shutdown lands, both code paths
        // should grow JoinHandle tracking together.
        // Initialize default logger BEFORE spawning auto-compaction tasks
        // (issue #69 follow-up — addresses Gemini review on PR #84).
        // Previously this `try_init` ran after the spawn loop, so any
        // `log::info!` / `log::warn!` emitted by the spawned tasks before
        // this point routed to the fallback (silently dropped under the
        // default `RUST_LOG` filter).
        let _ = env_logger::Builder::from_default_env()
            .format_timestamp_millis()
            .format_module_path(false)
            .try_init(); // Use try_init to avoid panic if already initialized

        if let Some(cfg) = self.auto_compaction {
            let models = self.models.read().await;
            for reg in models.iter() {
                // Skip handlers that don't event-source — their `compact()`
                // is a no-op but we'd still pay the per-tick lock acquire.
                // The event-store handle is also what we read `event_count()`
                // from on each tick; without it, no threshold check is
                // meaningful.
                let Some(event_store) = reg.handler.event_store_arc() else {
                    log::debug!(
                        "Auto-compaction: model '{}' has no EventStore, skipping",
                        reg.name
                    );
                    continue;
                };
                let handler = Arc::clone(&reg.handler);
                let model_name = reg.name.clone();
                log::info!(
                    "Auto-compaction enabled for model '{}': threshold={}, interval={:?}",
                    model_name,
                    cfg.events_threshold,
                    cfg.check_interval
                );
                tokio::spawn(async move {
                    let mut ticker = tokio::time::interval(cfg.check_interval);
                    // `MissedTickBehavior::Skip` — if the system is under
                    // load and several check intervals elapse between
                    // `.tick()` resolutions, fire once and align to the
                    // next interval (don't burst-fire a compaction check
                    // multiple times in a row). Default `Burst` would
                    // hammer a maintenance task that should be paced.
                    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                    // Skip the immediate first tick — `tokio::time::interval`
                    // fires immediately on the first `.tick()` which would
                    // be a spurious read of an empty just-started store.
                    ticker.tick().await;
                    loop {
                        ticker.tick().await;
                        let needs_compaction = {
                            let store = event_store.read().await;
                            store.event_count() > cfg.events_threshold
                        };
                        if !needs_compaction {
                            continue;
                        }
                        log::info!(
                            "Auto-compaction: model '{}' crossed threshold {}, compacting (snapshot + truncate)",
                            model_name,
                            cfg.events_threshold
                        );
                        // Delegate to the handler's `compact()` — it owns
                        // the snapshot+truncate atomicity guarantee. The
                        // server-side loop intentionally does NOT touch
                        // `EventStore::truncate_events()` directly: that
                        // path caused the data-loss bug Gemini flagged on
                        // PR #84 (truncate without snapshot = next replay
                        // sees an empty log = state lost).
                        if let Err(e) = handler.compact().await {
                            log::warn!(
                                "Auto-compaction: model '{}' compact() failed: {}",
                                model_name,
                                e
                            );
                        }
                    }
                });
            }
        }

        // Validate configuration
        self.config.validate()?;

        log::info!("Starting Lithair Server");
        log::info!("   Port: {}", self.config.server.port);
        log::info!("   Host: {}", self.config.server.host);
        log::info!(
            "   Sessions: {}",
            if self.config.sessions.enabled { "enabled" } else { "disabled" }
        );
        log::info!("   RBAC: {}", if self.config.rbac.enabled { "enabled" } else { "disabled" });
        log::info!("   Admin: {}", if self.config.admin.enabled { "enabled" } else { "disabled" });
        log::info!("   Models: {}", self.models.read().await.len());
        log::info!("   Custom routes: {}", self.custom_routes.len());

        // Load frontend assets - support both old config and new multi-frontend approach
        let mut frontends_to_load = Vec::new();

        // Add legacy frontend config if enabled
        if self.config.frontend.enabled {
            if let Some(ref static_dir) = self.config.frontend.static_dir {
                frontends_to_load.push(("/".to_string(), static_dir.clone()));
            }
        }

        // Add new multi-frontend configs
        frontends_to_load.extend(self.frontend_configs.clone());

        // Load each frontend
        for (route_prefix, static_dir) in frontends_to_load {
            log::info!("Loading frontend at '{}' from {}...", route_prefix, static_dir);

            // Create unique host_id from route_prefix
            let host_id = if route_prefix == "/" {
                "default".to_string()
            } else {
                route_prefix.trim_matches('/').replace('/', "_")
            };

            // Create FrontendEngine (SCC2 lock-free with event sourcing)
            match crate::frontend::FrontendEngine::new(&host_id, "./data/frontend").await {
                Ok(engine) => {
                    log::info!("   FrontendEngine created (host_id: {})", host_id);

                    // Load assets into memory
                    match engine.load_directory(&static_dir).await {
                        Ok(count) => {
                            log::info!("   {} assets loaded (40M+ ops/sec)", count);
                            self.frontend_engines.insert(route_prefix.clone(), Arc::new(engine));
                        }
                        Err(e) => {
                            log::warn!("   Could not load frontend assets: {}", e);
                        }
                    }
                }
                Err(e) => {
                    log::warn!("   Could not create frontend engine: {}", e);
                }
            }
        }

        // Load per-vhost frontends (host-header based routing, see
        // `lithair/lithair#30`). Each vhost gets its own set of
        // FrontendEngines keyed by route prefix, exactly like the
        // host-agnostic `frontend_engines` above. The resulting map is
        // stored under the normalized host in `vhost_frontend_router`.
        if !self.vhost_frontend_configs.is_empty() {
            use crate::app::builder::VhostScope;

            // Group configs by scope so we iterate each vhost once.
            // `serve` owns `mut self` and `vhost_frontend_configs` is not
            // used again after startup, so take it instead of cloning.
            let mut by_scope: std::collections::HashMap<VhostScope, Vec<(String, String)>> =
                std::collections::HashMap::new();
            for (scope, prefix, dir) in std::mem::take(&mut self.vhost_frontend_configs) {
                by_scope.entry(scope).or_default().push((prefix, dir));
            }

            for (scope, entries) in by_scope {
                let scope_label = match &scope {
                    VhostScope::Host(h) => h.clone(),
                    VhostScope::Default => "<default>".to_string(),
                };
                log::info!("Loading vhost '{}' ({} frontends)", scope_label, entries.len());

                let mut engines: std::collections::HashMap<
                    String,
                    Arc<crate::frontend::FrontendEngine>,
                > = std::collections::HashMap::new();

                // Injective byte-level encoder used for host_id segments:
                // ASCII alphanumerics and '-' pass through, everything else
                // becomes `_<hex>`. This makes the (vhost, prefix) → host_id
                // mapping collision-free — otherwise pairs like ("foo.bar",
                // "/") and ("foo_bar", "/"), or "/a/b" and "/a_b", would
                // collapse onto the same on-disk SCC2 store.
                let stable_host_id_segment = |input: &str| -> String {
                    let mut out = String::with_capacity(input.len() * 3);
                    for b in input.bytes() {
                        match b {
                            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' => out.push(b as char),
                            _ => {
                                use std::fmt::Write as _;
                                let _ = write!(&mut out, "_{b:02x}");
                            }
                        }
                    }
                    out
                };

                for (route_prefix, static_dir) in entries {
                    // Compose a stable host_id unique to (vhost, prefix)
                    // so the two frontends don't clobber each other's
                    // SCC2 event store.
                    let prefix_segment = stable_host_id_segment(&route_prefix);
                    let scope_segment = match &scope {
                        VhostScope::Host(h) => stable_host_id_segment(h),
                        VhostScope::Default => "_default".to_string(),
                    };
                    let host_id = format!("vhost_{}__{}", scope_segment, prefix_segment);

                    match crate::frontend::FrontendEngine::new(&host_id, "./data/frontend").await {
                        Ok(engine) => match engine.load_directory(&static_dir).await {
                            Ok(count) => {
                                log::info!(
                                    "   [{}] {} → {} ({} assets)",
                                    scope_label,
                                    route_prefix,
                                    static_dir,
                                    count
                                );
                                engines.insert(route_prefix, Arc::new(engine));
                            }
                            Err(e) => {
                                log::warn!(
                                    "   [{}] could not load {} -> {}: {}",
                                    scope_label,
                                    route_prefix,
                                    static_dir,
                                    e
                                );
                            }
                        },
                        Err(e) => {
                            log::warn!(
                                "   [{}] could not create frontend engine for {}: {}",
                                scope_label,
                                route_prefix,
                                e
                            );
                        }
                    }
                }

                match scope {
                    VhostScope::Host(h) => {
                        self.vhost_frontend_router.insert(h, engines);
                    }
                    VhostScope::Default => {
                        self.vhost_frontend_router.set_default(engines);
                    }
                }
            }
        }

        // Log HTTP features
        if self.firewall_config.is_some() {
            log::info!("   Firewall enabled");
        }
        if self.anti_ddos_config.is_some() {
            log::info!("   Anti-DDoS protection enabled");
        }

        // Initialize Raft cluster if configured
        if self.config.raft.enabled && !self.cluster_peers.is_empty() {
            if let Some(node_id) = self.node_id {
                let port = self.config.server.port;

                log::info!("Initializing Raft cluster...");
                log::info!("   Node ID: {}", node_id);
                log::info!("   Peers: {:?}", self.cluster_peers);
                log::info!("   Raft path: {}", self.config.raft.path);
                log::info!(
                    "   Raft auth: {}",
                    if self.config.raft.auth_required { "enabled" } else { "disabled" }
                );

                let raft_state =
                    Arc::new(RaftLeadershipState::new(node_id, port, self.cluster_peers.clone()));

                if raft_state.is_leader() {
                    log::info!("THIS NODE IS THE LEADER");
                } else {
                    log::info!(
                        "This node is a FOLLOWER (leader port: {})",
                        raft_state.get_leader_port()
                    );
                }

                self.raft_state = Some(raft_state);

                // Initialize replication batcher with peers
                if let Some(ref batcher) = self.replication_batcher {
                    batcher.initialize(&self.cluster_peers).await;
                    log::info!(
                        "Replication batcher initialized with {} peers",
                        self.cluster_peers.len()
                    );
                }

                // ── WAL REPLAY ──────────────────────────────────────────────
                // On restart, the WAL contains all committed operations from
                // before the crash. We replay them into the ConsensusLog to
                // restore the node's state without losing data.
                if let (Some(ref wal), Some(ref consensus_log)) = (&self.wal, &self.consensus_log) {
                    match wal.read_all() {
                        Ok(entries) if !entries.is_empty() => {
                            let count = consensus_log.replay_from_wal_entries(entries).await;
                            log::info!(
                                "WAL replay: restored {} entries (term={}, commit_index={})",
                                count,
                                consensus_log.current_term(),
                                consensus_log.commit_index(),
                            );
                        }
                        Ok(_) => {
                            log::info!("WAL replay: no entries to restore (fresh node)");
                        }
                        Err(e) => {
                            log::warn!("WAL replay failed: {} — starting with empty log", e);
                        }
                    }
                }

                // Start WAL background flush task (group commit)
                if let Some(ref wal) = self.wal {
                    let _flush_handle = wal.spawn_flush_task();
                    log::info!("WAL group commit enabled (flush interval: 5ms)");
                }

                // Log snapshot status
                if self.snapshot_manager.is_some() {
                    log::info!("Snapshot manager enabled for resync");
                }

                // ── BACKGROUND REPLICATION TASK ────────────────────────────
                //
                // This long-running task handles three responsibilities:
                //
                // 1. CATCH-UP: Periodically checks each follower's last_replicated_index
                //    and sends only the missing entries. This is how lagging followers
                //    eventually converge with the leader — the write path only sends
                //    the current entry, this task fills in any gaps.
                //
                // 2. HEARTBEAT: Every ~1.7s (election_timeout/3), sends an empty
                //    AppendEntriesRequest to all followers. This prevents followers
                //    from starting unnecessary elections during idle periods.
                //
                // 3. RESYNC: When a follower is detected as desynced (>1000 entries
                //    behind, or unresponsive for >30s with pending work), triggers
                //    a full snapshot transfer instead of incremental catch-up.
                //
                // Runs on a 100ms tick interval.
                if let Some(ref batcher) = self.replication_batcher {
                    let batcher_clone = Arc::clone(batcher);
                    let peers = self.cluster_peers.clone();
                    let consensus_log = self.consensus_log.clone();
                    let node_id = self.node_id.unwrap_or(0);
                    let self_port = self.config.server.port;
                    let raft_state = self.raft_state.clone();
                    let snapshot_manager = self.snapshot_manager.clone();
                    let models = Arc::clone(&self.models);
                    let replication_config = self.config.replication.clone();
                    let resync_stats = Arc::clone(&self.resync_stats);
                    let wal_for_resync = self.wal.clone();

                    tokio::spawn(async move {
                        use std::collections::HashMap;
                        use std::time::Duration;
                        use tokio::time::interval;

                        let mut ticker = interval(Duration::from_millis(100)); // Check every 100ms
                        let mut _catchup_counter = 0u64; // Reserved for future use
                        let mut resync_counter = 0u64; // For periodic snapshot resync
                        let mut heartbeat_counter = 0u64; // For leader heartbeat

                        // Heartbeat interval: election_timeout / 3, derived from config
                        let heartbeat_ticks = raft_state
                            .as_ref()
                            .map(|s| (s.election_timeout.as_millis() as u64 / 3 + 50) / 100)
                            .unwrap_or(17);

                        // Shared HTTP client for background tasks (reuses connection pool)
                        let bg_client = reqwest::Client::builder()
                            .timeout(Duration::from_secs(5))
                            .build()
                            .unwrap_or_else(|_| reqwest::Client::new());

                        // Track last resync time per follower for cooldown
                        let mut last_resync: HashMap<String, std::time::Instant> = HashMap::new();

                        // Calculate resync check ticks from config (100ms base interval)
                        let resync_check_ticks = replication_config.resync_check_interval_ms / 100;

                        loop {
                            ticker.tick().await;

                            // Only leader should do background replication
                            if let Some(ref state) = raft_state {
                                if !state.is_leader() {
                                    continue;
                                }
                            }

                            let consensus_log_ref = match &consensus_log {
                                Some(log) => log,
                                None => continue,
                            };

                            let term = consensus_log_ref.current_term();
                            let commit_index = consensus_log_ref.commit_index();

                            // Increment counters
                            _catchup_counter += 1;
                            resync_counter += 1;

                            // === SNAPSHOT-BASED RESYNC FOR DESYNCED FOLLOWERS ===
                            // Check based on configurable interval (default: 1 second = 10 ticks)
                            if resync_counter >= resync_check_ticks {
                                resync_counter = 0;

                                // Get list of desynced followers
                                let desynced =
                                    batcher_clone.get_desynced_followers(commit_index).await;

                                if !desynced.is_empty() {
                                    if let Some(ref snap_mgr_inner) = snapshot_manager {
                                        // Filter out followers that are in cooldown
                                        let cooldown_duration = Duration::from_secs(
                                            replication_config.resync_cooldown_secs,
                                        );
                                        let now = std::time::Instant::now();
                                        let eligible_for_resync: Vec<_> = desynced
                                            .into_iter()
                                            .filter(|peer| {
                                                match last_resync.get(peer) {
                                                    Some(last_time) => {
                                                        now.duration_since(*last_time)
                                                            >= cooldown_duration
                                                    }
                                                    None => true, // Never resynced, eligible
                                                }
                                            })
                                            .collect();

                                        if !eligible_for_resync.is_empty() {
                                            log::info!(
                                                "Found {} desynced followers eligible for resync",
                                                eligible_for_resync.len()
                                            );

                                            let snapshot_mgr = snap_mgr_inner.clone();
                                            let models_clone = Arc::clone(&models);
                                            let batcher_for_resync = Arc::clone(&batcher_clone);

                                            // Create snapshot if needed (only once per resync cycle)
                                            if let Err(e) = Self::create_snapshot_from_models(
                                                &models_clone,
                                                &snapshot_mgr,
                                                term,
                                                commit_index,
                                            )
                                            .await
                                            {
                                                log::warn!(
                                                    "Failed to create snapshot for resync: {}",
                                                    e
                                                );
                                            } else {
                                                // Track snapshot creation
                                                resync_stats.record_snapshot_created();

                                                // ── WAL COMPACTION ─────────────────────────
                                                // Now that the snapshot captures state up to
                                                // commit_index, we can safely remove older WAL
                                                // entries. This prevents the WAL from growing
                                                // unbounded over time.
                                                if let Some(ref wal_for_compact) = wal_for_resync {
                                                    match wal_for_compact.compact(commit_index).await {
                                                        Ok(0) => {}
                                                        Ok(n) => log::info!(
                                                            "WAL compacted: removed {} entries (snapshot covers up to index {})",
                                                            n, commit_index
                                                        ),
                                                        Err(e) => log::warn!("WAL compaction failed: {}", e),
                                                    }
                                                }

                                                // Send snapshot to each desynced follower (in parallel, with configurable rate limit)
                                                let max_concurrent =
                                                    replication_config.max_concurrent_resyncs;
                                                let snapshot_timeout_secs =
                                                    replication_config.snapshot_send_timeout_secs;

                                                for peer in eligible_for_resync
                                                    .into_iter()
                                                    .take(max_concurrent)
                                                {
                                                    // Mark as resyncing with current timestamp
                                                    last_resync.insert(peer.clone(), now);

                                                    let peer_clone = peer.clone();
                                                    let snapshot_mgr_clone = snapshot_mgr.clone();
                                                    let batcher_resync =
                                                        Arc::clone(&batcher_for_resync);
                                                    let stats_clone = Arc::clone(&resync_stats);

                                                    // Track send attempt
                                                    resync_stats.record_send_attempt(commit_index);

                                                    tokio::spawn(async move {
                                                        log::info!(
                                                    "Sending snapshot to desynced follower: {}",
                                                    peer_clone
                                                );

                                                        match Self::send_snapshot_to_follower_with_timeout(
                                                    &peer_clone,
                                                    &snapshot_mgr_clone,
                                                    snapshot_timeout_secs,
                                                )
                                                .await
                                                {
                                                    Ok(()) => {
                                                        log::info!(
                                                            "Snapshot installed on {}",
                                                            peer_clone
                                                        );
                                                        // Track success
                                                        stats_clone.record_send_success();
                                                        // Reset follower health after successful resync
                                                        if let Some(follower) = batcher_resync
                                                            .get_follower(&peer_clone)
                                                            .await
                                                        {
                                                            follower.record_success(0, 0).await;
                                                            // Reset to healthy
                                                        }
                                                    }
                                                    Err(e) => {
                                                        log::error!(
                                                            "Snapshot send to {} failed: {}",
                                                            peer_clone,
                                                            e
                                                        );
                                                        // Track failure
                                                        stats_clone.record_send_failure();
                                                    }
                                                }
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            // === INCREMENTAL CATCH-UP FOR LAGGING FOLLOWERS ===
                            // Only send entries that followers are actually missing
                            if commit_index > 0 {
                                // Get health status to skip desynced followers
                                let health_summary = batcher_clone.get_health_summary().await;

                                for peer in &peers {
                                    // Skip desynced followers - they'll get snapshots instead
                                    if let Some(health) = health_summary.get(peer) {
                                        if *health == crate::cluster::replication_batcher::FollowerHealth::Desynced {
                                        log::debug!("Skipping desynced follower {} (will use snapshot)", peer);
                                        continue;
                                    }
                                    }

                                    // Get follower's last replicated index
                                    let follower_index = if let Some(follower) =
                                        batcher_clone.get_follower(peer).await
                                    {
                                        follower
                                            .last_replicated_index
                                            .load(std::sync::atomic::Ordering::SeqCst)
                                    } else {
                                        0
                                    };

                                    // Skip if follower is already in sync
                                    if follower_index >= commit_index {
                                        continue;
                                    }

                                    // Only get entries the follower is missing
                                    let missing_entries = consensus_log_ref
                                        .get_entries_from(follower_index + 1)
                                        .await;
                                    if missing_entries.is_empty() {
                                        continue;
                                    }

                                    let peer = peer.clone();
                                    let entries = missing_entries;
                                    let batcher = Arc::clone(&batcher_clone);
                                    let commit = commit_index;

                                    tokio::spawn(async move {
                                        let client = reqwest::Client::builder()
                                            .timeout(Duration::from_secs(5))
                                            .build()
                                            .unwrap_or_else(|_| reqwest::Client::new());

                                        let request =
                                            crate::cluster::consensus_log::AppendEntriesRequest {
                                                term,
                                                leader_id: node_id,
                                                leader_port: self_port,
                                                prev_log_index: 0,
                                                prev_log_term: 0,
                                                entries: entries.clone(),
                                                leader_commit: commit,
                                            };

                                        let start = std::time::Instant::now();
                                        let url = format!("http://{}/_raft/append", peer);

                                        match client.post(&url).json(&request).send().await {
                                            Ok(resp) if resp.status().is_success() => {
                                                let latency = start.elapsed().as_millis() as u64;
                                                let last_index = entries
                                                    .last()
                                                    .map(|e| e.log_id.index)
                                                    .unwrap_or(0);
                                                batcher
                                                    .record_success(&peer, last_index, latency)
                                                    .await;
                                                log::debug!(
                                                    "Background catch-up: {} entries to {} ({}ms)",
                                                    entries.len(),
                                                    peer,
                                                    latency
                                                );
                                            }
                                            Ok(resp) => {
                                                log::debug!(
                                                    "Background catch-up to {} failed: {}",
                                                    peer,
                                                    resp.status()
                                                );
                                                batcher.record_failure(&peer).await;
                                            }
                                            Err(e) => {
                                                log::debug!(
                                                    "Background catch-up to {} error: {}",
                                                    peer,
                                                    e
                                                );
                                                batcher.record_failure(&peer).await;
                                            }
                                        }
                                    });
                                }
                            }

                            // === LEADER HEARTBEAT ===
                            // Send empty AppendEntriesRequest to all followers periodically
                            // to prevent them from starting unnecessary elections.
                            // Includes the leader's last log index/term so followers can
                            // detect log divergence (Raft safety property).
                            heartbeat_counter += 1;
                            if heartbeat_counter >= heartbeat_ticks {
                                heartbeat_counter = 0;

                                let last_log_index = consensus_log_ref.last_index().await;
                                let last_log_term = consensus_log_ref
                                    .last_entry()
                                    .await
                                    .map_or(0, |e| e.log_id.term);

                                for peer in &peers {
                                    let peer = peer.clone();
                                    let client = bg_client.clone();
                                    let heartbeat_request =
                                        crate::cluster::consensus_log::AppendEntriesRequest {
                                            term,
                                            leader_id: node_id,
                                            leader_port: self_port,
                                            prev_log_index: last_log_index,
                                            prev_log_term: last_log_term,
                                            entries: vec![], // Empty = heartbeat
                                            leader_commit: commit_index,
                                        };

                                    tokio::spawn(async move {
                                        let url = format!("http://{}/_raft/append", peer);
                                        let _ =
                                            client.post(&url).json(&heartbeat_request).send().await;
                                    });
                                }
                            }

                            // Normal batch processing for new entries
                            if !batcher_clone.should_send_batch().await {
                                continue;
                            }

                            let batch = batcher_clone.take_batch().await;
                            if batch.is_empty() {
                                continue;
                            }

                            // Send batch to all peers
                            for peer in &peers {
                                let peer = peer.clone();
                                let entries = batch.clone();
                                let batcher = Arc::clone(&batcher_clone);
                                let max_entry_index =
                                    entries.iter().map(|e| e.log_id.index).max().unwrap_or(0);

                                tokio::spawn(async move {
                                    let client = reqwest::Client::builder()
                                        .timeout(Duration::from_secs(5))
                                        .build()
                                        .unwrap_or_else(|_| reqwest::Client::new());

                                    let request =
                                        crate::cluster::consensus_log::AppendEntriesRequest {
                                            term,
                                            leader_id: node_id,
                                            leader_port: self_port,
                                            prev_log_index: 0,
                                            prev_log_term: 0,
                                            entries: entries.clone(),
                                            leader_commit: max_entry_index,
                                        };

                                    let start = std::time::Instant::now();
                                    let url = format!("http://{}/_raft/append", peer);

                                    match client.post(&url).json(&request).send().await {
                                        Ok(resp) if resp.status().is_success() => {
                                            let latency = start.elapsed().as_millis() as u64;
                                            let last_index =
                                                entries.last().map(|e| e.log_id.index).unwrap_or(0);
                                            batcher
                                                .record_success(&peer, last_index, latency)
                                                .await;
                                            log::debug!(
                                                "Background replicated {} entries to {} ({}ms)",
                                                entries.len(),
                                                peer,
                                                latency
                                            );
                                        }
                                        Ok(resp) => {
                                            log::warn!(
                                                "Background replication to {} failed: {}",
                                                peer,
                                                resp.status()
                                            );
                                            batcher.record_failure(&peer).await;
                                        }
                                        Err(e) => {
                                            log::warn!(
                                                "Background replication to {} error: {}",
                                                peer,
                                                e
                                            );
                                            batcher.record_failure(&peer).await;
                                        }
                                    }
                                });
                            }
                        }
                    });
                    log::info!("Background replication task started");
                }
            }
        }

        // Build server address
        let addr = format!("{}:{}", self.config.server.host, self.config.server.port);

        // Build optional TLS acceptor
        #[cfg(feature = "tls")]
        let tls_acceptor =
            match (&self.config.server.tls_cert_path, &self.config.server.tls_key_path) {
                (Some(cert_path), Some(key_path)) => {
                    let certs = load_tls_certs(cert_path)?;
                    let key = load_tls_key(key_path)?;

                    // Log certificate fingerprint for verification
                    if let Some(leaf_cert) = certs.first() {
                        let hash = sha2::Sha256::digest(leaf_cert.as_ref());
                        log::info!("TLS certificate SHA-256: {:x}", hash);
                    }

                    let tls_config = rustls::ServerConfig::builder()
                        .with_no_client_auth()
                        .with_single_cert(certs, key)
                        .context("Invalid TLS certificate/key pair")?;
                    Some(tokio_rustls::TlsAcceptor::from(Arc::new(tls_config)))
                }
                _ => None,
            };
        #[cfg(not(feature = "tls"))]
        let tls_acceptor: Option<()> = {
            if self.config.server.tls_cert_path.is_some()
                || self.config.server.tls_key_path.is_some()
            {
                log::warn!("TLS certificate/key configured but the 'tls' feature is not enabled");
                log::warn!("   Add `features = [\"tls\"]` to your lithair-core dependency");
            }
            None
        };
        let tls_active = tls_acceptor.is_some();

        // Start server
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .with_context(|| format!("Failed to bind to {}", addr))?;

        let scheme = if tls_active { "https" } else { "http" };
        log::info!("Server listening on {}://{}", scheme, addr);

        // Start Raft background tasks if cluster mode enabled
        if let Some(ref raft_state) = self.raft_state {
            let state_clone = Arc::clone(raft_state);
            let peers = self.cluster_peers.clone();
            let raft_config = self.config.raft.clone();

            if raft_state.is_leader() {
                // Leader: send heartbeats to followers
                tokio::spawn(async move {
                    use reqwest::Client as HttpClient;
                    use std::time::Duration;
                    use tokio::time::sleep;

                    let client = HttpClient::builder()
                        .timeout(Duration::from_secs(2))
                        .build()
                        .unwrap_or_else(|_| HttpClient::new());

                    let heartbeat_interval =
                        Duration::from_secs(raft_config.heartbeat_interval_secs);

                    loop {
                        sleep(heartbeat_interval).await;

                        if !state_clone.is_leader() {
                            log::info!("No longer leader, stopping heartbeat sender");
                            break;
                        }

                        let heartbeat_msg = serde_json::json!({
                            "leader_id": state_clone.node_id,
                            "leader_port": state_clone.self_port,
                            "term": 1
                        });

                        for peer in &peers {
                            let url = format!("http://{}{}/heartbeat", peer, raft_config.path);
                            let mut req = client.post(&url).json(&heartbeat_msg);

                            if let Some(ref token) = raft_config.auth_token {
                                req = req.header("X-Raft-Token", token);
                            }

                            let _ = req.send().await;
                        }
                    }
                });
            } else {
                // Follower: monitor heartbeats and trigger election if timeout
                tokio::spawn(async move {
                    use std::time::Duration;
                    use tokio::time::sleep;

                    loop {
                        sleep(Duration::from_secs(1)).await;

                        if state_clone.should_start_election() {
                            log::info!("⏰ Heartbeat timeout detected! Starting election...");

                            let (should_become_leader, new_leader_id, new_leader_port) =
                                state_clone.start_election().await;

                            if should_become_leader {
                                state_clone.become_leader();
                            } else {
                                state_clone.become_follower(new_leader_id, new_leader_port);
                            }
                        }
                    }
                });
            }
        }

        // Extract config values before moving self into Arc
        let request_timeout = self.config.server.request_timeout;
        let max_body_size = self.config.server.max_body_size;

        // Materialize firewall from config (builder > env)
        let firewall = Arc::new(crate::http::Firewall::new(
            crate::http::firewall::resolve_firewall_config(self.firewall_config.clone(), None),
        ));

        // Materialize anti-DDoS protection if configured
        let anti_ddos: Option<Arc<crate::security::anti_ddos::AntiDDoSProtection>> = self
            .anti_ddos_config
            .as_ref()
            .map(|cfg| Arc::new(crate::security::anti_ddos::AntiDDoSProtection::new(cfg.clone())));

        // Resolve access log: builder flag OR env var
        let access_log = self.access_log
            || std::env::var("LT_HTTP_ACCESS_LOG")
                .ok()
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);

        if access_log {
            crate::http::init_access_log_buffer(self.access_log_capacity);
        }

        // Initialize system metrics collector (CPU, RAM, load, RSS)
        crate::system::init_system_metrics();

        // Share server state
        let server = Arc::new(self);

        // Accept connections
        loop {
            let (stream, remote_addr) = listener.accept().await?;

            // Connection-level anti-DDoS check
            if let Some(ref protection) = anti_ddos {
                if !protection.is_connection_allowed(remote_addr.ip()).await {
                    log::warn!("Anti-DDoS: rejected connection from {}", remote_addr.ip());
                    drop(stream);
                    continue;
                }
            }

            let server = server.clone();
            let firewall = firewall.clone();
            let anti_ddos = anti_ddos.clone();
            #[cfg(feature = "tls")]
            let tls_acceptor = tls_acceptor.clone();

            tokio::spawn(async move {
                // TLS handshake (if configured) or plain TCP
                #[cfg(feature = "tls")]
                let io = {
                    let maybe_tls = if let Some(acceptor) = tls_acceptor {
                        match tokio::time::timeout(
                            std::time::Duration::from_secs(10),
                            acceptor.accept(stream),
                        )
                        .await
                        {
                            Ok(Ok(tls_stream)) => MaybeTlsStream::Tls(Box::new(tls_stream)),
                            Ok(Err(e)) => {
                                log::debug!("TLS handshake failed from {}: {}", remote_addr, e);
                                return;
                            }
                            Err(_) => {
                                log::debug!("TLS handshake timeout from {}", remote_addr);
                                return;
                            }
                        }
                    } else {
                        MaybeTlsStream::Plain(stream)
                    };
                    hyper_util::rt::TokioIo::new(maybe_tls)
                };
                #[cfg(not(feature = "tls"))]
                let io = hyper_util::rt::TokioIo::new(stream);

                let service = hyper::service::service_fn(
                    move |req: hyper::Request<hyper::body::Incoming>| {
                        let server = server.clone();
                        let firewall = firewall.clone();
                        let anti_ddos = anti_ddos.clone();
                        async move {
                            let start = std::time::Instant::now();
                            let req_method = req.method().to_string();
                            let req_path = req.uri().path().to_string();
                            // Resolve real client IP (trusts proxy headers only from loopback/private)
                            let client_ip = crate::http::resolve_client_ip(&req, remote_addr);

                            let result = (async move {
                                // Firewall check (preserves 403 vs 429 distinction)
                                if let Err(denied) = firewall.check(
                                    Some(remote_addr),
                                    req.method(),
                                    req.uri().path(),
                                ) {
                                    let (parts, boxed_body) = denied.into_parts();
                                    let body_bytes = http_body_util::BodyExt::collect(boxed_body)
                                        .await
                                        .map(|c| c.to_bytes())
                                        .unwrap_or_else(|_| {
                                            bytes::Bytes::from(r#"{"error":"Forbidden"}"#)
                                        });
                                    return Ok::<_, std::convert::Infallible>(
                                        Self::add_security_headers(
                                            hyper::Response::builder()
                                                .status(parts.status)
                                                .header("Content-Type", "application/json")
                                                .body(http_body_util::Full::new(body_bytes))
                                                .expect("valid HTTP response"),
                                            tls_active,
                                        ),
                                    );
                                }

                                // Anti-DDoS request rate check
                                if let Some(ref protection) = anti_ddos {
                                    if !protection.is_request_allowed(remote_addr.ip()).await {
                                        return Ok(Self::add_security_headers(
                                            hyper::Response::builder()
                                                .status(429)
                                                .header("Content-Type", "application/json")
                                                .header("Retry-After", "60")
                                                .body(http_body_util::Full::new(
                                                    bytes::Bytes::from(
                                                        r#"{"error":"Rate limit exceeded"}"#,
                                                    ),
                                                ))
                                                .expect("valid HTTP response"),
                                            tls_active,
                                        ));
                                    }
                                }

                                // Body size enforcement via Content-Length
                                if let Some(cl) = req.headers().get(hyper::header::CONTENT_LENGTH) {
                                    if let Ok(len) = cl.to_str().unwrap_or("0").parse::<usize>() {
                                        if len > max_body_size {
                                            return Ok(Self::add_security_headers(
                                                hyper::Response::builder()
                                                    .status(413)
                                                    .header("Content-Type", "application/json")
                                                    .body(http_body_util::Full::new(
                                                        bytes::Bytes::from(
                                                            r#"{"error":"Request body too large"}"#,
                                                        ),
                                                    ))
                                                    .expect("valid HTTP response"),
                                                tls_active,
                                            ));
                                        }
                                    }
                                }

                                match server.handle_request(req).await {
                                    Ok(resp) => Ok::<_, std::convert::Infallible>(
                                        Self::add_security_headers(resp, tls_active),
                                    ),
                                    Err(e) => {
                                        log::error!("Request handler error: {}", e);
                                        Ok(Self::add_security_headers(
                                            hyper::Response::builder()
                                                .status(500)
                                                .header("Content-Type", "application/json")
                                                .body(http_body_util::Full::new(
                                                    bytes::Bytes::from(
                                                        r#"{"error":"Internal server error"}"#,
                                                    ),
                                                ))
                                                .expect("valid HTTP response"),
                                            tls_active,
                                        ))
                                    }
                                }
                            })
                            .await;

                            if access_log {
                                let Ok(ref resp) = result;
                                crate::http::log_access_ip(
                                    &client_ip,
                                    &req_method,
                                    &req_path,
                                    resp,
                                    start,
                                );
                            }

                            result
                        }
                    },
                );

                if let Err(err) = hyper::server::conn::http1::Builder::new()
                    .timer(hyper_util::rt::TokioTimer::new())
                    .header_read_timeout(std::time::Duration::from_secs(request_timeout))
                    .keep_alive(true)
                    .serve_connection(io, service)
                    .await
                {
                    log::error!("Connection error from {}: {}", remote_addr, err);
                }
            });
        }
    }

    /// Add security headers to a response.
    /// Uses `entry().or_insert()` so handlers that explicitly set a header are not overridden.
    /// When `tls_active` is true, adds HSTS header.
    fn add_security_headers(
        resp: hyper::Response<http_body_util::Full<bytes::Bytes>>,
        tls_active: bool,
    ) -> hyper::Response<http_body_util::Full<bytes::Bytes>> {
        let (mut parts, body) = resp.into_parts();
        let h = &mut parts.headers;
        h.entry("x-content-type-options")
            .or_insert("nosniff".parse().expect("valid header value"));
        h.entry("x-frame-options")
            .or_insert("DENY".parse().expect("valid header value"));
        h.entry("referrer-policy")
            .or_insert("strict-origin-when-cross-origin".parse().expect("valid header value"));
        h.entry("x-xss-protection")
            .or_insert("1; mode=block".parse().expect("valid header value"));
        if tls_active {
            h.entry("strict-transport-security").or_insert(
                "max-age=31536000; includeSubDomains".parse().expect("valid header value"),
            );
        }
        hyper::Response::from_parts(parts, body)
    }

    /// Build the canonical "leader port unknown" 503 response used by every
    /// follower-side write redirect path.
    ///
    /// A follower learns the leader's port from the first AppendEntries it
    /// receives. Until then `raft_state.get_leader_port()` returns 0, and
    /// blindly redirecting to `http://127.0.0.1:0` would point clients at the
    /// kernel's "ephemeral port" sentinel. Callers must short-circuit to this
    /// 503 when `leader_port == 0`. The body and headers are kept identical
    /// across call sites so clients can recognize and back off uniformly.
    ///
    /// Note: only the 503 fallback is centralized here. The 307 redirect
    /// branches in `handle_request`, `handle_model_request`, and
    /// `handle_migrate_operation` differ in body shape, headers
    /// (`X-Raft-Leader`), and the path/query forwarded in `Location`, so
    /// folding them would change wire behavior. They remain inlined.
    fn leader_port_unknown_503() -> hyper::Response<http_body_util::Full<bytes::Bytes>> {
        use http_body_util::Full;
        hyper::Response::builder()
            .status(hyper::StatusCode::SERVICE_UNAVAILABLE)
            .header("Content-Type", "application/json")
            .header("Retry-After", "1")
            .body(Full::new(Bytes::from(
                r#"{"error":"Leader port not yet discovered, retry after heartbeat"}"#,
            )))
            .expect("valid HTTP response")
    }

    /// Match a path against a pattern with wildcard support
    ///
    /// Supports:
    /// - Exact match: `/api/products`
    /// - Single segment wildcard: `/api/*` matches `/api/products` but not `/api/products/123`
    /// - Multi-segment wildcard: `/api/**` matches `/api/products`, `/api/products/123`, etc.
    /// - Suffix wildcard: `/static/*` matches any path starting with `/static/`
    /// - Middle wildcard: `/api/consumers/*/orders` matches `/api/consumers/{id}/orders`
    fn path_matches(pattern: &str, path: &str) -> bool {
        // Exact match
        if pattern == path {
            return true;
        }

        // Wildcard matching
        if pattern.contains('*') {
            // Handle `**` (multi-segment wildcard) - matches everything after
            if let Some(prefix) = pattern.strip_suffix("/**") {
                return path.starts_with(prefix);
            }

            // Handle `/*` (any single path after prefix) - but only if it's at the end
            if pattern.ends_with("/*") && !pattern.contains("/*/") {
                let prefix = &pattern[..pattern.len() - 2];
                return path.starts_with(prefix);
            }

            // Handle exact wildcard `/` + `*`
            if pattern == "/*" {
                return true; // Matches any path
            }

            // Handle middle wildcard: `/api/consumers/*/orders`
            // Split both pattern and path by '/' and match segment by segment
            let pattern_segments: Vec<&str> = pattern.split('/').collect();
            let path_segments: Vec<&str> = path.split('/').collect();

            // Must have same number of segments for exact middle wildcard matching
            if pattern_segments.len() != path_segments.len() {
                return false;
            }

            // Match segment by segment
            for (p_seg, path_seg) in pattern_segments.iter().zip(path_segments.iter()) {
                if *p_seg == "*" {
                    // Wildcard matches any single segment
                    continue;
                }
                if p_seg != path_seg {
                    return false;
                }
            }
            return true;
        }

        false
    }

    /// Handle incoming HTTP request
    async fn handle_request(
        &self,
        req: hyper::Request<hyper::body::Incoming>,
    ) -> Result<hyper::Response<http_body_util::Full<bytes::Bytes>>> {
        use bytes::Bytes;
        use http_body_util::Full;

        let method = req.method().clone();
        let path = req.uri().path().to_string();

        // Resolve the request host once and reuse it for both the host
        // redirect (below) and the vhost lookup (further down). Both
        // call paths previously called `host_from_request` independently;
        // hoisting it avoids duplicate header parsing and keeps the two
        // dispatch decisions consistent on a single source of truth.
        // `host_from_request` returns `None` only when neither URI
        // authority nor `Host:` header is present — we treat that as
        // "" (matches no entry).
        let req_host = crate::http::host_from_request(&req).unwrap_or("").to_string();

        // Host-to-host 301 redirect (canonical URL hygiene).
        //
        // Declared via `LithairServerBuilder::with_redirect`. Runs before
        // any other dispatch logic so a redirected host never reaches the
        // vhost router, frontends, or custom routes — the only thing the
        // client gets back is a 301 with `Location:` pointing at the
        // canonical host, preserving path + query string. Applies to ALL
        // HTTP methods (clients may follow 301 on any verb).
        //
        // Zero-cost when no redirects are configured.
        if self.host_redirects.has_entries() {
            if let Some(target_host) = self.host_redirects.lookup(&req_host) {
                let path_and_query =
                    req.uri().path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
                let location = format!("https://{}{}", target_host, path_and_query);

                // Build the `Location:` header value defensively.
                // `target_host` was normalized at config time and
                // `path_and_query` comes from a parsed `hyper::Uri`, so
                // the bytes should already be header-safe. We still go
                // through `HeaderValue::try_from` rather than panicking:
                // an unexpected failure here (e.g. a future change to the
                // URI parser) should produce a clean 500 to the client,
                // not crash the worker.
                match hyper::header::HeaderValue::try_from(location.as_str()) {
                    Ok(loc_hv) => {
                        log::debug!("301 host redirect: {} -> {}", req_host, location);
                        return Ok(hyper::Response::builder()
                            .status(hyper::StatusCode::MOVED_PERMANENTLY)
                            .header(hyper::header::LOCATION, loc_hv)
                            .header("Content-Type", "text/plain; charset=utf-8")
                            .body(Full::new(Bytes::from_static(b"Moved Permanently")))
                            .expect("static body + valid status never fails"));
                    }
                    Err(e) => {
                        log::error!(
                            "host redirect: refusing to emit invalid Location='{}' ({})",
                            location,
                            e
                        );
                        return Ok(hyper::Response::builder()
                            .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                            .header("Content-Type", "text/plain; charset=utf-8")
                            .body(Full::new(Bytes::from_static(b"Internal Server Error")))
                            .expect("static body + valid status never fails"));
                    }
                }
            }
        }

        // Resolve the vhost-scoped frontend engine map (if any) before we
        // consume `req` later in the pipeline. If no vhosts are
        // declared, `vhost_engines` is `None` and the server behaves as
        // before (host-agnostic path routing).
        let vhost_engines: Option<
            &std::collections::HashMap<String, Arc<crate::frontend::FrontendEngine>>,
        > = if self.vhost_frontend_router.has_entries() {
            self.vhost_frontend_router.lookup(&req_host)
        } else {
            None
        };

        log::debug!("{} {}", method, path);

        // Raft Cluster: Check for write redirection and Raft endpoints
        if let Some(ref raft_state) = self.raft_state {
            let heartbeat_path = self.config.raft.heartbeat_path();
            let leader_path = self.config.raft.leader_path();

            // Raft heartbeat endpoint
            if path == heartbeat_path && method == hyper::Method::POST {
                let provided_token =
                    req.headers().get("X-Raft-Token").and_then(|v| v.to_str().ok());

                if !self.config.raft.validate_token(provided_token) {
                    return Ok(hyper::Response::builder()
                        .status(hyper::StatusCode::UNAUTHORIZED)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(r#"{"error":"Invalid Raft token"}"#)))
                        .expect("valid HTTP response"));
                }

                // Update heartbeat timestamp
                raft_state.update_heartbeat();

                // Parse heartbeat to update leader info if needed
                use http_body_util::BodyExt;
                let body_bytes =
                    req.into_body().collect().await.map(|c| c.to_bytes()).unwrap_or_default();
                if let Ok(heartbeat) = serde_json::from_slice::<serde_json::Value>(&body_bytes) {
                    let leader_id =
                        heartbeat.get("leader_id").and_then(|v| v.as_u64()).unwrap_or(0);
                    let leader_port =
                        heartbeat.get("leader_port").and_then(|v| v.as_u64()).unwrap_or(0) as u16;

                    if !raft_state.is_leader()
                        && leader_id
                            != raft_state
                                .current_leader_id
                                .load(std::sync::atomic::Ordering::Relaxed)
                    {
                        log::info!(
                            "Heartbeat: updating leader to node {} (port {})",
                            leader_id,
                            leader_port
                        );
                        raft_state.become_follower(leader_id, leader_port);
                    }
                }

                return Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::OK)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(r#"{"status":"ok"}"#)))
                    .expect("valid HTTP response"));
            }

            // Raft leader discovery endpoint
            if path == leader_path && method == hyper::Method::GET {
                let provided_token =
                    req.headers().get("X-Raft-Token").and_then(|v| v.to_str().ok());

                if !self.config.raft.validate_token(provided_token) {
                    return Ok(hyper::Response::builder()
                        .status(hyper::StatusCode::UNAUTHORIZED)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(r#"{"error":"Invalid Raft token"}"#)))
                        .expect("valid HTTP response"));
                }

                let response = serde_json::json!({
                    "leader_id": raft_state.current_leader_id.load(std::sync::atomic::Ordering::Relaxed),
                    "leader_port": raft_state.get_leader_port(),
                    "is_current_node_leader": raft_state.is_leader(),
                    "node_id": raft_state.node_id
                });

                return Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::OK)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(response.to_string())))
                    .expect("valid HTTP response"));
            }

            // Redirect writes to leader if we're a follower
            // Exception: /internal/* and /_raft/* endpoints are internal cluster communication
            let is_write =
                matches!(method, hyper::Method::POST | hyper::Method::PUT | hyper::Method::DELETE);
            let is_internal = path.starts_with("/internal/") || path.starts_with("/_raft/");

            if is_write && !raft_state.is_leader() && !is_internal {
                let leader_port = raft_state.get_leader_port();
                if leader_port == 0 {
                    return Ok(Self::leader_port_unknown_503());
                }

                let redirect_url = format!(
                    "http://127.0.0.1:{}{}",
                    leader_port,
                    req.uri().path_and_query().map(|pq| pq.as_str()).unwrap_or(&path)
                );

                log::debug!("Redirecting write to leader on port {}", leader_port);

                return Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::TEMPORARY_REDIRECT)
                    .header(hyper::header::LOCATION, redirect_url.clone())
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(format!(
                        r#"{{"message":"Redirected to leader","leader_url":"{}"}}"#,
                        redirect_url
                    ))))
                    .expect("valid HTTP response"));
            }
        }

        // Internal replication endpoints (for followers to receive data from leader)
        if path == "/internal/replicate" && method == hyper::Method::POST {
            return self.handle_internal_replicate(req).await;
        }

        if path == "/internal/replicate_bulk" && method == hyper::Method::POST {
            return self.handle_internal_replicate_bulk(req).await;
        }

        if path == "/internal/replicate_update" && method == hyper::Method::POST {
            return self.handle_internal_replicate_update(req).await;
        }

        if path == "/internal/replicate_delete" && method == hyper::Method::POST {
            return self.handle_internal_replicate_delete(req).await;
        }

        // Raft consensus log append entries endpoint (for followers to receive log entries from leader)
        if path == "/_raft/append" && method == hyper::Method::POST {
            return self.handle_raft_append_entries(req).await;
        }

        // Snapshot endpoints for resync of desynced followers
        if path == "/_raft/snapshot" && method == hyper::Method::GET {
            return self.handle_get_snapshot(req).await;
        }
        if path == "/_raft/snapshot" && method == hyper::Method::POST {
            return self.handle_install_snapshot(req).await;
        }

        // Cluster health endpoint (follower status)
        if path == "/_raft/health" && method == hyper::Method::GET {
            return self.handle_cluster_health().await;
        }

        // Resync stats endpoint (snapshot resync observability)
        if path == "/_raft/resync_stats" && method == hyper::Method::GET {
            return self.handle_resync_stats().await;
        }

        // Migration operation endpoint (for rolling upgrades)
        if path == "/_raft/migrate" && method == hyper::Method::POST {
            return self.handle_migrate_operation(req).await;
        }

        // Sync status endpoint (detailed follower sync state for ops)
        if path == "/_raft/sync-status" && method == hyper::Method::GET {
            return self.handle_sync_status().await;
        }

        // Force resync endpoint (manually trigger snapshot resync)
        if path.starts_with("/_raft/force-resync") && method == hyper::Method::POST {
            return self.handle_force_resync(req).await;
        }

        // Schema sync endpoints (cluster-internal)
        if path == "/_raft/schema/propose" && method == hyper::Method::POST {
            return self.handle_schema_propose(req).await;
        }
        if path == "/_raft/schema/vote" && method == hyper::Method::POST {
            return self.handle_schema_vote(req).await;
        }
        if path == "/_raft/schema/current" && method == hyper::Method::GET {
            return self.handle_schema_current(req).await;
        }

        // Schema admin endpoints (external management)
        if path == "/_admin/schema" && method == hyper::Method::GET {
            return self.handle_admin_schema_list().await;
        }
        if path == "/_admin/schema/pending" && method == hyper::Method::GET {
            return self.handle_admin_schema_pending().await;
        }
        if path.starts_with("/_admin/schema/approve/") && method == hyper::Method::POST {
            return self.handle_admin_schema_approve(req, &path).await;
        }
        if path.starts_with("/_admin/schema/reject/") && method == hyper::Method::POST {
            return self.handle_admin_schema_reject(req, &path).await;
        }
        // Phase 3: Schema management operations
        if path == "/_admin/schema/sync" && method == hyper::Method::POST {
            return self.handle_admin_schema_sync().await;
        }
        if path == "/_admin/schema/diff" && method == hyper::Method::GET {
            return self.handle_admin_schema_diff().await;
        }
        if path == "/_admin/schema/history" && method == hyper::Method::GET {
            return self.handle_admin_schema_history().await;
        }
        if path == "/_admin/schema/revalidate" && method == hyper::Method::POST {
            return self.handle_admin_schema_revalidate().await;
        }
        if path.starts_with("/_admin/schema/rollback/") && method == hyper::Method::POST {
            return self.handle_admin_schema_rollback(req, &path).await;
        }
        // Schema lock/unlock endpoints
        if path == "/_admin/schema/lock/status" && method == hyper::Method::GET {
            return self.handle_admin_schema_lock_status().await;
        }
        if path == "/_admin/schema/lock" && method == hyper::Method::POST {
            return self.handle_admin_schema_lock(req).await;
        }
        if path == "/_admin/schema/unlock" && method == hyper::Method::POST {
            return self.handle_admin_schema_unlock(req).await;
        }

        // Route Guards - Declarative protection (authentication, authorization, etc.)
        for guard_matcher in &self.route_guards {
            if guard_matcher.matches(&req) {
                log::debug!("Evaluating guard for pattern: {}", guard_matcher.pattern);
                match guard_matcher.guard.check(&req, self.session_manager.clone()).await {
                    Ok(crate::http::GuardResult::Allow) => {
                        log::debug!("Guard allowed request");
                        // Continue to next guard or routing
                    }
                    Ok(crate::http::GuardResult::Deny(response)) => {
                        log::debug!("Guard denied request");
                        return Ok(response);
                    }
                    Err(e) => {
                        log::error!("Guard check failed: {}", e);
                        return Ok(hyper::Response::builder()
                            .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                            .header("Content-Type", "application/json")
                            .body(Full::new(Bytes::from(r#"{"error":"Internal server error"}"#)))
                            .expect("valid HTTP response"));
                    }
                }
            }
        }

        // Status endpoint (for health checks and cluster discovery)
        if path == "/status" && method == hyper::Method::GET {
            let mut status = serde_json::json!({
                "status": "ready",
                "service": "lithair-server",
                "version": "1.0.0"
            });

            // Add Raft cluster info if enabled
            if let Some(ref raft_state) = self.raft_state {
                status["raft"] = serde_json::json!({
                    "enabled": true,
                    "node_id": raft_state.node_id,
                    "is_leader": raft_state.is_leader(),
                    "leader_port": raft_state.get_leader_port(),
                    "peers": self.cluster_peers.len()
                });
            }

            return Ok(hyper::Response::builder()
                .status(hyper::StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(status.to_string())))
                .expect("valid HTTP response"));
        }

        // OpenAPI spec endpoint
        if self.openapi_enabled && path == "/openapi.json" && method == hyper::Method::GET {
            let spec = if let Some(cached) = self.openapi_spec_cache.get() {
                cached
            } else {
                let models = self.models.read().await;
                let model_infos: Vec<crate::http::OpenApiModelInfo> = models
                    .iter()
                    .filter_map(|m| {
                        m.handler.schema_spec().map(|spec| crate::http::OpenApiModelInfo {
                            name: m.name.clone(),
                            base_path: m.base_path.clone(),
                            spec,
                        })
                    })
                    .collect();
                let generated = crate::http::generate_openapi_spec(&model_infos);
                let _ = self.openapi_spec_cache.set(generated);
                self.openapi_spec_cache.get().expect("just set")
            };

            return Ok(hyper::Response::builder()
                .status(hyper::StatusCode::OK)
                .header("Content-Type", "application/json")
                .header("Access-Control-Allow-Origin", "*")
                .body(Full::new(Bytes::from(spec.to_string())))
                .expect("valid HTTP response"));
        }

        // Swagger UI endpoint
        if self.openapi_enabled && path == "/docs" && method == hyper::Method::GET {
            let html = r##"<!DOCTYPE html>
<html><head>
<title>API Documentation</title>
<meta charset="utf-8"/>
<meta name="viewport" content="width=device-width, initial-scale=1"/>
<link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css"/>
</head><body>
<div id="swagger-ui"></div>
<script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
<script>SwaggerUIBundle({url:"/openapi.json",dom_id:"#swagger-ui"})</script>
</body></html>"##;

            return Ok(hyper::Response::builder()
                .status(hyper::StatusCode::OK)
                .header("Content-Type", "text/html; charset=utf-8")
                .body(Full::new(Bytes::from(html)))
                .expect("valid HTTP response"));
        }

        // Metrics endpoint
        if self.config.admin.metrics_enabled && path == self.config.admin.metrics_path {
            return self.handle_metrics_request(req).await;
        }

        // Data Admin API endpoints (/_admin/data/*)
        if self.config.admin.data_admin_enabled && path.starts_with("/_admin/data/") {
            return self.handle_data_admin_request(req, &path, &method).await;
        }

        // Data Admin UI (embedded dashboard, requires admin-ui feature)
        #[cfg(feature = "admin-ui")]
        if let Some(ref ui_path) = self.config.admin.data_admin_ui_path {
            if path == *ui_path || path == format!("{}/", ui_path) {
                return self.handle_data_admin_ui_request().await;
            }
        }

        // Admin panel endpoint
        if self.config.admin.enabled && path == self.config.admin.path {
            return self.handle_admin_request(req).await;
        }

        // Custom routes checked first — user overrides take priority over model prefix matches
        for route in &self.custom_routes {
            if route.method == method && Self::path_matches(&route.path, &path) {
                return (route.handler)(req).await;
            }
        }

        // Built-in operations endpoints (`/health`, `/ready`, `/info`).
        //
        // These match the README's "Every Lithair server comes with
        // /health, /ready, and /info out of the box" promise (see issue
        // #40). They live AFTER the `custom_routes` loop above so a
        // user calling `.with_route(GET, "/health", ...)` always wins
        // over the default — the override remains a one-line opt-out.
        // They live BEFORE model dispatch so a model whose base_path
        // happens to be `/health` (unlikely but legal) cannot shadow
        // them.
        //
        // The dispatch is a method-and-path equality check so we never
        // accidentally swallow `POST /health` or `/health/sub`.
        if method == hyper::Method::GET {
            match path.as_str() {
                ops_endpoints::HEALTH_PATH => return Ok(ops_endpoints::serve_health()),
                ops_endpoints::READY_PATH => return Ok(ops_endpoints::serve_ready()),
                ops_endpoints::INFO_PATH => {
                    let models = self.models.read().await;
                    let base_paths: Vec<String> =
                        models.iter().map(|m| m.base_path.clone()).collect();
                    drop(models);
                    return Ok(ops_endpoints::serve_info(&base_paths));
                }
                _ => {}
            }
        }

        // Model routes (DeclarativeModel CRUD endpoints)
        let models = self.models.read().await;
        for model in models.iter() {
            if path.starts_with(&model.base_path) {
                return self.handle_model_request(req, model).await;
            }
        }
        drop(models);

        // Frontend assets (memory-first serving with SCC2)
        // Checked AFTER API routes so /admin/login.html is served but /admin/api/* can still work.
        //
        // Resolution order:
        //   1. Vhost-scoped frontends (if the request's Host: matches a
        //      registered vhost or a default vhost is set).
        //   2. Host-agnostic frontends declared via `.with_frontend_at`.
        //
        // This ordering means declaring a vhost *narrows* serving for
        // that host without breaking path-only apps that never call
        // `.with_vhost`.
        // If a vhost matched (even with zero engines — e.g. registration
        // succeeded but all frontend loads failed), we stay scoped to that
        // vhost: serving host-agnostic frontends in that case would leak
        // them into a host that explicitly opted into isolation.
        let active_engines: &std::collections::HashMap<
            String,
            Arc<crate::frontend::FrontendEngine>,
        > = match vhost_engines {
            Some(e) => e,
            None => &self.frontend_engines,
        };

        // Static-file dispatch accepts both GET and HEAD. HEAD must
        // return the same status + headers as GET with an empty body
        // (RFC 7231 §4.3.2). Before this change, HEAD requests fell
        // straight through to the default 404 below — search engines
        // and uptime monitors that probe with HEAD saw
        // `404 Not Found / Content-Type: application/json` on every
        // static page, even though the body served on GET was correct
        // (issue #56).
        let is_head = method == hyper::Method::HEAD;
        if (method == hyper::Method::GET || is_head) && !active_engines.is_empty() {
            // Sort prefixes by length (longest first) for proper matching
            let mut prefixes: Vec<_> = active_engines.keys().collect();
            prefixes.sort_by_key(|b| std::cmp::Reverse(b.len()));

            // Special handling for _astro assets: try ALL frontends as fallback
            // This allows admin frontend to reference /_astro/* even when served at /secure-xy3xir/
            if path.starts_with("/_astro/") {
                // Try each frontend engine directly via SCC2 lookup
                for prefix in &prefixes {
                    if let Some(engine) = active_engines.get(*prefix) {
                        // Check if this engine has the asset in its SCC2 storage
                        if let Some(asset) = engine.get_asset(&path).await {
                            // Use mime_type from asset
                            let content_length = asset.content.len();
                            let body_bytes: Bytes =
                                if is_head { Bytes::new() } else { Bytes::from(asset.content) };
                            return Ok(hyper::Response::builder()
                                .status(200)
                                .header("Content-Type", asset.mime_type)
                                .header("Content-Length", content_length)
                                .header("Cache-Control", "public, max-age=31536000, immutable")
                                .body(Full::new(body_bytes))
                                .expect("valid HTTP response"));
                        }
                    }
                }
                // All frontends returned 404, fall through to final 404
            } else {
                // Normal path matching: find the first matching prefix
                for prefix in prefixes {
                    if path.starts_with(prefix) {
                        if let Some(engine) = active_engines.get(prefix) {
                            let frontend_server =
                                crate::frontend::FrontendServer::new_scc2(engine.clone());

                            // For non-root frontends, strip the prefix from the path
                            let asset_path = if prefix == "/" {
                                path.to_string()
                            } else {
                                path.strip_prefix(prefix.as_str())
                                    .unwrap_or(path.as_str())
                                    .to_string()
                            };

                            // Create modified request with stripped path
                            let (mut parts, body) = req.into_parts();
                            parts.uri = asset_path.parse().expect("valid URI path");
                            let modified_req = hyper::Request::from_parts(parts, body);

                            // Call frontend server (returns BoxBody, Infallible error)
                            let Ok(response) = frontend_server.handle_request(modified_req).await;
                            // Convert BoxBody to Full<Bytes>
                            use http_body_util::BodyExt;
                            let (parts, body) = response.into_parts();
                            let bytes = body
                                .collect()
                                .await
                                .map_err(|e| anyhow::anyhow!("failed to collect body: {}", e))?
                                .to_bytes();
                            let full_response =
                                hyper::Response::from_parts(parts, Full::new(bytes));
                            return Ok(full_response);
                        }
                        // Break after first prefix match to avoid consuming req multiple times
                        break;
                    }
                }
            }
        }

        // Custom 404 handler (if configured)
        if let Some(ref handler) = self.not_found_handler {
            return (handler)(req).await;
        }

        // 404 Not Found (default)
        Ok(hyper::Response::builder()
            .status(404)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(r#"{"error":"Not found"}"#)))
            .expect("valid HTTP response"))
    }

    /// Handle admin panel request — returns a JSON overview of the running server
    async fn handle_admin_request(
        &self,
        _req: hyper::Request<hyper::body::Incoming>,
    ) -> Result<hyper::Response<http_body_util::Full<bytes::Bytes>>> {
        use http_body_util::Full;

        let models = self.models.read().await;
        let model_names: Vec<&str> = models.iter().map(|m| m.name.as_str()).collect();
        let custom_route_paths: Vec<String> =
            self.custom_routes.iter().map(|r| format!("{} {}", r.method, r.path)).collect();

        let dashboard = serde_json::json!({
            "server": {
                "host": self.config.server.host,
                "port": self.config.server.port,
            },
            "models": model_names,
            "custom_routes": custom_route_paths,
            "features": {
                "admin_panel": self.config.admin.enabled,
                "metrics": self.config.admin.metrics_enabled,
                "data_admin": self.config.admin.data_admin_enabled,
                "sessions": self.session_manager.is_some(),
                "frontend": !self.frontend_engines.is_empty(),
                "cluster": self.config.replication.enabled,
            },
            "endpoints": {
                "admin": self.config.admin.path,
                "metrics": if self.config.admin.metrics_enabled { Some(&self.config.admin.metrics_path) } else { None },
                "schema": "/_admin/schema",
                "data_admin": if self.config.admin.data_admin_enabled { Some("/_admin/data/models") } else { None },
            }
        });

        let body = serde_json::to_string_pretty(&dashboard).unwrap_or_default();

        Ok(hyper::Response::builder()
            .status(200)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(body)))
            .expect("valid HTTP response"))
    }

    /// Handle internal replication request from leader
    /// POST /internal/replicate
    /// Body: { "model": "products", "operation": "create", "data": {...} }
    async fn handle_internal_replicate(
        &self,
        req: hyper::Request<hyper::body::Incoming>,
    ) -> Result<hyper::Response<http_body_util::Full<bytes::Bytes>>> {
        use http_body_util::{BodyExt, Full};

        // Parse body
        let body_bytes = match req.into_body().collect().await.map(|c| c.to_bytes()) {
            Ok(bytes) => bytes,
            Err(_) => {
                return Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::BAD_REQUEST)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(r#"{"error":"Invalid body"}"#)))
                    .expect("valid HTTP response"));
            }
        };

        let message: serde_json::Value = match serde_json::from_slice(&body_bytes) {
            Ok(v) => v,
            Err(e) => {
                return Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::BAD_REQUEST)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(
                        serde_json::json!({"error": format!("Invalid JSON: {}", e)}).to_string(),
                    )))
                    .expect("valid HTTP response"));
            }
        };

        // Extract model base_path from the message if present, else try to match by data structure
        let base_path = message.get("base_path").and_then(|v| v.as_str()).map(|s| s.to_string());

        // Check for consensus-style operation (LithairAppData structure)
        let operation = message.get("operation");
        let model_type = message.get("model_type").and_then(|v| v.as_str());

        // Find the matching model handler
        let models = self.models.read().await;

        // Try to match by base_path, model_type, or fallback to first
        let handler = if let Some(ref path) = base_path {
            models
                .iter()
                .find(|m| m.base_path == *path || m.base_path == format!("/api/{}", path))
        } else if let Some(mtype) = model_type {
            models.iter().find(|m| m.name == mtype || m.base_path.contains(mtype))
        } else {
            // Fallback: use first model (typical single-model clusters)
            models.first()
        };

        if let Some(model) = handler {
            // Handle consensus-style CrudOperation enum from LithairAppData
            if let Some(op) = operation {
                // Parse CrudOperation: {"Create": {...}}, {"Update": {...}}, or {"Delete": {...}}
                if let Some(create_data) = op.get("Create") {
                    let item_data =
                        create_data.get("item").cloned().unwrap_or(serde_json::Value::Null);
                    match model.handler.apply_replicated_item_json(item_data).await {
                        Ok(()) => {
                            log::debug!("CREATE replication applied for model {}", model.name);
                            return Ok(hyper::Response::builder()
                                .status(hyper::StatusCode::OK)
                                .header("Content-Type", "application/json")
                                .body(Full::new(Bytes::from(r#"{"status":"ok"}"#)))
                                .expect("valid HTTP response"));
                        }
                        Err(e) => {
                            log::error!("CREATE replication failed: {}", e);
                            return Ok(hyper::Response::builder()
                                .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                                .header("Content-Type", "application/json")
                                .body(Full::new(Bytes::from(
                                    serde_json::json!({"error": e.to_string()}).to_string(),
                                )))
                                .expect("valid HTTP response"));
                        }
                    }
                } else if let Some(update_data) = op.get("Update") {
                    let item_data =
                        update_data.get("item").cloned().unwrap_or(serde_json::Value::Null);
                    let primary_key =
                        update_data.get("primary_key").and_then(|v| v.as_str()).unwrap_or("");
                    match model.handler.apply_replicated_update_json(primary_key, item_data).await {
                        Ok(()) => {
                            log::debug!("UPDATE replication applied for model {}", model.name);
                            return Ok(hyper::Response::builder()
                                .status(hyper::StatusCode::OK)
                                .header("Content-Type", "application/json")
                                .body(Full::new(Bytes::from(r#"{"status":"ok"}"#)))
                                .expect("valid HTTP response"));
                        }
                        Err(e) => {
                            log::error!("UPDATE replication failed: {}", e);
                            return Ok(hyper::Response::builder()
                                .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                                .header("Content-Type", "application/json")
                                .body(Full::new(Bytes::from(
                                    serde_json::json!({"error": e.to_string()}).to_string(),
                                )))
                                .expect("valid HTTP response"));
                        }
                    }
                } else if let Some(delete_data) = op.get("Delete") {
                    let primary_key =
                        delete_data.get("primary_key").and_then(|v| v.as_str()).unwrap_or("");
                    match model.handler.apply_replicated_delete_json(primary_key).await {
                        Ok(_) => {
                            log::debug!("DELETE replication applied for model {}", model.name);
                            return Ok(hyper::Response::builder()
                                .status(hyper::StatusCode::OK)
                                .header("Content-Type", "application/json")
                                .body(Full::new(Bytes::from(r#"{"status":"ok"}"#)))
                                .expect("valid HTTP response"));
                        }
                        Err(e) => {
                            log::error!("DELETE replication failed: {}", e);
                            return Ok(hyper::Response::builder()
                                .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                                .header("Content-Type", "application/json")
                                .body(Full::new(Bytes::from(
                                    serde_json::json!({"error": e.to_string()}).to_string(),
                                )))
                                .expect("valid HTTP response"));
                        }
                    }
                }
            }

            // Fallback: legacy format with "data" field (CREATE only)
            let item_data = match message.get("data") {
                Some(data) => data.clone(),
                None => {
                    return Ok(hyper::Response::builder()
                        .status(hyper::StatusCode::BAD_REQUEST)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(
                            r#"{"error":"Missing 'data' or 'operation' field"}"#,
                        )))
                        .expect("valid HTTP response"));
                }
            };

            match model.handler.apply_replicated_item_json(item_data).await {
                Ok(()) => {
                    log::debug!("Replication applied for model {}", model.name);
                    Ok(hyper::Response::builder()
                        .status(hyper::StatusCode::OK)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(r#"{"status":"ok"}"#)))
                        .expect("valid HTTP response"))
                }
                Err(e) => {
                    log::error!("Replication failed: {}", e);
                    Ok(hyper::Response::builder()
                        .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(
                            serde_json::json!({"error": e.to_string()}).to_string(),
                        )))
                        .expect("valid HTTP response"))
                }
            }
        } else {
            Ok(hyper::Response::builder()
                .status(hyper::StatusCode::NOT_FOUND)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(r#"{"error":"No model handler found"}"#)))
                .expect("valid HTTP response"))
        }
    }

    /// Handle bulk internal replication request from leader
    /// POST /internal/replicate_bulk
    /// Body: { "model": "products", "items": [...], "batch_id": "..." }
    async fn handle_internal_replicate_bulk(
        &self,
        req: hyper::Request<hyper::body::Incoming>,
    ) -> Result<hyper::Response<http_body_util::Full<bytes::Bytes>>> {
        use http_body_util::{BodyExt, Full};

        // Parse body
        let body_bytes = match req.into_body().collect().await.map(|c| c.to_bytes()) {
            Ok(bytes) => bytes,
            Err(_) => {
                return Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::BAD_REQUEST)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(r#"{"error":"Invalid body"}"#)))
                    .expect("valid HTTP response"));
            }
        };

        let message: serde_json::Value = match serde_json::from_slice(&body_bytes) {
            Ok(v) => v,
            Err(e) => {
                return Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::BAD_REQUEST)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(
                        serde_json::json!({"error": format!("Invalid JSON: {}", e)}).to_string(),
                    )))
                    .expect("valid HTTP response"));
            }
        };

        // Extract model base_path
        let base_path = message.get("base_path").and_then(|v| v.as_str()).map(|s| s.to_string());

        // Get the items array
        let items: Vec<serde_json::Value> = match message.get("items") {
            Some(serde_json::Value::Array(arr)) => arr.clone(),
            _ => {
                return Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::BAD_REQUEST)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(r#"{"error":"Missing or invalid 'items' field"}"#)))
                    .expect("valid HTTP response"));
            }
        };

        let batch_id = message.get("batch_id").and_then(|v| v.as_str()).unwrap_or("unknown");

        // Find the matching model handler
        let models = self.models.read().await;

        let handler = if let Some(ref path) = base_path {
            models
                .iter()
                .find(|m| m.base_path == *path || m.base_path == format!("/api/{}", path))
        } else {
            models.first()
        };

        if let Some(model) = handler {
            match model.handler.apply_replicated_items_json(items).await {
                Ok(count) => {
                    log::debug!(
                        "Bulk replication applied: {} items for model {} (batch: {})",
                        count,
                        model.name,
                        batch_id
                    );
                    Ok(hyper::Response::builder()
                        .status(hyper::StatusCode::OK)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(format!(
                            r#"{{"status":"ok","count":{}}}"#,
                            count
                        ))))
                        .expect("valid HTTP response"))
                }
                Err(e) => {
                    log::error!("Bulk replication failed: {}", e);
                    Ok(hyper::Response::builder()
                        .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(
                            serde_json::json!({"error": e.to_string()}).to_string(),
                        )))
                        .expect("valid HTTP response"))
                }
            }
        } else {
            Ok(hyper::Response::builder()
                .status(hyper::StatusCode::NOT_FOUND)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(r#"{"error":"No model handler found"}"#)))
                .expect("valid HTTP response"))
        }
    }

    /// Handle internal UPDATE replication request from leader
    /// POST /internal/replicate_update
    /// Body: { "base_path": "products", "id": "123", "data": {...} }
    async fn handle_internal_replicate_update(
        &self,
        req: hyper::Request<hyper::body::Incoming>,
    ) -> Result<hyper::Response<http_body_util::Full<bytes::Bytes>>> {
        use http_body_util::{BodyExt, Full};

        // Parse body
        let body_bytes = match req.into_body().collect().await.map(|c| c.to_bytes()) {
            Ok(bytes) => bytes,
            Err(_) => {
                return Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::BAD_REQUEST)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(r#"{"error":"Invalid body"}"#)))
                    .expect("valid HTTP response"));
            }
        };

        let message: serde_json::Value = match serde_json::from_slice(&body_bytes) {
            Ok(v) => v,
            Err(e) => {
                return Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::BAD_REQUEST)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(
                        serde_json::json!({"error": format!("Invalid JSON: {}", e)}).to_string(),
                    )))
                    .expect("valid HTTP response"));
            }
        };

        // Extract required fields
        let base_path = message.get("base_path").and_then(|v| v.as_str()).map(|s| s.to_string());

        let id = match message.get("id").and_then(|v| v.as_str()) {
            Some(id) => id.to_string(),
            None => {
                return Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::BAD_REQUEST)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(r#"{"error":"Missing 'id' field"}"#)))
                    .expect("valid HTTP response"));
            }
        };

        let item_data = match message.get("data") {
            Some(data) => data.clone(),
            None => {
                return Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::BAD_REQUEST)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(r#"{"error":"Missing 'data' field"}"#)))
                    .expect("valid HTTP response"));
            }
        };

        // Find the matching model handler
        let models = self.models.read().await;

        let handler = if let Some(ref path) = base_path {
            models
                .iter()
                .find(|m| m.base_path == *path || m.base_path == format!("/api/{}", path))
        } else {
            models.first()
        };

        if let Some(model) = handler {
            match model.handler.apply_replicated_update_json(&id, item_data).await {
                Ok(()) => {
                    log::debug!("Replication UPDATE applied for {} in model {}", id, model.name);
                    Ok(hyper::Response::builder()
                        .status(hyper::StatusCode::OK)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(r#"{"status":"ok"}"#)))
                        .expect("valid HTTP response"))
                }
                Err(e) => {
                    log::error!("Replication UPDATE failed: {}", e);
                    Ok(hyper::Response::builder()
                        .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(
                            serde_json::json!({"error": e.to_string()}).to_string(),
                        )))
                        .expect("valid HTTP response"))
                }
            }
        } else {
            Ok(hyper::Response::builder()
                .status(hyper::StatusCode::NOT_FOUND)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(r#"{"error":"No model handler found"}"#)))
                .expect("valid HTTP response"))
        }
    }

    /// Handle internal DELETE replication request from leader
    /// POST /internal/replicate_delete
    /// Body: { "base_path": "products", "id": "123" }
    async fn handle_internal_replicate_delete(
        &self,
        req: hyper::Request<hyper::body::Incoming>,
    ) -> Result<hyper::Response<http_body_util::Full<bytes::Bytes>>> {
        use http_body_util::{BodyExt, Full};

        // Parse body
        let body_bytes = match req.into_body().collect().await.map(|c| c.to_bytes()) {
            Ok(bytes) => bytes,
            Err(_) => {
                return Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::BAD_REQUEST)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(r#"{"error":"Invalid body"}"#)))
                    .expect("valid HTTP response"));
            }
        };

        let message: serde_json::Value = match serde_json::from_slice(&body_bytes) {
            Ok(v) => v,
            Err(e) => {
                return Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::BAD_REQUEST)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(
                        serde_json::json!({"error": format!("Invalid JSON: {}", e)}).to_string(),
                    )))
                    .expect("valid HTTP response"));
            }
        };

        // Extract required fields
        let base_path = message.get("base_path").and_then(|v| v.as_str()).map(|s| s.to_string());

        let id = match message.get("id").and_then(|v| v.as_str()) {
            Some(id) => id.to_string(),
            None => {
                return Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::BAD_REQUEST)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(r#"{"error":"Missing 'id' field"}"#)))
                    .expect("valid HTTP response"));
            }
        };

        // Find the matching model handler
        let models = self.models.read().await;

        let handler = if let Some(ref path) = base_path {
            models
                .iter()
                .find(|m| m.base_path == *path || m.base_path == format!("/api/{}", path))
        } else {
            models.first()
        };

        if let Some(model) = handler {
            match model.handler.apply_replicated_delete_json(&id).await {
                Ok(deleted) => {
                    log::debug!(
                        "Replication DELETE applied for {} in model {} (deleted: {})",
                        id,
                        model.name,
                        deleted
                    );
                    Ok(hyper::Response::builder()
                        .status(hyper::StatusCode::OK)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(format!(
                            r#"{{"status":"ok","deleted":{}}}"#,
                            deleted
                        ))))
                        .expect("valid HTTP response"))
                }
                Err(e) => {
                    log::error!("Replication DELETE failed: {}", e);
                    Ok(hyper::Response::builder()
                        .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(
                            serde_json::json!({"error": e.to_string()}).to_string(),
                        )))
                        .expect("valid HTTP response"))
                }
            }
        } else {
            Ok(hyper::Response::builder()
                .status(hyper::StatusCode::NOT_FOUND)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(r#"{"error":"No model handler found"}"#)))
                .expect("valid HTTP response"))
        }
    }

    /// Handle Raft append entries request from leader
    /// POST /_raft/append
    /// Body: AppendEntriesRequest { term, leader_id, prev_log_index, prev_log_term, entries, leader_commit }
    ///
    /// This endpoint is called by the leader to replicate log entries to followers.
    /// Followers:
    /// 1. Store the entries in their local log
    /// 2. Update their commit index based on leader_commit
    /// 3. Apply committed entries to their state machine
    async fn handle_raft_append_entries(
        &self,
        req: hyper::Request<hyper::body::Incoming>,
    ) -> Result<hyper::Response<http_body_util::Full<bytes::Bytes>>> {
        use http_body_util::BodyExt;
        use http_body_util::Full;

        // Parse request body
        let (_parts, body) = req.into_parts();
        let body_bytes = body.collect().await?.to_bytes();

        let request: crate::cluster::consensus_log::AppendEntriesRequest =
            match serde_json::from_slice(&body_bytes) {
                Ok(r) => r,
                Err(e) => {
                    return Ok(hyper::Response::builder()
                        .status(hyper::StatusCode::BAD_REQUEST)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(format!(
                            r#"{{"error":"Invalid request: {}"}}"#,
                            e
                        ))))
                        .expect("valid HTTP response"));
                }
            };

        // Check if we have a consensus log
        let consensus_log = match &self.consensus_log {
            Some(log) => log,
            None => {
                return Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::SERVICE_UNAVAILABLE)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(r#"{"error":"Consensus log not initialized"}"#)))
                    .expect("valid HTTP response"));
            }
        };

        // Check term - if leader's term is higher, update ours
        let our_term = consensus_log.current_term();
        if request.term > our_term {
            consensus_log.set_term(request.term);
        } else if request.term < our_term {
            // Reject requests from old leaders
            let response = crate::cluster::consensus_log::AppendEntriesResponse {
                term: our_term,
                success: false,
                last_log_index: consensus_log.last_index().await,
                applied_index: consensus_log.applied_index(),
            };
            return Ok(hyper::Response::builder()
                .status(hyper::StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(serde_json::to_vec(&response).unwrap_or_default())))
                .expect("valid HTTP response"));
        }

        // Reconcile Raft role: an accepted AppendEntries means another node is
        // the legitimate leader for this term. If we still think we're a
        // leader/candidate, or the leader has changed, take the full
        // become_follower path so we never keep an old leader's port mapped to
        // a new leader_id (relevant when a legacy peer omits leader_port and
        // update_leader_port would preserve the stale port).
        if let Some(ref raft_state) = self.raft_state {
            let was_follower =
                raft_state.get_current_state() == crate::cluster::RaftNodeState::Follower;
            let same_leader =
                raft_state.current_leader_id.load(std::sync::atomic::Ordering::Relaxed)
                    == request.leader_id;
            if was_follower && same_leader {
                raft_state.update_leader_port(request.leader_id, request.leader_port);
            } else {
                raft_state.become_follower(request.leader_id, request.leader_port);
            }
            raft_state.update_heartbeat();
        }

        // Append entries to local log (can happen concurrently)
        let entries_count = request.entries.len();
        consensus_log
            .append_entries(request.entries.clone(), request.leader_commit)
            .await;

        log::debug!(
            "Received {} entries from leader {}, commit_index={}",
            entries_count,
            request.leader_id,
            request.leader_commit
        );

        // CRITICAL: Acquire the apply lock BEFORE getting unapplied entries and applying them.
        // This prevents race conditions where multiple concurrent handlers could:
        // 1. Both see the same entries as unapplied
        // 2. Apply entries out of order (e.g., DELETE before CREATE)
        // 3. Cause data inconsistency (items existing on followers but not leader, or vice versa)
        let _apply_guard = consensus_log.lock_apply().await;

        // Apply committed entries that we haven't applied yet
        // Since we send ALL entries from index 1, there should be no gaps
        let unapplied = consensus_log.get_unapplied_entries().await;
        let mut all_applied_successfully = true;

        for entry in unapplied {
            let op_type = match &entry.operation {
                crate::cluster::CrudOperation::Create { .. } => "CREATE",
                crate::cluster::CrudOperation::Update { .. } => "UPDATE",
                crate::cluster::CrudOperation::Delete { .. } => "DELETE",
                crate::cluster::CrudOperation::MigrationBegin { .. } => "MIGRATION_BEGIN",
                crate::cluster::CrudOperation::MigrationStep { .. } => "MIGRATION_STEP",
                crate::cluster::CrudOperation::MigrationCommit { .. } => "MIGRATION_COMMIT",
                crate::cluster::CrudOperation::MigrationRollback { .. } => "MIGRATION_ROLLBACK",
            };
            log::debug!("FOLLOWER: Applying {} entry index={}", op_type, entry.log_id.index);
            match self.apply_crud_operation(&entry.operation).await {
                Ok(_) => {
                    consensus_log.mark_applied(entry.log_id.index);
                    log::debug!("Applied entry index={}", entry.log_id.index);
                }
                Err(e) => {
                    // CRITICAL: Stop processing here! If we continue, we'd skip this entry
                    // because mark_applied on later entries would advance applied_index past it.
                    // Mark as NOT all successful so leader knows to retry
                    log::error!(
                        "Failed to apply entry index={}: {} - stopping to prevent skip",
                        entry.log_id.index,
                        e
                    );
                    all_applied_successfully = false;
                    break;
                }
            }
        }
        // _apply_guard dropped here, releasing the lock

        // Only report success if all entries were applied successfully
        // If some failed, leader will retry via background catch-up
        let response = crate::cluster::consensus_log::AppendEntriesResponse {
            term: consensus_log.current_term(),
            success: all_applied_successfully,
            last_log_index: consensus_log.last_index().await,
            applied_index: consensus_log.applied_index(),
        };

        Ok(hyper::Response::builder()
            .status(hyper::StatusCode::OK)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(serde_json::to_vec(&response).unwrap_or_default())))
            .expect("valid HTTP response"))
    }

    /// Handle GET /_raft/snapshot - Return current snapshot for resync
    ///
    /// This endpoint is called by desynced followers to get a full snapshot
    /// of the leader's state for faster catch-up than replaying all logs.
    async fn handle_get_snapshot(
        &self,
        _req: hyper::Request<hyper::body::Incoming>,
    ) -> Result<hyper::Response<http_body_util::Full<bytes::Bytes>>> {
        use http_body_util::Full;

        // Check if snapshot manager is available
        let snapshot_manager = match &self.snapshot_manager {
            Some(mgr) => mgr,
            None => {
                return Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::SERVICE_UNAVAILABLE)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(r#"{"error":"Snapshot manager not initialized"}"#)))
                    .expect("valid HTTP response"));
            }
        };

        // Get current snapshot metadata
        let mgr = snapshot_manager.read().await;
        let meta = match mgr.current_meta() {
            Some(m) => m.clone(),
            None => {
                return Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::NOT_FOUND)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(r#"{"error":"No snapshot available"}"#)))
                    .expect("valid HTTP response"));
            }
        };

        // Get snapshot bytes
        match mgr.get_snapshot_bytes(meta.last_included_index) {
            Ok(bytes) => {
                // Return snapshot with metadata in headers
                Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::OK)
                    .header("Content-Type", "application/octet-stream")
                    .header("X-Snapshot-Term", meta.term.to_string())
                    .header("X-Snapshot-Index", meta.last_included_index.to_string())
                    .header("X-Snapshot-Checksum", meta.checksum.to_string())
                    .header("X-Snapshot-Size", meta.size_bytes.to_string())
                    .body(Full::new(Bytes::from(bytes)))
                    .expect("valid HTTP response"))
            }
            Err(e) => {
                log::error!("Failed to read snapshot: {}", e);
                Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(format!(
                        r#"{{"error":"Failed to read snapshot: {}"}}"#,
                        e
                    ))))
                    .expect("valid HTTP response"))
            }
        }
    }

    /// Handle POST /_raft/snapshot - Install snapshot received from leader
    ///
    /// Desynced followers call this to install a snapshot and catch up quickly.
    /// After installation, the follower's state is reset to the snapshot state.
    async fn handle_install_snapshot(
        &self,
        req: hyper::Request<hyper::body::Incoming>,
    ) -> Result<hyper::Response<http_body_util::Full<bytes::Bytes>>> {
        use http_body_util::{BodyExt, Full};

        // Check if snapshot manager is available
        let snapshot_manager = match &self.snapshot_manager {
            Some(mgr) => mgr,
            None => {
                return Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::SERVICE_UNAVAILABLE)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(r#"{"error":"Snapshot manager not initialized"}"#)))
                    .expect("valid HTTP response"));
            }
        };

        // Extract metadata from headers
        let headers = req.headers();
        let term: u64 = headers
            .get("X-Snapshot-Term")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let last_included_index: u64 = headers
            .get("X-Snapshot-Index")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let checksum: u64 = headers
            .get("X-Snapshot-Checksum")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let size_bytes: u64 = headers
            .get("X-Snapshot-Size")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        // Read body bytes
        // IMPORTANT: Convert to Vec<u8> for proper alignment - rkyv 0.8's bytecheck
        // validation requires aligned data, and bytes::Bytes may not provide this
        let body_bytes: Vec<u8> = match req.into_body().collect().await.map(|c| c.to_bytes()) {
            Ok(bytes) => bytes.to_vec(),
            Err(e) => {
                return Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::BAD_REQUEST)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(format!(
                        r#"{{"error":"Failed to read body: {}"}}"#,
                        e
                    ))))
                    .expect("valid HTTP response"));
            }
        };

        // Create metadata
        let meta = crate::cluster::snapshot::SnapshotMeta {
            term,
            last_included_index,
            created_at_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            size_bytes,
            checksum,
        };

        // Record snapshot received (for observability)
        self.resync_stats.record_snapshot_received(last_included_index);

        // Install the snapshot
        let mut mgr = snapshot_manager.write().await;
        match mgr.install_snapshot(meta.clone(), &body_bytes) {
            Ok(snapshot_data) => {
                log::info!("Snapshot installed: index={}, term={}", last_included_index, term);

                // Record snapshot applied (for observability)
                self.resync_stats.record_snapshot_applied();

                // Apply snapshot data to models
                let models = self.models.read().await;
                for (model_path, json_data) in &snapshot_data.models {
                    if let Some(model) = models.iter().find(|m| m.base_path == *model_path) {
                        let items: Vec<serde_json::Value> =
                            serde_json::from_str(json_data).unwrap_or_default();
                        if let Err(e) = model.handler.apply_replicated_items_json(items).await {
                            log::error!("Failed to apply snapshot data for {}: {}", model_path, e);
                        }
                    }
                }

                // Update consensus log if present
                if let Some(ref consensus_log) = self.consensus_log {
                    consensus_log.set_term(term);
                    // Mark entries up to snapshot index as applied
                    consensus_log.mark_applied(last_included_index);
                }

                let response = crate::cluster::snapshot::InstallSnapshotResponse {
                    term,
                    success: true,
                    error: None,
                };

                Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::OK)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(serde_json::to_vec(&response).unwrap_or_default())))
                    .expect("valid HTTP response"))
            }
            Err(e) => {
                log::error!("Failed to install snapshot: {}", e);
                let response = crate::cluster::snapshot::InstallSnapshotResponse {
                    term: self.consensus_log.as_ref().map(|l| l.current_term()).unwrap_or(0),
                    success: false,
                    error: Some(e.to_string()),
                };

                Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(serde_json::to_vec(&response).unwrap_or_default())))
                    .expect("valid HTTP response"))
            }
        }
    }

    /// Handle GET /_raft/health - Return cluster health status
    ///
    /// Returns detailed health information about all followers including:
    /// - Health status (healthy, lagging, desynced, unknown)
    /// - Last replicated index
    /// - Latency statistics
    /// - Pending entry counts
    async fn handle_cluster_health(
        &self,
    ) -> Result<hyper::Response<http_body_util::Full<bytes::Bytes>>> {
        use http_body_util::Full;

        let mut health_data = serde_json::json!({
            "status": "ok",
            "node_id": self.node_id,
            "is_leader": self.raft_state.as_ref().map(|s| s.is_leader()).unwrap_or(false),
            "cluster_peers": self.cluster_peers.len(),
        });

        // Add consensus log info if present
        if let Some(ref consensus_log) = self.consensus_log {
            health_data["consensus"] = serde_json::json!({
                "term": consensus_log.current_term(),
                "commit_index": consensus_log.commit_index(),
                "last_applied": consensus_log.applied_index(),
            });
        }

        // Add batcher health summary if present
        if let Some(ref batcher) = self.replication_batcher {
            let summary = batcher.get_health_summary().await;
            let mut followers = Vec::new();

            for (addr, health) in summary {
                let mut follower_info = serde_json::json!({
                    "address": addr,
                    "health": health.to_string(),
                });

                // Get detailed stats if available
                if let Some(stats) = batcher.get_follower_stats(&addr).await {
                    follower_info["last_replicated_index"] =
                        serde_json::json!(stats.last_replicated_index);
                    follower_info["last_latency_ms"] = serde_json::json!(stats.last_latency_ms);
                    follower_info["pending_count"] = serde_json::json!(stats.pending_count);
                    follower_info["consecutive_failures"] =
                        serde_json::json!(stats.consecutive_failures);
                }

                followers.push(follower_info);
            }

            health_data["followers"] = serde_json::json!(followers);

            // Check for desynced followers
            let commit_index = self.consensus_log.as_ref().map(|l| l.commit_index()).unwrap_or(0);
            let desynced = batcher.get_desynced_followers(commit_index).await;
            if !desynced.is_empty() {
                health_data["desynced_followers"] = serde_json::json!(desynced);
            }
        }

        // Add WAL info if present
        if self.wal.is_some() {
            health_data["wal"] = serde_json::json!({
                "enabled": true,
            });
        }

        // Add snapshot info if present
        if let Some(ref snapshot_manager) = self.snapshot_manager {
            let mgr = snapshot_manager.read().await;
            if let Some(meta) = mgr.current_meta() {
                health_data["snapshot"] = serde_json::json!({
                    "available": true,
                    "term": meta.term,
                    "last_included_index": meta.last_included_index,
                    "size_bytes": meta.size_bytes,
                    "created_at_ms": meta.created_at_ms,
                });
            } else {
                health_data["snapshot"] = serde_json::json!({
                    "available": false,
                });
            }
        }

        Ok(hyper::Response::builder()
            .status(hyper::StatusCode::OK)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(
                serde_json::to_string_pretty(&health_data).unwrap_or_default(),
            )))
            .expect("valid HTTP response"))
    }

    /// Handle GET /_raft/resync_stats - Return snapshot resync statistics
    ///
    /// Returns observability data for snapshot-based resync operations:
    /// - Leader side: snapshots created, send attempts/successes/failures
    /// - Follower side: snapshots received, snapshots applied
    /// - Indices and timestamps for debugging
    async fn handle_resync_stats(
        &self,
    ) -> Result<hyper::Response<http_body_util::Full<bytes::Bytes>>> {
        use http_body_util::Full;

        let stats_json = self.resync_stats.to_json();

        let response_data = serde_json::json!({
            "node_id": self.node_id,
            "is_leader": self.raft_state.as_ref().map(|s| s.is_leader()).unwrap_or(false),
            "resync_stats": stats_json,
        });

        Ok(hyper::Response::builder()
            .status(hyper::StatusCode::OK)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(
                serde_json::to_string_pretty(&response_data).unwrap_or_default(),
            )))
            .expect("valid HTTP response"))
    }

    /// Handle GET /_raft/sync-status - Return detailed sync status for each follower
    ///
    /// Returns for each follower:
    /// - address: peer address
    /// - health: healthy/lagging/desynced/unknown
    /// - last_replicated_index: last known replicated index
    /// - lag: how many entries behind the leader commit_index
    /// - last_latency_ms: last replication latency
    /// - pending_count: pending batched entries
    /// - consecutive_failures: failure counter
    async fn handle_sync_status(
        &self,
    ) -> Result<hyper::Response<http_body_util::Full<bytes::Bytes>>> {
        use http_body_util::Full;

        let is_leader = self.raft_state.as_ref().map(|s| s.is_leader()).unwrap_or(false);

        if !is_leader {
            return Ok(hyper::Response::builder()
                .status(hyper::StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(serde_json::to_string_pretty(&serde_json::json!({
                    "node_id": self.node_id,
                    "is_leader": false,
                    "message": "This node is not the leader. Sync status is only available on the leader."
                })).unwrap_or_default())))
                .expect("valid HTTP response"));
        }

        // Get commit index from consensus log
        let commit_index = if let Some(log) = &self.consensus_log { log.commit_index() } else { 0 };

        // Get follower stats from batcher
        let followers_stats = if let Some(batcher) = &self.replication_batcher {
            batcher.get_all_follower_stats().await
        } else {
            vec![]
        };

        // Build response with lag calculation
        let followers_json: Vec<serde_json::Value> = followers_stats
            .iter()
            .map(|f| {
                let lag = commit_index.saturating_sub(f.last_replicated_index);

                serde_json::json!({
                    "address": f.address,
                    "health": f.health.to_string(),
                    "last_replicated_index": f.last_replicated_index,
                    "lag": lag,
                    "last_latency_ms": f.last_latency_ms,
                    "pending_count": f.pending_count,
                    "consecutive_failures": f.consecutive_failures,
                })
            })
            .collect();

        let response_data = serde_json::json!({
            "node_id": self.node_id,
            "is_leader": true,
            "commit_index": commit_index,
            "followers": followers_json,
        });

        Ok(hyper::Response::builder()
            .status(hyper::StatusCode::OK)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(
                serde_json::to_string_pretty(&response_data).unwrap_or_default(),
            )))
            .expect("valid HTTP response"))
    }

    /// Handle POST /_raft/force-resync - Manually trigger snapshot resync to a follower
    ///
    /// Query params:
    /// - target: peer address (e.g., "127.0.0.1:8081")
    ///
    /// This marks the follower as desynced and triggers immediate snapshot send.
    /// Use this when a node has restarted and needs to catch up from scratch.
    async fn handle_force_resync(
        &self,
        req: hyper::Request<hyper::body::Incoming>,
    ) -> Result<hyper::Response<http_body_util::Full<bytes::Bytes>>> {
        use http_body_util::Full;

        let is_leader = self.raft_state.as_ref().map(|s| s.is_leader()).unwrap_or(false);

        if !is_leader {
            return Ok(hyper::Response::builder()
                .status(hyper::StatusCode::BAD_REQUEST)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(r#"{"error":"This node is not the leader. Force resync must be called on the leader."}"#)))
                .expect("valid HTTP response"));
        }

        // Parse target from query string
        let uri = req.uri();
        let query = uri.query().unwrap_or("");
        let target = query.split('&').find_map(|pair| {
            let mut parts = pair.split('=');
            match (parts.next(), parts.next()) {
                (Some("target"), Some(value)) => Some(value.to_string()),
                _ => None,
            }
        });

        let target = match target {
            Some(t) => t,
            None => {
                return Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::BAD_REQUEST)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(r#"{"error":"Missing 'target' query parameter. Use /_raft/force-resync?target=127.0.0.1:8081"}"#)))
                    .expect("valid HTTP response"));
            }
        };

        log::info!("Manual resync requested for follower: {}", target);

        // Mark follower as desynced
        let marked = if let Some(batcher) = &self.replication_batcher {
            batcher.mark_follower_desynced(&target).await
        } else {
            false
        };

        if !marked {
            return Ok(hyper::Response::builder()
                .status(hyper::StatusCode::NOT_FOUND)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(format!(
                    r#"{{"error":"Follower '{}' not found in cluster"}}"#,
                    target
                ))))
                .expect("valid HTTP response"));
        }

        // Trigger immediate snapshot send
        let snapshot_result = if let Some(snapshot_manager) = &self.snapshot_manager {
            Self::send_snapshot_to_follower_with_timeout(&target, snapshot_manager, 60).await
        } else {
            Err("Snapshot manager not available".to_string())
        };

        // Update resync stats
        // Stats will be recorded after we know the result

        let (status, message) = match snapshot_result {
            Ok(()) => {
                self.resync_stats.record_send_success();
                log::info!("Manual resync to {} completed successfully", target);
                (
                    hyper::StatusCode::OK,
                    format!("Snapshot resync to {} completed successfully", target),
                )
            }
            Err(e) => {
                self.resync_stats.record_send_failure();
                log::error!("Manual resync to {} failed: {}", target, e);
                (hyper::StatusCode::INTERNAL_SERVER_ERROR, format!("Resync failed: {}", e))
            }
        };

        Ok(hyper::Response::builder()
            .status(status)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(
                serde_json::to_string_pretty(&serde_json::json!({
                    "target": target,
                    "success": status == hyper::StatusCode::OK,
                    "message": message,
                }))
                .unwrap_or_default(),
            )))
            .expect("valid HTTP response"))
    }

    /// Handle POST /_raft/migrate - Submit migration operations through consensus
    ///
    /// This endpoint allows submitting migration operations (MigrationBegin, MigrationStep,
    /// MigrationCommit, MigrationRollback) to be replicated through the Raft consensus log.
    /// Only the leader can accept these operations.
    async fn handle_migrate_operation(
        &self,
        mut req: hyper::Request<hyper::body::Incoming>,
    ) -> Result<hyper::Response<http_body_util::Full<bytes::Bytes>>> {
        use http_body_util::{BodyExt, Full};

        // Only leader can accept migration operations
        let is_leader = self.raft_state.as_ref().map(|s| s.is_leader()).unwrap_or(true);
        if !is_leader {
            let leader_port = self.raft_state.as_ref().map(|s| s.get_leader_port()).unwrap_or(0);
            if leader_port == 0 {
                return Ok(Self::leader_port_unknown_503());
            }
            return Ok(hyper::Response::builder()
                .status(hyper::StatusCode::TEMPORARY_REDIRECT)
                .header("Content-Type", "application/json")
                .header("Location", format!("http://127.0.0.1:{}/_raft/migrate", leader_port))
                .body(Full::new(Bytes::from(
                    serde_json::json!({
                        "error": "Not leader",
                        "leader_port": leader_port
                    })
                    .to_string(),
                )))
                .expect("valid HTTP response"));
        }

        // Parse the operation from request body
        let body = req
            .body_mut()
            .collect()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read body: {}", e))?
            .to_bytes();

        let operation: crate::cluster::CrudOperation = match serde_json::from_slice(&body) {
            Ok(op) => op,
            Err(e) => {
                return Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::BAD_REQUEST)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(
                        serde_json::json!({
                            "error": format!("Invalid operation: {}", e)
                        })
                        .to_string(),
                    )))
                    .expect("valid HTTP response"));
            }
        };

        // Verify it's a migration operation
        let is_migration = matches!(
            operation,
            crate::cluster::CrudOperation::MigrationBegin { .. }
                | crate::cluster::CrudOperation::MigrationStep { .. }
                | crate::cluster::CrudOperation::MigrationCommit { .. }
                | crate::cluster::CrudOperation::MigrationRollback { .. }
        );

        if !is_migration {
            return Ok(hyper::Response::builder()
                .status(hyper::StatusCode::BAD_REQUEST)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(
                    serde_json::json!({
                        "error": "Only migration operations are allowed on this endpoint"
                    })
                    .to_string(),
                )))
                .expect("valid HTTP response"));
        }

        // Create log entry and replicate
        if let Some(ref consensus_log) = self.consensus_log {
            // Append to local log (creates LogEntry internally)
            let entry = consensus_log.append(operation.clone()).await;

            // Write to WAL
            if let Some(ref wal) = self.wal {
                if let Err(e) = wal.append(&entry).await {
                    log::error!("Failed to write migration to WAL: {}", e);
                }
            }

            // Replicate to followers
            let commit_index = consensus_log.commit_index();
            let term = consensus_log.current_term();
            let leader_id = self.node_id.unwrap_or(0);

            let replication_result = Self::replicate_log_entries_to_followers(
                &self.cluster_peers,
                vec![entry],
                commit_index,
                term,
                leader_id,
                self.config.server.port,
                self.replication_batcher.clone(),
            )
            .await;

            match replication_result {
                Ok(new_commit) => {
                    // Update commit index
                    consensus_log.commit(new_commit);

                    // Apply the operation locally
                    let apply_result = self.apply_crud_operation(&operation).await;

                    match apply_result {
                        Ok(result) => Ok(hyper::Response::builder()
                            .status(hyper::StatusCode::OK)
                            .header("Content-Type", "application/json")
                            .body(Full::new(Bytes::from(
                                serde_json::json!({
                                    "success": true,
                                    "commit_index": new_commit,
                                    "result": result
                                })
                                .to_string(),
                            )))
                            .expect("valid HTTP response")),
                        Err(e) => Ok(hyper::Response::builder()
                            .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                            .header("Content-Type", "application/json")
                            .body(Full::new(Bytes::from(
                                serde_json::json!({
                                    "error": format!("Migration apply failed: {}", e),
                                    "commit_index": new_commit
                                })
                                .to_string(),
                            )))
                            .expect("valid HTTP response")),
                    }
                }
                Err(e) => Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::SERVICE_UNAVAILABLE)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(
                        serde_json::json!({
                            "error": format!("Replication failed: {}", e)
                        })
                        .to_string(),
                    )))
                    .expect("valid HTTP response")),
            }
        } else {
            // Single node mode - just apply
            let apply_result = self.apply_crud_operation(&operation).await;
            match apply_result {
                Ok(result) => Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::OK)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(
                        serde_json::json!({
                            "success": true,
                            "result": result
                        })
                        .to_string(),
                    )))
                    .expect("valid HTTP response")),
                Err(e) => Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(
                        serde_json::json!({
                            "error": format!("Migration failed: {}", e)
                        })
                        .to_string(),
                    )))
                    .expect("valid HTTP response")),
            }
        }
    }

    /// Handle metrics request
    async fn handle_metrics_request(
        &self,
        _req: hyper::Request<hyper::body::Incoming>,
    ) -> Result<hyper::Response<http_body_util::Full<bytes::Bytes>>> {
        use http_body_util::Full;

        // Escape a string for use as a Prometheus label value per the text
        // exposition format spec (backslash, double quote, newline).
        fn escape_prom_label(s: &str) -> String {
            let mut out = String::with_capacity(s.len());
            for c in s.chars() {
                match c {
                    '\\' => out.push_str("\\\\"),
                    '"' => out.push_str("\\\""),
                    '\n' => out.push_str("\\n"),
                    c => out.push(c),
                }
            }
            out
        }

        // Snapshot the registered models under the read lock, then drop it
        // before awaiting per-model `get_stats` — otherwise long-running
        // sampling I/O blocks any writer trying to register a new model
        // (Gemini PR #83 review). Cloning the Arcs and the small metadata
        // strings is cheap; holding the lock across `.await` is not.
        let models_snapshot: Vec<(String, String, Arc<dyn crate::app::ModelHandler>)> = {
            let models = self.models.read().await;
            models
                .iter()
                .map(|m| (m.name.clone(), m.data_path.clone(), Arc::clone(&m.handler)))
                .collect()
        };

        let mut lines = Vec::new();

        lines.push("# HELP lithair_models_total Number of registered models".to_string());
        lines.push("# TYPE lithair_models_total gauge".to_string());
        lines.push(format!("lithair_models_total {}", models_snapshot.len()));

        lines.push("# HELP lithair_custom_routes_total Number of custom routes".to_string());
        lines.push("# TYPE lithair_custom_routes_total gauge".to_string());
        lines.push(format!("lithair_custom_routes_total {}", self.custom_routes.len()));

        lines.push("# HELP lithair_frontend_engines_total Number of frontend engines".to_string());
        lines.push("# TYPE lithair_frontend_engines_total gauge".to_string());
        lines.push(format!("lithair_frontend_engines_total {}", self.frontend_engines.len()));

        // Per-model storage stats (issue #72). One series per registered model.
        // approx_ram_bytes is a sample-based estimate — see ModelStats docs.
        // Compute stats once per model to avoid 3x I/O for raftlog metadata.
        //
        // Stats collection is parallelised via `futures::future::join_all` so
        // `/metrics` scales with the slowest model, not the sum of all models
        // (Gemini PR #83 round-3 review). The read lock has already been
        // dropped above, so the per-model `get_stats` futures are independent
        // — there's no shared mutable state across them. With N models the
        // wall-clock cost drops from sum(get_stats_i) to max(get_stats_i),
        // which matters for operators with many small models scraped by a
        // tight Prometheus interval.
        let stats_futures =
            models_snapshot.into_iter().map(|(name, data_path, handler)| async move {
                let stats = handler.get_stats(&data_path).await;
                (escape_prom_label(&name), stats)
            });
        let per_model: Vec<(String, crate::app::ModelStats)> =
            futures::future::join_all(stats_futures).await;

        lines.push("# HELP lithair_model_items Number of items held in RAM per model".to_string());
        lines.push("# TYPE lithair_model_items gauge".to_string());
        for (label, stats) in &per_model {
            lines.push(format!("lithair_model_items{{model=\"{}\"}} {}", label, stats.item_count));
        }

        lines.push(
            "# HELP lithair_model_ram_bytes Approximate per-model RAM cost in bytes (sampled)"
                .to_string(),
        );
        lines.push("# TYPE lithair_model_ram_bytes gauge".to_string());
        for (label, stats) in &per_model {
            lines.push(format!(
                "lithair_model_ram_bytes{{model=\"{}\"}} {}",
                label, stats.approx_ram_bytes
            ));
        }

        lines.push(
            "# HELP lithair_model_raftlog_bytes Size of events.raftlog on disk per model"
                .to_string(),
        );
        lines.push("# TYPE lithair_model_raftlog_bytes gauge".to_string());
        for (label, stats) in &per_model {
            lines.push(format!(
                "lithair_model_raftlog_bytes{{model=\"{}\"}} {}",
                label, stats.raftlog_size_bytes
            ));
        }

        lines.push(String::new());

        Ok(hyper::Response::builder()
            .status(200)
            .header("Content-Type", "text/plain; version=0.0.4")
            .body(Full::new(Bytes::from(lines.join("\n"))))
            .expect("valid HTTP response"))
    }

    /// Handle data admin API requests (/_admin/data/*)
    ///
    /// Endpoints:
    /// - GET /_admin/data/models - List all registered models with stats
    /// - GET /_admin/data/models/{name} - Get model info and data
    /// - GET /_admin/data/models/{name}/export - Export model data as JSON
    /// - GET /_admin/data/routes - List all registered API routes
    /// - POST /_admin/data/backup - Trigger full data backup
    async fn handle_data_admin_request(
        &self,
        _req: hyper::Request<hyper::body::Incoming>,
        path: &str,
        method: &hyper::Method,
    ) -> Result<hyper::Response<http_body_util::Full<bytes::Bytes>>> {
        use bytes::Bytes;
        use http_body_util::Full;

        // Parse the path: /_admin/data/{resource}[/{name}][/{action}]
        let path_parts: Vec<&str> = path
            .strip_prefix("/_admin/data/")
            .unwrap_or("")
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        match (method, path_parts.as_slice()) {
            // GET /_admin/data/models - List all models
            (&hyper::Method::GET, ["models"]) => {
                let models = self.models.read().await;
                let mut model_list = Vec::new();

                for model in models.iter() {
                    let count = model.handler.get_count().await;
                    model_list.push(serde_json::json!({
                        "name": model.name,
                        "base_path": model.base_path,
                        "data_path": model.data_path,
                        "count": count
                    }));
                }

                let response = serde_json::json!({
                    "models": model_list,
                    "total_models": models.len()
                });

                Ok(hyper::Response::builder()
                    .status(200)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(
                        serde_json::to_string_pretty(&response).expect("serializable response"),
                    )))
                    .expect("valid HTTP response"))
            }

            // GET /_admin/data/models/{name} - Get model data
            (&hyper::Method::GET, ["models", name]) => {
                let models = self.models.read().await;

                if let Some(model) = models.iter().find(|m| m.name == *name) {
                    let data = model.handler.get_all_data_json().await;
                    let count = model.handler.get_count().await;

                    let response = serde_json::json!({
                        "model": model.name,
                        "base_path": model.base_path,
                        "count": count,
                        "data": data
                    });

                    Ok(hyper::Response::builder()
                        .status(200)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(
                            serde_json::to_string_pretty(&response).expect("serializable response"),
                        )))
                        .expect("valid HTTP response"))
                } else {
                    Ok(hyper::Response::builder()
                        .status(404)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(format!(
                            r#"{{"error":"Model '{}' not found"}}"#,
                            name
                        ))))
                        .expect("valid HTTP response"))
                }
            }

            // GET /_admin/data/models/{name}/_stats - Per-model storage stats (issue #72)
            (&hyper::Method::GET, ["models", name, "_stats"]) => {
                // Resolve the model under the read lock, snapshot the handler
                // + data_path, then drop the lock before awaiting get_stats.
                // Same rationale as handle_metrics_request: stats sampling
                // must not block writers (Gemini PR #83 review).
                let resolved: Option<(String, Arc<dyn crate::app::ModelHandler>)> = {
                    let models = self.models.read().await;
                    models
                        .iter()
                        .find(|m| m.name == *name)
                        .map(|m| (m.data_path.clone(), Arc::clone(&m.handler)))
                };

                if let Some((data_path, handler)) = resolved {
                    let stats = handler.get_stats(&data_path).await;

                    Ok(hyper::Response::builder()
                        .status(200)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(
                            serde_json::to_string_pretty(&stats).expect("serializable response"),
                        )))
                        .expect("valid HTTP response"))
                } else {
                    // Build the JSON body via serde_json so `name` is properly
                    // escaped — a naked `format!` would let a model name like
                    // `x", "y":"z` break out of the error string and produce
                    // malformed (or worse, attacker-shaped) JSON. See Gemini
                    // review on PR #83.
                    Ok(hyper::Response::builder()
                        .status(404)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(
                            serde_json::to_string(&serde_json::json!({
                                "error": format!("Model '{}' not found", name)
                            }))
                            .expect("error response is serializable"),
                        )))
                        .expect("valid HTTP response"))
                }
            }

            // GET /_admin/data/models/{name}/export - Export model data
            (&hyper::Method::GET, ["models", name, "export"]) => {
                let models = self.models.read().await;

                if let Some(model) = models.iter().find(|m| m.name == *name) {
                    let export = model.handler.export_json().await;

                    Ok(hyper::Response::builder()
                        .status(200)
                        .header("Content-Type", "application/json")
                        .header(
                            "Content-Disposition",
                            format!("attachment; filename=\"{}_export.json\"", name),
                        )
                        .body(Full::new(Bytes::from(
                            serde_json::to_string_pretty(&export).expect("serializable response"),
                        )))
                        .expect("valid HTTP response"))
                } else {
                    Ok(hyper::Response::builder()
                        .status(404)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(format!(
                            r#"{{"error":"Model '{}' not found"}}"#,
                            name
                        ))))
                        .expect("valid HTTP response"))
                }
            }

            // GET /_admin/data/models/{name}/{id}/history - Get entity event history
            (&hyper::Method::GET, ["models", name, id, "history"]) => {
                let models = self.models.read().await;

                if let Some(model) = models.iter().find(|m| m.name == *name) {
                    let history = model.handler.get_entity_history(id).await;

                    Ok(hyper::Response::builder()
                        .status(200)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(
                            serde_json::to_string_pretty(&history).expect("serializable response"),
                        )))
                        .expect("valid HTTP response"))
                } else {
                    Ok(hyper::Response::builder()
                        .status(404)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(format!(
                            r#"{{"error":"Model '{}' not found"}}"#,
                            name
                        ))))
                        .expect("valid HTTP response"))
                }
            }

            // POST /_admin/data/models/{name}/{id}/edit - Submit edit event (event-sourced)
            (&hyper::Method::POST, ["models", name, id, "edit"]) => {
                use http_body_util::BodyExt;

                let models = self.models.read().await;

                if let Some(model) = models.iter().find(|m| m.name == *name) {
                    // Parse request body
                    let body_bytes = match _req.into_body().collect().await.map(|c| c.to_bytes()) {
                        Ok(bytes) => bytes,
                        Err(_) => {
                            return Ok(hyper::Response::builder()
                                .status(400)
                                .header("Content-Type", "application/json")
                                .body(Full::new(Bytes::from(r#"{"error":"Invalid request body"}"#)))
                                .expect("valid HTTP response"));
                        }
                    };

                    let changes: serde_json::Value = match serde_json::from_slice(&body_bytes) {
                        Ok(v) => v,
                        Err(_) => {
                            return Ok(hyper::Response::builder()
                                .status(400)
                                .header("Content-Type", "application/json")
                                .body(Full::new(Bytes::from(r#"{"error":"Invalid JSON"}"#)))
                                .expect("valid HTTP response"));
                        }
                    };

                    match model.handler.submit_edit_event(id, changes).await {
                        Ok(updated) => {
                            let response = serde_json::json!({
                                "success": true,
                                "message": "Edit event submitted successfully",
                                "entity_id": id,
                                "model": name,
                                "updated_data": updated
                            });

                            Ok(hyper::Response::builder()
                                .status(200)
                                .header("Content-Type", "application/json")
                                .body(Full::new(Bytes::from(
                                    serde_json::to_string_pretty(&response)
                                        .expect("serializable response"),
                                )))
                                .expect("valid HTTP response"))
                        }
                        Err(e) => Ok(hyper::Response::builder()
                            .status(400)
                            .header("Content-Type", "application/json")
                            .body(Full::new(Bytes::from(
                                serde_json::json!({"error": e.to_string()}).to_string(),
                            )))
                            .expect("valid HTTP response")),
                    }
                } else {
                    Ok(hyper::Response::builder()
                        .status(404)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(format!(
                            r#"{{"error":"Model '{}' not found"}}"#,
                            name
                        ))))
                        .expect("valid HTTP response"))
                }
            }

            // GET /_admin/data/routes - List all routes
            (&hyper::Method::GET, ["routes"]) => {
                let models = self.models.read().await;
                let mut routes = Vec::new();

                // Model routes
                for model in models.iter() {
                    routes.push(serde_json::json!({
                        "method": "GET",
                        "path": model.base_path.clone(),
                        "type": "model",
                        "model": model.name
                    }));
                    routes.push(serde_json::json!({
                        "method": "POST",
                        "path": model.base_path.clone(),
                        "type": "model",
                        "model": model.name
                    }));
                    routes.push(serde_json::json!({
                        "method": "GET",
                        "path": format!("{}/:id", model.base_path),
                        "type": "model",
                        "model": model.name
                    }));
                    routes.push(serde_json::json!({
                        "method": "PUT",
                        "path": format!("{}/:id", model.base_path),
                        "type": "model",
                        "model": model.name
                    }));
                    routes.push(serde_json::json!({
                        "method": "DELETE",
                        "path": format!("{}/:id", model.base_path),
                        "type": "model",
                        "model": model.name
                    }));
                }
                drop(models);

                // Custom routes
                for route in &self.custom_routes {
                    routes.push(serde_json::json!({
                        "method": route.method.to_string(),
                        "path": route.path,
                        "type": "custom"
                    }));
                }

                // Admin routes
                if self.config.admin.data_admin_enabled {
                    routes.push(serde_json::json!({
                        "method": "GET",
                        "path": "/_admin/data/models",
                        "type": "admin"
                    }));
                    routes.push(serde_json::json!({
                        "method": "GET",
                        "path": "/_admin/data/models/:name",
                        "type": "admin"
                    }));
                    routes.push(serde_json::json!({
                        "method": "GET",
                        "path": "/_admin/data/models/:name/export",
                        "type": "admin"
                    }));
                    routes.push(serde_json::json!({
                        "method": "GET",
                        "path": "/_admin/data/routes",
                        "type": "admin"
                    }));
                    routes.push(serde_json::json!({
                        "method": "POST",
                        "path": "/_admin/data/backup",
                        "type": "admin"
                    }));
                }

                let response = serde_json::json!({
                    "routes": routes,
                    "total_routes": routes.len()
                });

                Ok(hyper::Response::builder()
                    .status(200)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(
                        serde_json::to_string_pretty(&response).expect("serializable response"),
                    )))
                    .expect("valid HTTP response"))
            }

            // POST /_admin/data/backup - Backup all models
            (&hyper::Method::POST, ["backup"]) => {
                let models = self.models.read().await;
                let mut backup_data = Vec::new();

                for model in models.iter() {
                    let export = model.handler.export_json().await;
                    backup_data.push(export);
                }

                let backup = serde_json::json!({
                    "backup_type": "full",
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                    "model_count": models.len(),
                    "models": backup_data
                });

                Ok(hyper::Response::builder()
                    .status(200)
                    .header("Content-Type", "application/json")
                    .header("Content-Disposition", "attachment; filename=\"lithair_backup.json\"")
                    .body(Full::new(Bytes::from(
                        serde_json::to_string_pretty(&backup).expect("serializable response"),
                    )))
                    .expect("valid HTTP response"))
            }

            // 404 for unknown data admin paths
            _ => Ok(hyper::Response::builder()
                .status(404)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(r#"{"error":"Unknown data admin endpoint"}"#)))
                .expect("valid HTTP response")),
        }
    }

    /// Handle embedded data admin UI request (serves the dashboard HTML)
    /// Only available when the `admin-ui` feature is enabled
    #[cfg(feature = "admin-ui")]
    async fn handle_data_admin_ui_request(
        &self,
    ) -> Result<hyper::Response<http_body_util::Full<bytes::Bytes>>> {
        use bytes::Bytes;
        use http_body_util::Full;

        Ok(hyper::Response::builder()
            .status(200)
            .header("Content-Type", "text/html; charset=utf-8")
            .header("Cache-Control", "no-cache")
            .body(Full::new(Bytes::from(crate::admin_ui::DASHBOARD_HTML)))
            .expect("valid HTTP response"))
    }

    /// Handle model request
    ///
    /// In cluster mode, write operations go through the Raft consensus log:
    /// 1. Leader appends operation to log
    /// 2. Leader replicates to followers (synchronous, waits for majority)
    /// 3. After majority acknowledgment, operation is committed
    /// 4. All nodes (including leader) apply committed entries in order
    ///
    /// In single-node mode, operations are applied directly without logging.
    async fn handle_model_request(
        &self,
        req: hyper::Request<hyper::body::Incoming>,
        model: &ModelRegistration,
    ) -> Result<hyper::Response<http_body_util::Full<bytes::Bytes>>> {
        use http_body_util::Full;

        // Extract path segments after base_path (clone path first to avoid borrow issues)
        let path = req.uri().path().to_string();
        let method = req.method().clone();
        let segments: Vec<&str> = path
            .strip_prefix(&model.base_path)
            .unwrap_or("")
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        // Check if this is a write operation
        let is_create = method == hyper::Method::POST && segments.is_empty();
        let is_bulk_create = method == hyper::Method::POST && segments.first() == Some(&"_bulk");
        let is_update = (method == hyper::Method::PUT || method == hyper::Method::PATCH)
            && !segments.is_empty();
        let is_delete = method == hyper::Method::DELETE && !segments.is_empty();
        let is_write = is_create || is_bulk_create || is_update || is_delete;

        // Extract the resource ID for UPDATE and DELETE operations
        let resource_id =
            if is_update || is_delete { segments.first().map(|s| s.to_string()) } else { None };

        // ==================== CLUSTER MODE WITH CONSENSUS LOG ====================
        // If we have a consensus log (cluster mode), write operations go through Raft
        if is_write && !self.cluster_peers.is_empty() {
            if let Some(ref consensus_log_ref) = self.consensus_log {
                log::debug!(
                    "CLUSTER MODE: {} {} (create={}, update={}, delete={})",
                    method,
                    path,
                    is_create,
                    is_update,
                    is_delete
                );

                // Check if we are the leader
                let is_leader = self.raft_state.as_ref().map(|s| s.is_leader()).unwrap_or(false);

                if !is_leader {
                    // Redirect to leader (if we know the leader's port)
                    if let Some(ref raft_state) = self.raft_state {
                        let leader_port = raft_state.get_leader_port();
                        if leader_port == 0 {
                            // Leader port not yet discovered (haven't received first heartbeat).
                            // Return 503 instead of redirecting to port 0.
                            return Ok(Self::leader_port_unknown_503());
                        }
                        return Ok(hyper::Response::builder()
                        .status(307) // Temporary Redirect
                        .header("Location", format!("http://127.0.0.1:{}{}", leader_port, path))
                        .header("X-Raft-Leader", format!("{}", leader_port))
                        .body(Full::new(Bytes::from(serde_json::json!({"error": "Not leader", "leader_port": leader_port}).to_string())))
                        .expect("valid HTTP response"));
                    }
                }

                // We are the leader - process through consensus log
                let consensus_log = consensus_log_ref;

                // Read request body for write operations
                use http_body_util::BodyExt;
                let (_parts, body) = req.into_parts();
                let body_bytes = body.collect().await?.to_bytes();
                let body_json: serde_json::Value =
                    serde_json::from_slice(&body_bytes).unwrap_or(serde_json::Value::Null);

                // Create the CRUD operation
                // For CREATE operations: generate ID on leader to ensure all nodes have same ID
                // Also inject timestamps to ensure consistency across all nodes
                let now = chrono::Utc::now().to_rfc3339();
                let operation = if is_create {
                    let mut data = body_json.clone();
                    // Generate ID on leader if not provided, so followers get the same ID
                    if data.get("id").is_none() || data.get("id") == Some(&serde_json::Value::Null)
                    {
                        data["id"] = serde_json::Value::String(uuid::Uuid::new_v4().to_string());
                    }
                    // Add timestamps for consistency across all nodes
                    if data.get("created_at").is_none() {
                        data["created_at"] = serde_json::Value::String(now.clone());
                    }
                    if data.get("updated_at").is_none() {
                        data["updated_at"] = serde_json::Value::String(now.clone());
                    }
                    crate::cluster::CrudOperation::Create {
                        model_path: model.base_path.clone(),
                        data,
                    }
                } else if is_update {
                    let id = resource_id.clone().unwrap_or_default();
                    log::info!("CLUSTER: Creating UPDATE operation for id={}", id);
                    // For UPDATE: merge delta with existing item to send complete object
                    // This ensures followers can deserialize the full item
                    let existing = model.handler.get_item_json(&id).await;
                    let mut merged_data = if let Some(mut existing_json) = existing {
                        // Merge delta into existing (delta overwrites existing fields)
                        if let Some(obj) = existing_json.as_object_mut() {
                            if let Some(delta_obj) = body_json.as_object() {
                                for (key, value) in delta_obj {
                                    obj.insert(key.clone(), value.clone());
                                }
                            }
                        }
                        existing_json
                    } else {
                        // Item doesn't exist - use delta as-is (will likely fail on follower too)
                        body_json.clone()
                    };
                    // Always update the updated_at timestamp for consistency
                    merged_data["updated_at"] = serde_json::Value::String(now);
                    crate::cluster::CrudOperation::Update {
                        model_path: model.base_path.clone(),
                        id,
                        data: merged_data,
                    }
                } else if is_delete {
                    let id = resource_id.clone().unwrap_or_default();
                    log::info!("CLUSTER: Creating DELETE operation for id={}", id);
                    crate::cluster::CrudOperation::Delete {
                        model_path: model.base_path.clone(),
                        id,
                    }
                } else {
                    // Bulk create - currently handled as a single operation
                    // Note: Proper BatchOperation support is not yet available
                    let mut data = body_json.clone();
                    if data.get("id").is_none() || data.get("id") == Some(&serde_json::Value::Null)
                    {
                        data["id"] = serde_json::Value::String(uuid::Uuid::new_v4().to_string());
                    }
                    // Add timestamps for consistency across all nodes
                    if data.get("created_at").is_none() {
                        data["created_at"] = serde_json::Value::String(now.clone());
                    }
                    if data.get("updated_at").is_none() {
                        data["updated_at"] = serde_json::Value::String(now);
                    }
                    crate::cluster::CrudOperation::Create {
                        model_path: model.base_path.clone(),
                        data,
                    }
                };

                // ── WRITE PATH: CONSENSUS + REPLICATION ────────────────────
                //
                // The write path for a cluster leader follows this sequence:
                //
                //   1. Append to ConsensusLog (in-memory, atomic index assignment)
                //   2. Queue for ReplicationBatcher (follower health tracking)
                //   3. In parallel:
                //      a. Write to WAL (disk durability, group commit batches fsync)
                //      b. Send this single entry to all followers via HTTP
                //   4. Wait for majority acknowledgment (quorum = peers/2 + 1)
                //   5. Commit: update commit_index in the ConsensusLog
                //   6. Apply: execute the operation on the local state machine
                //   7. Fire-and-forget: notify remaining followers of new commit_index
                //
                // The background catch-up task (100ms interval) handles lagging
                // followers by sending only their missing entries using per-follower
                // match_index tracking.

                // Step 1: Append to local consensus log (in-memory, fast)
                let log_entry = consensus_log.append(operation.clone()).await;
                let entry_index = log_entry.log_id.index;
                let term = consensus_log.current_term();
                let node_id = self.node_id.unwrap_or(0);
                log::debug!("Appended to log: index={}, term={}", entry_index, term);

                // Step 2: Queue for batcher (for lagging followers tracking)
                if let Some(ref batcher) = self.replication_batcher {
                    batcher.queue_entry(log_entry.clone()).await;
                }

                // Step 3: PARALLEL - WAL durability + Replication to followers
                // We use tokio::join! to run both concurrently and wait for both to complete.
                // This reduces latency since WAL fsync and network I/O happen simultaneously.
                let wal_clone = self.wal.clone();
                let log_entry_clone = log_entry.clone();
                let peers_clone = self.cluster_peers.clone();
                let port_clone = self.config.server.port;
                let batcher_clone = self.replication_batcher.clone();

                // WAL write task (uses group commit for batching)
                let wal_future = async {
                    if let Some(ref wal) = wal_clone {
                        // Use buffered append for group commit (higher throughput)
                        wal.append_buffered(&log_entry_clone).await
                    } else {
                        Ok(())
                    }
                };

                // Replication task (returns when majority responds)
                // Send only the new entry to followers. The background catch-up task
                // handles lagging followers using per-follower match_index tracking.
                let replication_future = async move {
                    let entries_to_send = vec![log_entry.clone()];

                    if entries_to_send.is_empty() {
                        return Ok(entry_index);
                    }

                    log::debug!(
                        "Replicating {} entries (window {} to {}), target_commit={}",
                        entries_to_send.len(),
                        entries_to_send.first().map(|e| e.log_id.index).unwrap_or(0),
                        entries_to_send.last().map(|e| e.log_id.index).unwrap_or(0),
                        entry_index
                    );

                    Self::replicate_log_entries_to_followers(
                        &peers_clone,
                        entries_to_send,
                        entry_index, // Commit up to this entry if majority responds
                        term,
                        node_id,
                        port_clone,
                        batcher_clone,
                    )
                    .await
                };

                // Run WAL and replication in parallel
                let (wal_result, replication_result) = tokio::join!(wal_future, replication_future);

                // Check WAL result first (must succeed for durability)
                if let Err(e) = wal_result {
                    log::error!("WAL write failed: {}", e);
                    return Ok(hyper::Response::builder()
                        .status(503)
                        .body(Full::new(Bytes::from(format!(
                            r#"{{"error":"WAL write failed: {}"}}"#,
                            e
                        ))))
                        .expect("valid HTTP response"));
                }
                log::debug!("WAL entry durable: index={}", entry_index);

                // Check replication result
                match replication_result {
                    Ok(new_commit_index) => {
                        // Step 4: Commit the entry (majority achieved)
                        consensus_log.commit(new_commit_index);
                        log::debug!("Committed index: {}", new_commit_index);

                        // Step 4.5: Send commit notification to followers IN PARALLEL (fire-and-forget)
                        // Include the window of entries so followers get both data and commit in one shot
                        let peers_for_notify = self.cluster_peers.clone();
                        let commit_index_to_notify = new_commit_index;
                        let term_for_notify = term;
                        let node_id_for_notify = node_id;
                        let port_for_notify = self.config.server.port;
                        let notify_prev_log_index = entry_index;
                        let notify_prev_log_term = term;
                        tokio::spawn(async move {
                            // Send commit notification with leader's log position
                            // so followers can detect divergence.
                            let client = reqwest::Client::builder()
                                .timeout(std::time::Duration::from_secs(1))
                                .build()
                                .ok();
                            if let Some(client) = client {
                                let request = crate::cluster::consensus_log::AppendEntriesRequest {
                                    term: term_for_notify,
                                    leader_id: node_id_for_notify,
                                    leader_port: port_for_notify,
                                    prev_log_index: notify_prev_log_index,
                                    prev_log_term: notify_prev_log_term,
                                    entries: vec![], // Commit notification only, catch-up via background task
                                    leader_commit: commit_index_to_notify,
                                };
                                // Send to ALL peers IN PARALLEL
                                let futures: Vec<_> = peers_for_notify
                                    .iter()
                                    .map(|peer| {
                                        let endpoint = format!("http://{}/_raft/append", peer);
                                        let client = client.clone();
                                        let request = request.clone();
                                        async move {
                                            let _ =
                                                client.post(&endpoint).json(&request).send().await;
                                        }
                                    })
                                    .collect();
                                futures::future::join_all(futures).await;
                            }
                        });

                        // Step 5: Apply to local state machine
                        // CRITICAL: Wait for all earlier entries to be applied first.
                        // Without this, entries can be applied out of order when commits happen
                        // out of order, causing data inconsistency (e.g., DELETE before CREATE).
                        //
                        // Example race without this fix:
                        // 1. Entry 100 (CREATE X) appended, replication starts
                        // 2. Entry 101 (DELETE X) appended, replication starts
                        // 3. Entry 101 replication completes, commits 101
                        // 4. Entry 101 applies (DELETE X - but X doesn't exist yet!)
                        // 5. Entry 100 replication completes, commits 100
                        // 6. Entry 100 applies (CREATE X - X now exists!)
                        // Result: Leader has X, but followers applied in correct order (no X)

                        // Wait for earlier entries to be COMMITTED first
                        // This handles the case where entry N+1 commits before entry N
                        // (due to faster replication). We must wait for N to commit before applying N+1.
                        let expected_prior = entry_index.saturating_sub(1);
                        let mut commit_waited = 0u32;
                        while consensus_log.commit_index() < expected_prior {
                            if commit_waited > 50000 {
                                // 50000 * 100µs = 5 seconds max wait for commit
                                log::error!(
                                    "Waited 5s for earlier entry {} to commit (current commit={})",
                                    expected_prior,
                                    consensus_log.commit_index()
                                );
                                // Return error - something is seriously wrong if commit takes this long
                                return Ok(hyper::Response::builder()
                                .status(503)
                                .body(Full::new(Bytes::from(format!(
                                    r#"{{"error":"Commit ordering timeout: entry {} waiting for {}"}}"#,
                                    entry_index, expected_prior
                                ))))
                                .expect("valid HTTP response"));
                            }
                            tokio::time::sleep(std::time::Duration::from_micros(100)).await;
                            commit_waited += 1;
                        }

                        // Now wait for earlier entry to be APPLIED
                        // Once it's committed, its handler will apply it (no timeout - it WILL apply)
                        let mut apply_waited = 0u32;
                        while consensus_log.applied_index() < expected_prior {
                            if apply_waited > 100000 {
                                // 100000 * 100µs = 10 seconds max wait for apply
                                // This should never happen if commit succeeded - log but continue waiting
                                log::warn!(
                                    "Slow apply: entry {} waiting for {} (commit={}, applied={})",
                                    entry_index,
                                    expected_prior,
                                    consensus_log.commit_index(),
                                    consensus_log.applied_index()
                                );
                                apply_waited = 0; // Reset counter to keep waiting
                            }
                            tokio::time::sleep(std::time::Duration::from_micros(100)).await;
                            apply_waited += 1;
                        }

                        // Now safe to acquire lock and apply
                        let _apply_guard = consensus_log.lock_apply().await;

                        // Now apply our entry
                        match self.apply_crud_operation(&operation).await {
                            Ok(result) => {
                                consensus_log.mark_applied(entry_index);
                                // _apply_guard dropped here
                                let response_body = serde_json::to_vec(&result).unwrap_or_default();
                                return Ok(hyper::Response::builder()
                                    .status(if is_create { 201 } else { 200 })
                                    .header("Content-Type", "application/json")
                                    .header("X-Raft-Index", entry_index.to_string())
                                    .body(Full::new(Bytes::from(response_body)))
                                    .expect("valid HTTP response"));
                            }
                            Err(e) => {
                                log::error!("Failed to apply operation: {}", e);
                                return Ok(hyper::Response::builder()
                                    .status(500)
                                    .body(Full::new(Bytes::from(format!(
                                        r#"{{"error":"Apply failed: {}"}}"#,
                                        e
                                    ))))
                                    .expect("valid HTTP response"));
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to replicate: {}", e);
                        return Ok(hyper::Response::builder()
                        .status(503) // Service Unavailable
                        .body(Full::new(Bytes::from(serde_json::json!({"error": format!("Replication failed: {}", e)}).to_string())))
                        .expect("valid HTTP response"));
                    }
                }
            }
        }

        // ==================== SINGLE-NODE MODE OR READ OPERATIONS ====================
        // No cluster or read operation - delegate directly to model handler
        match model.handler.handle_request(req, &segments).await {
            Ok(resp) => {
                use http_body_util::BodyExt;

                let (parts, body) = resp.into_parts();
                let body_bytes = body.collect().await?.to_bytes();
                Ok(hyper::Response::from_parts(parts, Full::new(body_bytes)))
            }
            Err(_) => Ok(hyper::Response::builder()
                .status(500)
                .body(Full::new(Bytes::from(r#"{"error":"Internal error"}"#)))
                .expect("valid HTTP response")),
        }
    }

    // NOTE: The old fire-and-forget replication methods (replicate_to_followers,
    // replicate_update_to_followers, replicate_delete_to_followers) have been removed.
    // They were replaced by the Raft consensus log approach which guarantees ordering.
    // See: replicate_log_entries_to_followers() and handle_raft_append_entries()

    /// Apply a CRUD operation from the consensus log to the appropriate model
    /// This is called when a log entry is committed and needs to be applied to the state machine
    pub async fn apply_crud_operation(
        &self,
        operation: &crate::cluster::CrudOperation,
    ) -> Result<serde_json::Value, String> {
        use crate::cluster::CrudOperation;

        let models = self.models.read().await;

        match operation {
            CrudOperation::Create { model_path, data } => {
                // Find the model by base_path
                let model = models
                    .iter()
                    .find(|m| model_path.starts_with(&m.base_path))
                    .ok_or_else(|| format!("Model not found for path: {}", model_path))?;

                model.handler.apply_replicated_item_json(data.clone()).await?;
                Ok(data.clone())
            }
            CrudOperation::Update { model_path, id, data } => {
                let model = models
                    .iter()
                    .find(|m| model_path.starts_with(&m.base_path))
                    .ok_or_else(|| format!("Model not found for path: {}", model_path))?;

                model.handler.apply_replicated_update_json(id, data.clone()).await?;
                Ok(data.clone())
            }
            CrudOperation::Delete { model_path, id } => {
                let model = models
                    .iter()
                    .find(|m| model_path.starts_with(&m.base_path))
                    .ok_or_else(|| format!("Model not found for path: {}", model_path))?;

                model.handler.apply_replicated_delete_json(id).await?;
                Ok(serde_json::json!({"deleted": id}))
            }
            // === Migration Operations (Phase 2: Full implementation) ===
            CrudOperation::MigrationBegin { from_version, to_version, migration_id } => {
                log::info!(
                    "MIGRATION_BEGIN: {} -> {} (id: {})",
                    from_version,
                    to_version,
                    migration_id
                );

                // Use migration manager if available
                if let Some(ref manager) = self.migration_manager {
                    match manager
                        .begin_migration(*migration_id, from_version.clone(), to_version.clone())
                        .await
                    {
                        Ok(()) => {
                            log::info!("Migration {} registered successfully", migration_id);
                            Ok(serde_json::json!({
                                "status": "started",
                                "migration_id": migration_id.to_string(),
                                "from": from_version.to_string(),
                                "to": to_version.to_string(),
                            }))
                        }
                        Err(e) => {
                            log::error!("Failed to begin migration {}: {}", migration_id, e);
                            Err(e)
                        }
                    }
                } else {
                    // No migration manager - acknowledge but warn
                    log::warn!("Migration manager not available, migration {} acknowledged but not tracked", migration_id);
                    Ok(serde_json::json!({
                        "status": "acknowledged",
                        "warning": "No migration manager available",
                        "migration_id": migration_id.to_string(),
                        "from": from_version.to_string(),
                        "to": to_version.to_string(),
                    }))
                }
            }
            CrudOperation::MigrationStep { migration_id, step_index, operation } => {
                log::info!(
                    "MIGRATION_STEP: migration={}, step={}, operation={:?}",
                    migration_id,
                    step_index,
                    operation
                );

                // Apply the schema change and record rollback
                let result = self.apply_schema_change(operation).await;

                match result {
                    Ok(rollback_op) => {
                        // Record step in migration manager
                        if let Some(ref manager) = self.migration_manager {
                            if let Err(e) = manager
                                .record_step(migration_id, *step_index, rollback_op, None)
                                .await
                            {
                                log::warn!("Failed to record migration step: {}", e);
                            }
                        }
                        Ok(serde_json::json!({
                            "status": "applied",
                            "migration_id": migration_id.to_string(),
                            "step_index": step_index,
                        }))
                    }
                    Err(e) => {
                        log::error!("Migration step {} failed: {}", step_index, e);
                        Err(format!("Migration step {} failed: {}", step_index, e))
                    }
                }
            }
            CrudOperation::MigrationCommit { migration_id, checksum } => {
                log::info!("MIGRATION_COMMIT: migration={}, checksum={}", migration_id, checksum);

                if let Some(ref manager) = self.migration_manager {
                    // Get migration context to get the target version
                    if let Some(ctx) = manager.get_migration(migration_id).await {
                        let new_version = ctx.to_version.clone();
                        match manager.commit_migration(migration_id, new_version).await {
                            Ok(()) => {
                                log::info!(
                                    "Migration {} committed, checksum verified: {}",
                                    migration_id,
                                    checksum
                                );
                                Ok(serde_json::json!({
                                    "status": "committed",
                                    "migration_id": migration_id.to_string(),
                                    "checksum": checksum,
                                }))
                            }
                            Err(e) => {
                                log::error!("Failed to commit migration {}: {}", migration_id, e);
                                Err(e)
                            }
                        }
                    } else {
                        let msg = format!("Migration {} not found", migration_id);
                        log::error!("{}", msg);
                        Err(msg)
                    }
                } else {
                    Ok(serde_json::json!({
                        "status": "acknowledged",
                        "warning": "No migration manager available",
                        "migration_id": migration_id.to_string(),
                        "checksum": checksum,
                    }))
                }
            }
            CrudOperation::MigrationRollback { migration_id, failed_step, reason } => {
                log::warn!(
                    "MIGRATION_ROLLBACK: migration={}, failed_step={}, reason={}",
                    migration_id,
                    failed_step,
                    reason
                );

                if let Some(ref manager) = self.migration_manager {
                    match manager.rollback_migration(migration_id).await {
                        Ok(rollback_ops) => {
                            // Apply rollback operations in reverse order
                            let mut rollback_errors = Vec::new();
                            for op in rollback_ops {
                                log::info!("Rolling back step {}", op.step_index);
                                if let Err(e) = self.apply_schema_change(&op.operation).await {
                                    log::error!("Rollback step {} failed: {}", op.step_index, e);
                                    rollback_errors.push(format!("Step {}: {}", op.step_index, e));
                                }
                            }

                            if rollback_errors.is_empty() {
                                log::info!("Migration {} fully rolled back", migration_id);
                                Ok(serde_json::json!({
                                    "status": "rolled_back",
                                    "migration_id": migration_id.to_string(),
                                    "failed_step": failed_step,
                                    "reason": reason,
                                }))
                            } else {
                                log::error!(
                                    "Migration {} partially rolled back with errors",
                                    migration_id
                                );
                                Ok(serde_json::json!({
                                    "status": "partial_rollback",
                                    "migration_id": migration_id.to_string(),
                                    "failed_step": failed_step,
                                    "reason": reason,
                                    "rollback_errors": rollback_errors,
                                }))
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to rollback migration {}: {}", migration_id, e);
                            Err(e)
                        }
                    }
                } else {
                    Ok(serde_json::json!({
                        "status": "acknowledged",
                        "warning": "No migration manager available",
                        "migration_id": migration_id.to_string(),
                        "failed_step": failed_step,
                        "reason": reason,
                    }))
                }
            }
        }
    }

    /// Apply a schema change and return the inverse operation for rollback
    ///
    /// This method applies schema changes from migrations and returns the inverse
    /// SchemaChange that would undo this operation.
    async fn apply_schema_change(
        &self,
        change: &crate::cluster::SchemaChange,
    ) -> Result<crate::cluster::SchemaChange, String> {
        use crate::cluster::SchemaChange;

        match change {
            SchemaChange::AddModel { name, schema: _ } => {
                log::info!("Applying AddModel: {}", name);
                // Phase 3 planned feature: register model in runtime schema registry
                // For now, log and return the inverse operation
                Ok(SchemaChange::RemoveModel { name: name.clone(), backup_path: None })
            }
            SchemaChange::RemoveModel { name, backup_path } => {
                log::info!("Applying RemoveModel: {} (backup: {:?})", name, backup_path);
                // Phase 3 planned feature: remove model from registry, backup data if path provided
                // For rollback, we need the original schema - this would be stored in migration context
                Ok(SchemaChange::Custom {
                    description: format!("Restore model: {}", name),
                    forward: format!("restore_model:{}", name),
                    backward: format!("remove_model:{}", name),
                })
            }
            SchemaChange::AddField { model, field, default_value: _ } => {
                log::info!("Applying AddField: {}.{}", model, field.name);
                // Phase 3 planned feature: add field to model schema
                Ok(SchemaChange::RemoveField { model: model.clone(), field: field.name.clone() })
            }
            SchemaChange::RemoveField { model, field } => {
                log::info!("Applying RemoveField: {}.{}", model, field);
                // Phase 3 planned feature: remove field from model, backup data
                // For rollback, we need the original field definition
                Ok(SchemaChange::Custom {
                    description: format!("Restore field: {}.{}", model, field),
                    forward: format!("restore_field:{}:{}", model, field),
                    backward: format!("remove_field:{}:{}", model, field),
                })
            }
            SchemaChange::RenameField { model, old_name, new_name } => {
                log::info!("Applying RenameField: {}.{} -> {}", model, old_name, new_name);
                // Phase 3 planned feature: update field name in schema and all data
                // Inverse is simply swapping old and new names
                Ok(SchemaChange::RenameField {
                    model: model.clone(),
                    old_name: new_name.clone(),
                    new_name: old_name.clone(),
                })
            }
            SchemaChange::ChangeFieldType { model, field, new_type, transform: _ } => {
                log::info!("Applying ChangeFieldType: {}.{} -> {:?}", model, field, new_type);
                // Phase 3 planned feature: transform field data to new type
                // For rollback, we need the original type - stored in migration context
                Ok(SchemaChange::Custom {
                    description: format!("Restore field type: {}.{}", model, field),
                    forward: format!("restore_type:{}:{}", model, field),
                    backward: format!("change_type:{}:{}:{:?}", model, field, new_type),
                })
            }
            SchemaChange::AddIndex { model, fields, unique } => {
                log::info!("Applying AddIndex: {}.{:?} (unique: {})", model, fields, unique);
                // Phase 3 planned feature: create index on model fields
                // Inverse: remove the index
                Ok(SchemaChange::Custom {
                    description: format!("Remove index on {}.{:?}", model, fields),
                    forward: format!("drop_index:{}:{}", model, fields.join(",")),
                    backward: format!("create_index:{}:{}:{}", model, fields.join(","), unique),
                })
            }
            SchemaChange::Custom { description, forward, backward } => {
                log::info!("Applying Custom: {}", description);
                // Custom operations define their own forward/backward
                // Inverse swaps forward and backward
                Ok(SchemaChange::Custom {
                    description: format!("Undo: {}", description),
                    forward: backward.clone(),
                    backward: forward.clone(),
                })
            }
        }
    }

    /// Create a snapshot from current model state
    ///
    /// This creates a fresh snapshot of all model data for sending to desynced followers.
    async fn create_snapshot_from_models(
        models: &Arc<tokio::sync::RwLock<Vec<ModelRegistration>>>,
        snapshot_manager: &Arc<tokio::sync::RwLock<crate::cluster::snapshot::SnapshotManager>>,
        term: u64,
        last_index: u64,
    ) -> Result<crate::cluster::snapshot::SnapshotMeta, String> {
        let mut snapshot_data = crate::cluster::snapshot::SnapshotData::new();

        // Collect all model data
        let models_read = models.read().await;
        for model in models_read.iter() {
            let data_json = model.handler.get_all_data_json().await;
            // get_all_data_json returns a JSON Value (array), convert to vec
            if let serde_json::Value::Array(items) = data_json {
                snapshot_data.add_model(&model.base_path, &items);
            }
        }
        drop(models_read);

        // Create the snapshot
        let mut mgr = snapshot_manager.write().await;
        mgr.create_snapshot(term, last_index, snapshot_data)
            .map_err(|e| format!("Failed to create snapshot: {}", e))
    }

    /// Send a snapshot to a desynced follower
    ///
    /// This pushes a snapshot to a follower that is too far behind to catch up
    /// via normal log replication. Returns Ok(()) on success.
    #[allow(dead_code)] // Kept for API compatibility, prefer send_snapshot_to_follower_with_timeout
    async fn send_snapshot_to_follower(
        peer: &str,
        snapshot_manager: &Arc<tokio::sync::RwLock<crate::cluster::snapshot::SnapshotManager>>,
    ) -> Result<(), String> {
        let mgr = snapshot_manager.read().await;

        let meta = mgr
            .current_meta()
            .ok_or_else(|| "No snapshot available to send".to_string())?
            .clone();

        let bytes = mgr
            .get_snapshot_bytes(meta.last_included_index)
            .map_err(|e| format!("Failed to read snapshot bytes: {}", e))?;

        drop(mgr);

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30)) // Longer timeout for large snapshots
            .build()
            .map_err(|e| e.to_string())?;

        let url = format!("http://{}/_raft/snapshot", peer);

        let response = client
            .post(&url)
            .header("Content-Type", "application/octet-stream")
            .header("X-Snapshot-Term", meta.term.to_string())
            .header("X-Snapshot-Index", meta.last_included_index.to_string())
            .header("X-Snapshot-Checksum", meta.checksum.to_string())
            .header("X-Snapshot-Size", meta.size_bytes.to_string())
            .body(bytes)
            .send()
            .await
            .map_err(|e| format!("Failed to send snapshot to {}: {}", peer, e))?;

        if response.status().is_success() {
            log::info!(
                "Snapshot sent successfully to {} (index={}, {}KB)",
                peer,
                meta.last_included_index,
                meta.size_bytes / 1024
            );
            Ok(())
        } else {
            Err(format!("Snapshot send to {} failed with status {}", peer, response.status()))
        }
    }

    /// Send a snapshot to a desynced follower with configurable timeout
    ///
    /// This version accepts a configurable timeout for large snapshots.
    async fn send_snapshot_to_follower_with_timeout(
        peer: &str,
        snapshot_manager: &Arc<tokio::sync::RwLock<crate::cluster::snapshot::SnapshotManager>>,
        timeout_secs: u64,
    ) -> Result<(), String> {
        let mgr = snapshot_manager.read().await;

        let meta = mgr
            .current_meta()
            .ok_or_else(|| "No snapshot available to send".to_string())?
            .clone();

        let bytes = mgr
            .get_snapshot_bytes(meta.last_included_index)
            .map_err(|e| format!("Failed to read snapshot bytes: {}", e))?;

        drop(mgr);

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .build()
            .map_err(|e| e.to_string())?;

        let url = format!("http://{}/_raft/snapshot", peer);

        let response = client
            .post(&url)
            .header("Content-Type", "application/octet-stream")
            .header("X-Snapshot-Term", meta.term.to_string())
            .header("X-Snapshot-Index", meta.last_included_index.to_string())
            .header("X-Snapshot-Checksum", meta.checksum.to_string())
            .header("X-Snapshot-Size", meta.size_bytes.to_string())
            .body(bytes)
            .send()
            .await
            .map_err(|e| format!("Failed to send snapshot to {}: {}", peer, e))?;

        if response.status().is_success() {
            log::info!(
                "Snapshot sent successfully to {} (index={}, {}KB, timeout={}s)",
                peer,
                meta.last_included_index,
                meta.size_bytes / 1024,
                timeout_secs
            );
            Ok(())
        } else {
            Err(format!("Snapshot send to {} failed with status {}", peer, response.status()))
        }
    }

    /// Replicate log entries to follower nodes and wait for majority acknowledgment
    ///
    /// This function sends requests to ALL followers IN PARALLEL and returns as soon as
    /// majority is reached. Slow nodes don't block the commit - they'll catch up later.
    ///
    /// Returns Ok(commit_index) when majority acknowledges, Err otherwise
    async fn replicate_log_entries_to_followers(
        peers: &[String],
        entries: Vec<crate::cluster::LogEntry>,
        leader_commit: u64,
        term: u64,
        leader_id: u64,
        leader_port: u16,
        batcher: Option<Arc<crate::cluster::ReplicationBatcher>>,
    ) -> Result<u64, String> {
        if peers.is_empty() {
            // Single node cluster - commit immediately
            return Ok(leader_commit);
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10)) // Long timeout to ensure all nodes receive
            .build()
            .map_err(|e| e.to_string())?;

        let request = crate::cluster::consensus_log::AppendEntriesRequest {
            term,
            leader_id,
            leader_port,
            prev_log_index: 0, // Simplified - in real Raft this would track per-follower
            prev_log_term: 0,
            entries: entries.clone(),
            leader_commit,
        };

        // Leader counts as 1 success
        let total_nodes = peers.len() + 1;
        let majority = total_nodes / 2 + 1;
        let needed_from_followers = majority.saturating_sub(1); // Already have leader's vote

        // Early return if leader alone is majority (single node with empty peers already handled above)
        if needed_from_followers == 0 {
            let new_commit = entries.last().map(|e| e.log_id.index).unwrap_or(leader_commit);
            return Ok(new_commit);
        }

        // Get health summary to skip desynced followers
        let health_summary = if let Some(ref b) = batcher {
            b.get_health_summary().await
        } else {
            std::collections::HashMap::new()
        };

        // Count active (non-desynced) peers for quorum calculation
        let active_peers: Vec<_> = peers
            .iter()
            .filter(|p| {
                health_summary
                    .get(*p)
                    .map(|h| *h != crate::cluster::replication_batcher::FollowerHealth::Desynced)
                    .unwrap_or(true) // Unknown = try anyway
            })
            .cloned()
            .collect();

        let skipped_count = peers.len() - active_peers.len();
        if skipped_count > 0 {
            log::debug!("Skipping {} desynced followers (will use snapshot resync)", skipped_count);
        }

        // Spawn parallel requests to active (non-desynced) followers only
        let mut handles = Vec::with_capacity(active_peers.len());
        for peer in active_peers {
            let endpoint = format!("http://{}/_raft/append", peer);
            let client = client.clone();
            let request = request.clone();
            let peer_name = peer.clone();
            let batcher_clone = batcher.clone();

            let target_commit = leader_commit;
            handles.push(tokio::spawn(async move {
                let start = std::time::Instant::now();
                let result = match client.post(&endpoint).json(&request).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        if let Ok(response) = resp
                            .json::<crate::cluster::consensus_log::AppendEntriesResponse>()
                            .await
                        {
                            if response.success {
                                // Log applied_index for debugging but don't require it
                                // Apply happens synchronously before response, so success means applied
                                if response.applied_index < target_commit {
                                    log::debug!(
                                        "Log replicated to {} (applied_index={}, target={})",
                                        peer_name,
                                        response.applied_index,
                                        target_commit
                                    );
                                } else {
                                    log::debug!(
                                        "Log replicated AND applied on {} (applied_index={})",
                                        peer_name,
                                        response.applied_index
                                    );
                                }
                                (peer_name.clone(), true, response.last_log_index)
                            } else {
                                (peer_name.clone(), false, 0)
                            }
                        } else {
                            (peer_name.clone(), false, 0)
                        }
                    }
                    Ok(resp) => {
                        log::warn!(
                            "Replicate log to {} failed: status {}",
                            peer_name,
                            resp.status()
                        );
                        (peer_name.clone(), false, 0)
                    }
                    Err(e) => {
                        log::warn!("Replicate log to {} error: {}", peer_name, e);
                        (peer_name.clone(), false, 0)
                    }
                };
                let latency_ms = start.elapsed().as_millis() as u64;

                // Update batcher with follower health info
                if let Some(ref batcher) = batcher_clone {
                    if result.1 {
                        batcher.record_success(&result.0, result.2, latency_ms).await;
                    } else {
                        batcher.record_failure(&result.0).await;
                    }
                }

                (result.0, result.1)
            }));
        }

        // Collect results - wait for ALL followers (not just majority) to ensure consistency
        // This trades latency for consistency: slow followers won't be left behind
        use futures::stream::{FuturesUnordered, StreamExt};
        let mut futures: FuturesUnordered<_> = handles.into_iter().collect();

        let mut success_count = 0usize;
        let mut _completed = 0usize; // Track completion count (reserved for metrics)
        let mut majority_reached = false;
        let mut commit_index = leader_commit;

        // Set a global timeout for the entire operation
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10); // Long timeout for consistency

        while let Ok(Some(result)) = tokio::time::timeout_at(deadline, futures.next()).await {
            _completed += 1;
            if let Ok((peer, success)) = result {
                if success {
                    success_count += 1;
                    log::debug!("{}/{} followers acknowledged", success_count, peers.len());

                    // Track when majority is reached, but DON'T return early
                    if success_count >= needed_from_followers && !majority_reached {
                        majority_reached = true;
                        commit_index =
                            entries.last().map(|e| e.log_id.index).unwrap_or(leader_commit);
                        log::debug!(
                            "Majority reached ({}/{}), will commit index {}",
                            success_count + 1,
                            total_nodes,
                            commit_index
                        );
                        // Continue waiting for remaining followers instead of returning
                    }
                } else {
                    log::debug!("Follower {} failed", peer);
                }
            }

            // Continue until all followers complete or timeout
            // Don't break early - we want to give all followers a chance
        }

        // Check if majority was reached (even if some followers timed out)
        if majority_reached {
            log::debug!(
                "Replication complete: {}/{} followers succeeded, committing {}",
                success_count,
                peers.len(),
                commit_index
            );
            Ok(commit_index)
        } else {
            Err(format!(
                "Failed to reach majority: {} of {} nodes responded successfully (need {})",
                success_count + 1, // +1 for leader
                total_nodes,
                majority
            ))
        }
    }
}

impl Default for LithairServer {
    fn default() -> Self {
        Self {
            config: LithairConfig::default(),
            session_manager: None,
            custom_routes: Vec::new(),
            not_found_handler: None,
            route_guards: Vec::new(),
            model_infos: Vec::new(),
            models_require_session: false,
            external_handler_gates: Vec::new(),
            external_handler_sse_wirings: Vec::new(),
            models: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            frontend_configs: Vec::new(),
            frontend_engines: std::collections::HashMap::new(),
            vhost_frontend_configs: Vec::new(),
            vhost_frontend_router: crate::http::HostRouter::new(),
            host_redirects: crate::http::HostRouter::new(),
            firewall_config: None,
            anti_ddos_config: None,
            access_log: false,
            access_log_capacity: crate::http::DEFAULT_ACCESS_LOG_CAPACITY,
            cluster_peers: Vec::new(),
            node_id: None,
            raft_state: None,
            raft_crud_sender: None,
            consensus_log: None,
            wal: None,
            replication_batcher: None,
            snapshot_manager: None,
            migration_manager: None,
            resync_stats: Arc::new(crate::cluster::ResyncStats::new()),
            schema_sync_state: Arc::new(tokio::sync::RwLock::new(
                crate::schema::SchemaSyncState::default(),
            )),
            openapi_enabled: false,
            openapi_spec_cache: std::sync::OnceLock::new(),
            sse_broadcaster: None,
            auto_compaction: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_creation() {
        let _server = LithairServer::default();
    }

    // ------------------------------------------------------------------
    // Built-in operations endpoints (`/health`, `/ready`, `/info`).
    //
    // These tests cover the LithairServer regression reported in
    // lithair/lithair#40: the README claims every Lithair server
    // ships with /health, /ready, /info, but `LithairServer` had no
    // dispatch for them and returned 404. We spin up a `LithairServer`
    // via `build()` (skipping the heavy `serve()` startup — schema
    // load, model factories, system metrics — none of which are
    // relevant here), bind a hyper service to a loopback ephemeral
    // port, and exercise the dispatch with a real reqwest client.
    // ------------------------------------------------------------------

    /// Serve a `LithairServer` on a loopback ephemeral port, return
    /// (base_url, abort_handle). Drop the abort_handle (or call
    /// `.abort()`) to stop the server.
    ///
    /// We deliberately *don't* call `LithairServer::serve()` here —
    /// it pulls in schema validation, env_logger init, frontend
    /// loading, and system metrics. None of that is wired for these
    /// tests, and serve() also blocks forever, which would deadlock
    /// the test harness.
    async fn spawn_for_test(server: LithairServer) -> (String, tokio::task::JoinHandle<()>) {
        use hyper::service::service_fn;
        use hyper_util::rt::TokioIo;
        use std::sync::Arc;

        let listener =
            tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind ephemeral port");
        let addr = listener.local_addr().expect("local_addr");
        let base = format!("http://{}", addr);

        let server = Arc::new(server);

        let handle = tokio::spawn(async move {
            loop {
                let (stream, _peer) = match listener.accept().await {
                    Ok(s) => s,
                    Err(_) => return,
                };
                let io = TokioIo::new(stream);
                let server = server.clone();
                tokio::spawn(async move {
                    let service = service_fn(move |req| {
                        let server = server.clone();
                        async move {
                            match server.handle_request(req).await {
                                Ok(resp) => Ok::<_, std::convert::Infallible>(resp),
                                Err(_) => Ok(hyper::Response::builder()
                                    .status(500)
                                    .body(http_body_util::Full::new(bytes::Bytes::from(
                                        r#"{"error":"handler error"}"#,
                                    )))
                                    .expect("valid HTTP response")),
                            }
                        }
                    });
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(io, service)
                        .await;
                });
            }
        });

        (base, handle)
    }

    #[tokio::test]
    async fn lithair_server_serves_default_health() {
        let server = LithairServer::new().build().expect("build server");
        let (base, handle) = spawn_for_test(server).await;

        let resp = reqwest::get(format!("{}/health", base)).await.expect("GET /health succeeded");
        assert_eq!(resp.status(), 200);
        let body = resp.text().await.expect("read body");
        assert_eq!(body, r#"{"status":"healthy"}"#);

        handle.abort();
    }

    #[tokio::test]
    async fn lithair_server_serves_default_ready() {
        let server = LithairServer::new().build().expect("build server");
        let (base, handle) = spawn_for_test(server).await;

        let resp = reqwest::get(format!("{}/ready", base)).await.expect("GET /ready succeeded");
        assert_eq!(resp.status(), 200);
        let body = resp.text().await.expect("read body");
        let parsed: serde_json::Value =
            serde_json::from_str(&body).expect("ready body must be JSON");
        assert_eq!(parsed["status"], "ready");
        assert_eq!(parsed["version"], env!("CARGO_PKG_VERSION"));

        handle.abort();
    }

    #[tokio::test]
    async fn lithair_server_serves_default_info() {
        let server = LithairServer::new().build().expect("build server");
        let (base, handle) = spawn_for_test(server).await;

        let resp = reqwest::get(format!("{}/info", base)).await.expect("GET /info succeeded");
        assert_eq!(resp.status(), 200);
        let body = resp.text().await.expect("read body");
        let parsed: serde_json::Value =
            serde_json::from_str(&body).expect("info body must be JSON");
        assert_eq!(parsed["server"], "Lithair Server");
        assert_eq!(parsed["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(parsed["endpoints"]["health"], "/health");
        assert_eq!(parsed["endpoints"]["ready"], "/ready");
        assert_eq!(parsed["endpoints"]["info"], "/info");
        // No models registered, so the array must be empty.
        assert!(parsed["models"].as_array().expect("models array").is_empty());

        handle.abort();
    }

    #[tokio::test]
    async fn lithair_server_user_with_route_overrides_default_health() {
        // A user calling `.with_route(GET, "/health", ...)` must win
        // over the built-in handler. The dispatch order in
        // handle_request places the custom_routes loop *before* the
        // ops endpoints precisely so this works.
        let server = LithairServer::new()
            .with_route(http::Method::GET, "/health", |_req| {
                Box::pin(async move {
                    Ok(hyper::Response::builder()
                        .status(418)
                        .header("Content-Type", "application/json")
                        .body(http_body_util::Full::new(bytes::Bytes::from(
                            r#"{"status":"i-am-a-teapot"}"#,
                        )))
                        .expect("valid HTTP response"))
                })
            })
            .build()
            .expect("build server");
        let (base, handle) = spawn_for_test(server).await;

        let resp = reqwest::get(format!("{}/health", base)).await.expect("GET /health succeeded");
        assert_eq!(resp.status(), 418, "user override must take precedence");
        let body = resp.text().await.expect("read body");
        assert_eq!(body, r#"{"status":"i-am-a-teapot"}"#);

        handle.abort();
    }

    #[tokio::test]
    async fn lithair_server_returns_404_for_non_get_health() {
        // The dispatch checks `method == GET`, so a POST /health
        // should still 404 (no built-in POST /health handler).
        let server = LithairServer::new().build().expect("build server");
        let (base, handle) = spawn_for_test(server).await;

        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/health", base))
            .send()
            .await
            .expect("POST /health succeeded");
        assert_eq!(resp.status(), 404);

        handle.abort();
    }

    // ------------------------------------------------------------------
    // Static-file HEAD support (issue #56).
    //
    // Before the fix, the static-file dispatch in `handle_request` only
    // matched `Method::GET`, and `FrontendServer::handle_request`
    // returned `METHOD_NOT_ALLOWED` for anything else. The result: any
    // HEAD probe on a perfectly-served static page (homepage, blog
    // post, rss.xml) fell through to the default
    // `{"error":"Not found"}` JSON 404 — silently breaking SEO
    // crawlers, monitors (`curl -I`, Uptime Robot), and any other
    // tooling that does HEAD-then-GET.
    //
    // RFC 7231 §4.3.2: HEAD must return the same status and headers
    // as GET, with an empty body.
    // ------------------------------------------------------------------

    /// Build a LithairServer wired with a single in-memory frontend
    /// engine serving the supplied (path, content, mime) tuples.
    async fn server_with_frontend(assets: &[(&str, &[u8])]) -> (LithairServer, tempfile::TempDir) {
        // FrontendEngine::new requires an on-disk data_dir for the
        // event store; the tempdir is dropped when the test ends.
        let tmp = tempfile::tempdir().expect("tempdir for frontend event store");
        let engine = crate::frontend::FrontendEngine::new("test_static", tmp.path())
            .await
            .expect("create FrontendEngine");

        for (path, content) in assets {
            engine.update_asset(path, content.to_vec()).await.expect("insert asset");
        }

        let mut server = LithairServer::new().build().expect("build server");
        // Inject directly — bypasses the on-disk load_directory path
        // that LithairServer normally uses in `serve()` (#56 test
        // doesn't exercise that path).
        server.frontend_engines.insert("/".to_string(), std::sync::Arc::new(engine));
        (server, tmp)
    }

    #[tokio::test]
    async fn static_file_get_returns_200_with_correct_content_type() {
        let (server, _tmp) = server_with_frontend(&[("/index.html", b"<h1>home</h1>")]).await;
        let (base, handle) = spawn_for_test(server).await;

        let resp = reqwest::get(format!("{}/index.html", base)).await.expect("GET /index.html");
        assert_eq!(resp.status(), 200);
        assert_eq!(
            resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or(""),
            "text/html"
        );
        let body = resp.text().await.expect("body");
        assert_eq!(body, "<h1>home</h1>");

        handle.abort();
    }

    #[tokio::test]
    async fn static_file_head_returns_200_with_correct_content_type_and_no_body() {
        // The original #56 reproduction: `curl -I /index.html` must
        // return 200 OK + text/html, not 404 + application/json.
        let (server, _tmp) = server_with_frontend(&[("/index.html", b"<h1>home</h1>")]).await;
        let (base, handle) = spawn_for_test(server).await;

        let client = reqwest::Client::new();
        let resp = client
            .head(format!("{}/index.html", base))
            .send()
            .await
            .expect("HEAD /index.html");

        assert_eq!(resp.status(), 200, "HEAD must mirror GET status");
        assert_eq!(
            resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or(""),
            "text/html",
            "HEAD must mirror GET content-type"
        );
        // Content-Length must describe what GET would have sent.
        assert_eq!(
            resp.headers().get("content-length").and_then(|v| v.to_str().ok()),
            Some("13"),
            "HEAD must advertise the GET payload size"
        );
        let body = resp.bytes().await.expect("body");
        assert!(body.is_empty(), "HEAD must not carry a body");

        handle.abort();
    }

    #[tokio::test]
    async fn static_file_head_rss_xml_returns_200_with_xml_content_type() {
        // The #56 acceptance list calls out /rss.xml specifically —
        // RSS readers and feed validators issue HEAD before GET. The
        // content-type must come from the asset's MIME guess, not be
        // hard-coded to text/html.
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?><rss/>"#;
        let (server, _tmp) = server_with_frontend(&[("/rss.xml", xml)]).await;
        let (base, handle) = spawn_for_test(server).await;

        let client = reqwest::Client::new();
        let resp = client.head(format!("{}/rss.xml", base)).send().await.expect("HEAD /rss.xml");

        assert_eq!(resp.status(), 200);
        let ct = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        // mime_guess maps .xml to either text/xml or application/xml
        // depending on platform DB; the key invariant is "not
        // application/json" (the broken 404 default).
        assert!(ct.contains("xml"), "expected an XML content-type, got `{}`", ct);

        let body = resp.bytes().await.expect("body");
        assert!(body.is_empty(), "HEAD must not carry a body");

        handle.abort();
    }

    #[tokio::test]
    async fn unknown_route_returns_404_with_json_error_negative_case() {
        // The negative case from #56's acceptance criteria: when no
        // frontend (and no other handler) matches, the default
        // `{"error":"Not found"}` JSON 404 must still fire — both for
        // GET and for HEAD. We deliberately do NOT register a
        // frontend here so the static dispatch is skipped entirely.
        let server = LithairServer::new().build().expect("build server");
        let (base, handle) = spawn_for_test(server).await;

        let client = reqwest::Client::new();

        // GET on a path with no handler.
        let resp = client
            .get(format!("{}/totally-unknown", base))
            .send()
            .await
            .expect("GET /totally-unknown");
        assert_eq!(resp.status(), 404);
        assert_eq!(
            resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or(""),
            "application/json"
        );
        let body = resp.text().await.expect("body");
        assert_eq!(body, r#"{"error":"Not found"}"#);

        // HEAD on the same unmatched path: hyper strips the body
        // automatically for HEAD responses on the wire, but the
        // status and content-type must still be the JSON 404 shape.
        let resp = client.head(format!("{}/totally-unknown", base)).send().await.expect("HEAD");
        assert_eq!(resp.status(), 404);
        assert_eq!(
            resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or(""),
            "application/json"
        );

        handle.abort();
    }

    // ------------------------------------------------------------------
    // Route handler type aliases + `with_route_async` helper (issue #59).
    //
    // The aliases (`RouteRequest`, `RouteResponse`) and the re-exports
    // (`Method`, `StatusCode`) exist so consumers can write handler
    // signatures without depending on `bytes`, `http`, `http-body-util`,
    // and `hyper` directly. The tests below prove:
    //
    // 1. The aliases are drop-in compatible with the existing
    //    `with_route` signature (no behavior change).
    // 2. The `with_route_async` helper accepts a plain async closure — no
    //    `Box::pin` boilerplate at the call site.
    // 3. Both registration paths route requests to the same dispatcher
    //    and produce the same response shape, so consumers can pick
    //    based on ergonomics alone.
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn with_route_alias_signature_compiles_and_serves() {
        // Prove `RouteRequest`/`RouteResponse` are drop-in replacements
        // for the long inline hyper types: register a route whose
        // closure uses *only* the public aliases plus the re-exported
        // `Method` / `StatusCode`, and dispatch a request through it.
        use super::{response, Method, RouteRequest, RouteResponse, StatusCode};

        let server = LithairServer::new()
            .with_route(Method::GET, "/issue-59-aliases", |_req: RouteRequest| {
                Box::pin(async move {
                    let resp: RouteResponse = response::json(StatusCode::OK, r#"{"alias":"ok"}"#);
                    Ok(resp)
                })
            })
            .build()
            .expect("build server");
        let (base, handle) = spawn_for_test(server).await;

        let resp = reqwest::get(format!("{}/issue-59-aliases", base))
            .await
            .expect("GET /issue-59-aliases");
        assert_eq!(resp.status(), 200);
        assert_eq!(
            resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or(""),
            "application/json"
        );
        let body = resp.text().await.expect("body");
        assert_eq!(body, r#"{"alias":"ok"}"#);

        handle.abort();
    }

    #[tokio::test]
    async fn with_route_async_compiles_without_box_pin_and_serves() {
        // `with_route_async` accepts a plain async closure — no manual
        // `Box::pin`, no explicit `Pin<Box<dyn Future>>` return type.
        // The dispatcher must still route the request correctly and
        // return the body the handler produced.
        use super::{response, Method, RouteRequest, StatusCode};

        let server = LithairServer::new()
            .with_route_async(
                Method::POST,
                "/issue-59-route-async",
                |_req: RouteRequest| async move {
                    Ok(response::json(StatusCode::ACCEPTED, r#"{"status":"queued"}"#))
                },
            )
            .build()
            .expect("build server");
        let (base, handle) = spawn_for_test(server).await;

        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/issue-59-route-async", base))
            .send()
            .await
            .expect("POST /issue-59-route-async");
        assert_eq!(resp.status(), 202);
        assert_eq!(
            resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or(""),
            "application/json"
        );
        let body = resp.text().await.expect("body");
        assert_eq!(body, r#"{"status":"queued"}"#);

        handle.abort();
    }

    #[tokio::test]
    async fn with_route_async_and_with_route_share_dispatch_precedence() {
        // Two routes registered via the two registration paths must
        // both win against the default ops endpoints. This is a
        // regression test against `with_route_async` accidentally routing
        // through a different code path than `with_route` (which would
        // be a silent behavior split).
        use super::{response, Method, RouteRequest, StatusCode};

        let server = LithairServer::new()
            // Override the built-in /health via the *new* helper.
            .with_route_async(Method::GET, "/health", |_req: RouteRequest| async move {
                Ok(response::json(StatusCode::IM_A_TEAPOT, r#"{"status":"teapot-async"}"#))
            })
            // Override /ready via the existing `with_route` API.
            .with_route(Method::GET, "/ready", |_req: RouteRequest| {
                Box::pin(async move {
                    Ok(response::json(StatusCode::IM_A_TEAPOT, r#"{"status":"teapot-with"}"#))
                })
            })
            .build()
            .expect("build server");
        let (base, handle) = spawn_for_test(server).await;

        let health = reqwest::get(format!("{}/health", base)).await.expect("GET /health succeeded");
        assert_eq!(health.status(), 418, "with_route_async override must take precedence");
        assert_eq!(health.text().await.expect("body"), r#"{"status":"teapot-async"}"#);

        let ready = reqwest::get(format!("{}/ready", base)).await.expect("GET /ready succeeded");
        assert_eq!(ready.status(), 418, "with_route override must still work");
        assert_eq!(ready.text().await.expect("body"), r#"{"status":"teapot-with"}"#);

        handle.abort();
    }

    // ------------------------------------------------------------------
    // `with_not_found_handler_async` (issue #61).
    //
    // Mirrors the `with_route` → `with_route_async` pairing from v0.4.0:
    // a plain async closure registers a 404 handler, no manual `Box::pin`.
    // Tests:
    //   1. The async variant routes through to the custom 404 path and
    //      its body/status reach the wire.
    //   2. The sync-pinned variant still works (regression guard).
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn with_not_found_handler_async_compiles_without_box_pin_and_serves() {
        use super::{response, RouteRequest, StatusCode};

        let server = LithairServer::new()
            .with_not_found_handler_async(|req: RouteRequest| async move {
                let path = req.uri().path().to_string();
                Ok(response::json_value(
                    StatusCode::NOT_FOUND,
                    &serde_json::json!({"error": "not_found", "path": path}),
                ))
            })
            .build()
            .expect("build server");
        let (base, handle) = spawn_for_test(server).await;

        let resp = reqwest::get(format!("{}/nope/missing", base)).await.expect("GET /nope/missing");
        assert_eq!(resp.status(), 404);
        assert_eq!(
            resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or(""),
            "application/json"
        );

        let body: serde_json::Value = resp.json().await.expect("json body");
        assert_eq!(body, serde_json::json!({"error": "not_found", "path": "/nope/missing"}));

        handle.abort();
    }

    #[tokio::test]
    async fn with_not_found_handler_sync_pinned_still_works() {
        // Regression: the sync-pinned `with_not_found_handler` must keep
        // working unchanged. Adding the `_async` variant is purely additive.
        use super::{response, StatusCode};

        let server = LithairServer::new()
            .with_not_found_handler(|_req| {
                Box::pin(async {
                    Ok(response::html(StatusCode::NOT_FOUND, "<h1>Page not found</h1>"))
                })
            })
            .build()
            .expect("build server");
        let (base, handle) = spawn_for_test(server).await;

        let resp = reqwest::get(format!("{}/nope", base)).await.expect("GET /nope");
        assert_eq!(resp.status(), 404);
        assert_eq!(
            resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or(""),
            "text/html; charset=utf-8"
        );
        let body = resp.text().await.expect("body");
        assert_eq!(body, "<h1>Page not found</h1>");

        handle.abort();
    }

    // ------------------------------------------------------------------
    // `request::*` body-reading helpers (issue #63).
    //
    // The unit tests in `app/request.rs` exercise the helpers through
    // `Request<Full<Bytes>>` because `hyper::body::Incoming` has no
    // public constructor. This e2e test drives the helpers through the
    // real wire path — request comes in as `Incoming`, handler calls
    // the helper, response goes out — so we catch any signature
    // mismatch the unit shims would miss.
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn request_read_body_as_string_drains_put_body_end_to_end() {
        use super::{request, response, Method, RouteRequest, StatusCode};

        let server = LithairServer::new()
            .with_route_async(Method::PUT, "/echo", |req: RouteRequest| async move {
                let body = request::read_body_as_string(req).await?;
                Ok(response::text(StatusCode::OK, body))
            })
            .build()
            .expect("build server");
        let (base, handle) = spawn_for_test(server).await;

        let client = reqwest::Client::new();
        let resp = client
            .put(format!("{}/echo", base))
            .body("config: ok\nname: kovre\n")
            .send()
            .await
            .expect("PUT /echo");
        assert_eq!(resp.status(), 200);
        let body = resp.text().await.expect("body");
        assert_eq!(body, "config: ok\nname: kovre\n");

        handle.abort();
    }

    #[tokio::test]
    async fn request_read_body_json_deserializes_put_body_end_to_end() {
        use super::{request, response, Method, RouteRequest, StatusCode};
        use serde::Deserialize;

        #[derive(Deserialize)]
        struct Payload {
            name: String,
            count: u32,
        }

        let server = LithairServer::new()
            .with_route_async(Method::POST, "/json", |req: RouteRequest| async move {
                let payload: Payload = request::read_body_json(req).await?;
                Ok(response::text(StatusCode::OK, format!("{} x{}", payload.name, payload.count)))
            })
            .build()
            .expect("build server");
        let (base, handle) = spawn_for_test(server).await;

        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/json", base))
            .header("content-type", "application/json")
            .body(r#"{"name":"widget","count":3}"#)
            .send()
            .await
            .expect("POST /json");
        assert_eq!(resp.status(), 200);
        assert_eq!(resp.text().await.expect("body"), "widget x3");

        handle.abort();
    }

    #[tokio::test]
    async fn request_read_body_with_limit_rejects_oversize_end_to_end() {
        // Send a 4 KiB payload to a route that caps the read at 1 KiB.
        // The handler returns 413 on rejection, mirroring how a real
        // consumer (kovre) would map the error.
        use super::{request, response, Method, RouteRequest, StatusCode};

        let server = LithairServer::new()
            .with_route_async(Method::PUT, "/limited", |req: RouteRequest| async move {
                match request::read_body_with_limit(req, 1024).await {
                    Ok(_bytes) => Ok(response::text(StatusCode::OK, "ok")),
                    Err(e) => Ok(response::text(StatusCode::PAYLOAD_TOO_LARGE, format!("{e}"))),
                }
            })
            .build()
            .expect("build server");
        let (base, handle) = spawn_for_test(server).await;

        let big = vec![b'a'; 4096];
        let client = reqwest::Client::new();
        let resp = client
            .put(format!("{}/limited", base))
            .body(big)
            .send()
            .await
            .expect("PUT /limited");
        assert_eq!(resp.status(), 413);
        let body = resp.text().await.expect("body");
        assert!(
            body.contains("exceeds limit"),
            "error body should describe the rejection, got: {body}"
        );

        handle.abort();
    }

    #[tokio::test]
    async fn request_read_body_with_limit_accepts_undersize_end_to_end() {
        use super::{request, response, Method, RouteRequest, StatusCode};

        let server = LithairServer::new()
            .with_route_async(Method::PUT, "/limited", |req: RouteRequest| async move {
                let bytes = request::read_body_with_limit(req, 1024).await?;
                Ok(response::text(StatusCode::OK, format!("read {} bytes", bytes.len())))
            })
            .build()
            .expect("build server");
        let (base, handle) = spawn_for_test(server).await;

        let client = reqwest::Client::new();
        let resp = client
            .put(format!("{}/limited", base))
            .body("small payload")
            .send()
            .await
            .expect("PUT /limited");
        assert_eq!(resp.status(), 200);
        assert_eq!(resp.text().await.expect("body"), "read 13 bytes");

        handle.abort();
    }

    // ------------------------------------------------------------------
    // Per-model stats parallelism on `/metrics` (Gemini PR #83 round-3).
    //
    // Regression test for the sequential `.await` loop that previously
    // computed each model's stats one after the other. With N models
    // and a per-model latency of `SLEEP`, the old wall-clock cost was
    // N * SLEEP. After parallelising via `futures::future::join_all`
    // the cost should be roughly SLEEP (max, not sum). We bound the
    // assertion at 2 * SLEEP so a true sequential regression (N=5,
    // 5 * SLEEP) trips it while CI scheduler jitter stays in the green.
    // ------------------------------------------------------------------

    /// Test-only `ModelHandler` that sleeps for a configurable duration in
    /// `get_stats` and returns trivial values elsewhere. Used to prove that
    /// `handle_metrics_request` collects per-model stats concurrently — only
    /// `get_stats` is invoked by `/metrics`, so the other trait methods
    /// stay as minimal stubs that panic if accidentally called (which would
    /// indicate the test wiring drifted from the metrics endpoint).
    struct SlowStatsHandler {
        name: String,
        sleep: std::time::Duration,
    }

    #[async_trait::async_trait]
    impl crate::app::ModelHandler for SlowStatsHandler {
        async fn handle_request(
            &self,
            _req: hyper::Request<hyper::body::Incoming>,
            _path_segments: &[&str],
        ) -> Result<
            hyper::Response<
                http_body_util::combinators::BoxBody<bytes::Bytes, std::convert::Infallible>,
            >,
            std::convert::Infallible,
        > {
            unreachable!("SlowStatsHandler::handle_request must not be called by /metrics");
        }

        async fn get_all_data_json(&self) -> serde_json::Value {
            serde_json::Value::Array(vec![])
        }

        async fn get_item_json(&self, _id: &str) -> Option<serde_json::Value> {
            None
        }

        async fn get_count(&self) -> usize {
            0
        }

        async fn export_json(&self) -> serde_json::Value {
            serde_json::json!({})
        }

        async fn get_stats(&self, _data_path: &str) -> crate::app::ModelStats {
            // Deliberate latency. join_all should fire all of these at once,
            // so total wall-clock time stays ~= `self.sleep`, not N * sleep.
            tokio::time::sleep(self.sleep).await;
            crate::app::ModelStats {
                model: self.name.clone(),
                item_count: 0,
                approx_ram_bytes: 0,
                raftlog_size_bytes: 0,
                events_since_last_compaction: None,
                last_compaction_at: None,
            }
        }

        fn model_name(&self) -> &str {
            &self.name
        }

        fn base_path(&self) -> &str {
            "/test"
        }

        async fn get_entity_history(&self, _id: &str) -> serde_json::Value {
            serde_json::Value::Array(vec![])
        }

        async fn get_entity_event_count(&self, _id: &str) -> usize {
            0
        }

        async fn submit_edit_event(
            &self,
            _id: &str,
            _changes: serde_json::Value,
        ) -> Result<serde_json::Value, String> {
            Err("not implemented in test handler".to_string())
        }

        async fn apply_replicated_item_json(
            &self,
            _item_json: serde_json::Value,
        ) -> Result<(), String> {
            Ok(())
        }

        async fn apply_replicated_items_json(
            &self,
            _items_json: Vec<serde_json::Value>,
        ) -> Result<usize, String> {
            Ok(0)
        }

        async fn apply_replicated_update_json(
            &self,
            _id: &str,
            _item_json: serde_json::Value,
        ) -> Result<(), String> {
            Ok(())
        }

        async fn apply_replicated_delete_json(&self, _id: &str) -> Result<bool, String> {
            Ok(false)
        }
    }

    #[tokio::test]
    async fn metrics_endpoint_collects_per_model_stats_concurrently() {
        // 5 models × 200 ms sleep. Sequential would take ~1000 ms; parallel
        // should land near 200 ms. We assert < 400 ms (2× SLEEP) so the test
        // doesn't false-fail on slow CI runners while still catching a true
        // sequential regression (which would be ≥5× SLEEP = 1000 ms).
        const N_MODELS: usize = 5;
        const SLEEP: std::time::Duration = std::time::Duration::from_millis(200);
        let upper_bound = SLEEP * 2;

        let server = LithairServer::new().build().expect("build server");

        // Inject N SlowStatsHandler instances directly into the models
        // registry. `models` is a private field but accessible from this
        // child test module (same crate, parent `app` module).
        {
            let mut models = server.models.write().await;
            for i in 0..N_MODELS {
                models.push(ModelRegistration {
                    name: format!("SlowModel{}", i),
                    base_path: format!("/test/{}", i),
                    data_path: "/tmp/lithair-slowmodel-test".to_string(),
                    handler: Arc::new(SlowStatsHandler {
                        name: format!("SlowModel{}", i),
                        sleep: SLEEP,
                    }),
                    schema_extractor: None,
                });
            }
        }

        // Round-trip through the real HTTP stack via spawn_for_test rather
        // than calling `handle_metrics_request` directly — the handler takes
        // `Request<hyper::body::Incoming>` and building an `Incoming` body
        // outside hyper's connection state is non-trivial. The HTTP path is
        // also what production code exercises, so timing it is the right
        // proxy for the real cost.
        let (base, handle) = spawn_for_test(server).await;

        let start = std::time::Instant::now();
        let resp = reqwest::get(format!("{}/metrics", base)).await.expect("GET /metrics succeeded");
        let elapsed = start.elapsed();
        assert_eq!(resp.status(), 200, "/metrics should return 200");

        // Body sanity check: each slow model must appear in the output. This
        // protects against the test silently passing if the loop were
        // skipped entirely (e.g. early-return on empty models).
        let body = resp.text().await.expect("read body");
        for i in 0..N_MODELS {
            let needle = format!(r#"lithair_model_items{{model="SlowModel{}"}}"#, i);
            assert!(
                body.contains(&needle),
                "metrics body missing per-model series for SlowModel{}: body was {}",
                i,
                body
            );
        }

        assert!(
            elapsed < upper_bound,
            "metrics collection took {:?}, expected < {:?} (sequential regression: \
             {} models × {:?} = {:?} would exceed this)",
            elapsed,
            upper_bound,
            N_MODELS,
            SLEEP,
            SLEEP * N_MODELS as u32
        );

        handle.abort();
    }
}
