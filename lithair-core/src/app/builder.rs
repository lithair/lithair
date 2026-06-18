//! Builder pattern for LithairServer

use super::{CustomRoute, LithairServer, RouteRequest, RouteResponse};
use crate::config::LithairConfig;
use crate::session::{PersistentSessionStore, SessionManager};
use anyhow::Result;
use std::sync::Arc;

/// Resolve the base directory for Raft persistent state (WAL, snapshots).
///
/// In production this defaults to `./data` (relative to the server's working
/// directory), which is the long-standing convention used by the replication
/// example. Tests and integration harnesses that need isolated per-run state
/// can override it by setting one of two environment variables:
///
/// - `LITHAIR_DATA_DIR` — primary override, framework-scoped name.
/// - `EXPERIMENT_DATA_BASE` — back-compat with the existing example/CI
///   convention already used by `examples/09-replication/cluster_node.rs`
///   to scope the per-node *event-store* directories. Honoring it here lets
///   the BDD harness scope the *Raft WAL and snapshots* with the same
///   env var it already sets — no harness change required.
///
/// Resolution order: `LITHAIR_DATA_DIR` > `EXPERIMENT_DATA_BASE` > `./data`.
///
/// Returning a String keeps the call sites simple (`format!("{}/...", ...)`).
/// The path is intentionally not validated/created here — `WriteAheadLog::new`
/// and `SnapshotManager::new` already create parent directories as needed.
fn raft_base_dir() -> String {
    if let Ok(v) = std::env::var("LITHAIR_DATA_DIR") {
        if !v.is_empty() {
            return v;
        }
    }
    if let Ok(v) = std::env::var("EXPERIMENT_DATA_BASE") {
        if !v.is_empty() {
            return v;
        }
    }
    "./data".to_string()
}

/// Identifies which virtual host a per-vhost configuration belongs to.
///
/// Used internally by the builder to tag frontend configurations so
/// the server can later install them into a [`crate::http::HostRouter`].
///
/// - [`VhostScope::Host`] — an explicit, normalized hostname.
/// - [`VhostScope::Default`] — the fallback when no host matches.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum VhostScope {
    /// Configuration bound to a normalized host (see
    /// [`crate::http::normalize_host`]).
    Host(String),
    /// Configuration used when no registered host matches.
    Default,
}

/// Builder for a single virtual host, obtained via
/// [`LithairServerBuilder::with_vhost`] or
/// [`LithairServerBuilder::with_default_vhost`].
///
/// The API deliberately starts small: the initial
/// use-case (lithair.net + arcker.org on a single process, tracked in
/// `lithair/lithair#30`) only needs per-host frontends. Models and custom
/// routes remain host-agnostic in this first iteration. See the PR
/// description for the follow-up plan.
pub struct VhostBuilder {
    #[allow(dead_code)] // reserved for richer per-vhost diagnostics in follow-ups
    scope: VhostScope,
    frontend_configs: Vec<(String, String)>, // (route_prefix, static_dir)
}

impl VhostBuilder {
    fn new(scope: VhostScope) -> Self {
        Self { scope, frontend_configs: Vec::new() }
    }

    /// Serve static files for this virtual host from `static_dir` at
    /// `route_prefix`. Behaves like
    /// [`LithairServerBuilder::with_frontend_at`] but is only consulted
    /// when the request's `Host:` header matches this vhost.
    pub fn with_frontend_at(
        mut self,
        route_prefix: impl Into<String>,
        static_dir: impl Into<String>,
    ) -> Self {
        self.frontend_configs.push((route_prefix.into(), static_dir.into()));
        self
    }
}

/// Builder for LithairServer
pub struct LithairServerBuilder {
    config: LithairConfig,
    session_manager: Option<Arc<dyn std::any::Any + Send + Sync>>,
    permission_checker: Option<Arc<dyn crate::rbac::PermissionChecker>>,
    custom_routes: Vec<CustomRoute>,
    not_found_handler: Option<super::RouteHandler>,
    model_infos: Vec<crate::app::ModelRegistrationInfo>,

    // HTTP Features
    route_guards: Vec<crate::http::RouteGuardMatcher>, // Declarative route protection
    firewall_config: Option<crate::http::FirewallConfig>,
    anti_ddos_config: Option<crate::security::anti_ddos::AntiDDoSConfig>,
    #[cfg(feature = "mfa")]
    mfa_storage: Option<Arc<crate::mfa::MfaStorage>>, // MFA/TOTP storage
    access_log: bool,
    access_log_capacity: usize,

    // SSE real-time subscriptions
    sse_enabled: bool,

    // Frontend configurations (path_prefix -> static_dir)
    frontend_configs: Vec<(String, String)>, // (route_prefix, static_dir)

    // Per-vhost frontend configurations.
    //
    // Host-header based routing (see `http::host_router`). Each entry is
    // bound to a normalized host; an entry whose host is the reserved
    // `VhostScope::Default` acts as the fallback when no host matches.
    //
    // When this vec is empty, the server behaves exactly as before this
    // feature was introduced (path-only routing).
    vhost_frontend_configs: Vec<(VhostScope, String, String)>, // (scope, route_prefix, static_dir)

    // Host-to-host 301 redirects, declared via [`Self::with_redirect`].
    //
    // Keyed by normalized source host, value is the canonical target host
    // (also normalized). At dispatch time, requests whose `Host:` header
    // matches a key are answered with a `301 Moved Permanently` whose
    // `Location:` header is `https://<target>{path_and_query}`.
    //
    // Stored as a plain `Vec` here so the builder stays cheap to clone /
    // inspect; the `HostRouter` is materialized in `build()`.
    host_redirects: Vec<(String, String)>, // (from_host_normalized, to_host_normalized)

    // Cluster/Raft configuration
    cluster_peers: Vec<String>,
    node_id: Option<u64>,

    // Schema sync policy (for cluster consensus)
    schema_vote_policy: Option<crate::schema::SchemaVotePolicy>,

    // OpenAPI spec generation
    openapi_enabled: bool,

    // Issue #78: when true, every auto-generated `/api/{model}` endpoint
    // registered via `with_model(...)` rejects requests without a valid
    // session with HTTP 401. Defaults to false (current behavior).
    //
    // Intentionally does NOT apply to `with_model_full(...)` — that path
    // already supports RBAC via `PermissionChecker` and has its own
    // semantics. Consumers using `with_model_full` opt into a richer
    // authorization model and don't need this flag.
    models_require_session: bool,

    // Issue #86: deferred session-gate appliers for models registered via
    // `with_handler(...)`. That path takes an externally-constructed
    // `Arc<DeclarativeHttpHandler<T>>` whose `set_require_session` flag we
    // can only flip *after* the Arc has been shared with the caller
    // (otherwise we'd break the whole point of `with_handler`, which is
    // letting the caller keep their own clone). Each closure captures
    // `Arc::clone(&handler)` from the registration site and applies the
    // flag through interior mutability at `serve()` time, after all
    // builder methods have run — so the order of `.with_handler(...)`,
    // `.with_sessions(...)`, and `.with_models_require_session(...)` no
    // longer matters.
    //
    // Type-erased over `T` so we can store heterogeneous handler types in
    // one vec; the closure body itself is monomorphic over the registering
    // call site's `T`.
    //
    // The closure signature is `Fn(bool)` where the bool is the value the
    // gate should be flipped to (always `true` in current use; passed
    // explicitly so future toggling paths stay uniform).
    external_handler_gates: Vec<super::ExternalHandlerGate>,

    // Issue #91: same shape as `external_handler_gates`, but for installing
    // the builder-level SSE broadcaster onto handlers registered via
    // `with_handler(...)` (and therefore `with_model_ref`). Each closure
    // takes the `Arc<SseEventBroadcaster>` and invokes
    // `set_sse_broadcaster_shared` on the captured handler Arc. Applied at
    // `serve()` time only when a builder-level broadcaster exists (i.e. the
    // user called `.with_sse(true)`).
    external_handler_sse_wirings: Vec<super::ExternalHandlerSseWiring>,

    // Issue #69: opt-in auto-compaction of the `.raftlog` for every
    // registered model. When `Some`, `LithairServer::serve()` spawns one
    // tokio task per model that periodically checks the model's event
    // count and triggers `EventStore::truncate_events()` when the count
    // crosses `events_threshold`. Default is `None` — feature is off, no
    // observable change for existing consumers.
    auto_compaction: Option<crate::engine::AutoCompactionConfig>,
}

impl LithairServerBuilder {
    /// Get the session manager (for custom routes that need session validation)
    pub fn session_manager(&self) -> Option<Arc<dyn std::any::Any + Send + Sync>> {
        self.session_manager.clone()
    }

    /// Create a new builder with default configuration
    pub fn new() -> Self {
        Self {
            config: LithairConfig::load().unwrap_or_default(),
            session_manager: None,
            permission_checker: None,
            custom_routes: Vec::new(),
            not_found_handler: None,
            model_infos: Vec::new(),
            route_guards: Vec::new(),
            firewall_config: None,
            anti_ddos_config: None,
            #[cfg(feature = "mfa")]
            mfa_storage: None,
            access_log: false,
            access_log_capacity: crate::http::DEFAULT_ACCESS_LOG_CAPACITY,
            frontend_configs: Vec::new(),
            vhost_frontend_configs: Vec::new(),
            host_redirects: Vec::new(),
            cluster_peers: Vec::new(),
            node_id: None,
            schema_vote_policy: None,
            openapi_enabled: false,
            sse_enabled: false,
            models_require_session: false,
            external_handler_gates: Vec::new(),
            external_handler_sse_wirings: Vec::new(),
            auto_compaction: None,
        }
    }

    /// Create a builder with custom configuration
    pub fn with_config(config: LithairConfig) -> Self {
        Self {
            config,
            session_manager: None,
            permission_checker: None,
            custom_routes: Vec::new(),
            not_found_handler: None,
            model_infos: Vec::new(),
            route_guards: Vec::new(),
            firewall_config: None,
            anti_ddos_config: None,
            #[cfg(feature = "mfa")]
            mfa_storage: None,
            access_log: false,
            access_log_capacity: crate::http::DEFAULT_ACCESS_LOG_CAPACITY,
            frontend_configs: Vec::new(),
            vhost_frontend_configs: Vec::new(),
            host_redirects: Vec::new(),
            cluster_peers: Vec::new(),
            node_id: None,
            schema_vote_policy: None,
            openapi_enabled: false,
            sse_enabled: false,
            models_require_session: false,
            external_handler_gates: Vec::new(),
            external_handler_sse_wirings: Vec::new(),
            auto_compaction: None,
        }
    }

    // ========================================================================
    // SERVER CONFIGURATION
    // ========================================================================

    /// Set server port (overrides config file and env vars)
    pub fn with_port(mut self, port: u16) -> Self {
        self.config.server.port = port;
        self
    }

    /// Set server host
    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.config.server.host = host.into();
        self
    }

    /// Set request timeout
    pub fn with_timeout(mut self, timeout: u64) -> Self {
        self.config.server.request_timeout = timeout;
        self
    }

    /// Set max body size
    pub fn with_max_body_size(mut self, size: usize) -> Self {
        self.config.server.max_body_size = size;
        self
    }

    /// Enable TLS with certificate and key PEM files
    ///
    /// When set, the server serves HTTPS with HSTS.
    /// Alternatively, set `LT_TLS_CERT` and `LT_TLS_KEY` environment variables.
    pub fn with_tls(mut self, cert_path: impl Into<String>, key_path: impl Into<String>) -> Self {
        self.config.server.tls_cert_path = Some(cert_path.into());
        self.config.server.tls_key_path = Some(key_path.into());
        self
    }

    // ========================================================================
    // SESSIONS
    // ========================================================================

    /// Add session manager
    pub fn with_sessions<S>(mut self, manager: SessionManager<S>) -> Self
    where
        S: crate::session::SessionStore + 'static + Send + Sync,
    {
        self.config.sessions.enabled = true;
        self.session_manager = Some(Arc::new(manager));
        self
    }

    /// Require a valid session on every auto-generated `/api/{model}` endpoint
    /// registered via [`Self::with_model`], [`Self::with_declarative_model`],
    /// or [`Self::with_handler`].
    ///
    /// When `require == true`, the model handler rejects every non-OPTIONS
    /// request lacking a valid (non-expired) session in the
    /// `Authorization: Bearer <session-id>` header with HTTP 401
    /// `{"error":"Authentication required"}` before any business logic runs.
    /// OPTIONS preflight requests are exempt by design (CORS preflight does
    /// not carry credentials).
    ///
    /// This is the simple "logged-in or not" gate for the common case. For
    /// per-role / per-permission gating, use [`Self::with_model_full`] with a
    /// [`crate::rbac::PermissionChecker`] instead; this flag does not affect
    /// `with_model_full(...)` registrations.
    ///
    /// **Note** (issue #86): pre-0.8.1, the `with_handler` registration path
    /// silently bypassed this flag. From 0.8.1 onward, `with_handler`-
    /// registered routes are gated uniformly with `with_model`-registered
    /// routes, matching the docstring above.
    ///
    /// Default: `false` (current behavior preserved — endpoints stay open).
    ///
    /// Must be paired with [`Self::with_sessions`]; without a configured
    /// session store, every request is treated as having no valid session
    /// and the gate returns 401 unconditionally.
    ///
    /// # Example
    /// ```ignore
    /// LithairServer::new()
    ///     .with_sessions(session_manager)
    ///     .with_models_require_session(true)   // gates ALL auto-generated /api/*
    ///     .with_model::<Account>("./data/accounts", "/api/accounts")
    ///     .with_model::<Mail>("./data/mails", "/api/mails")
    ///     .serve()
    ///     .await?;
    /// ```
    pub fn with_models_require_session(mut self, require: bool) -> Self {
        self.models_require_session = require;
        self
    }

    /// Enable builder-driven auto-compaction of every registered model's
    /// `.raftlog` (issue #69).
    ///
    /// `events_threshold` is the event count above which compaction
    /// (`EventStore::truncate_events`) fires. `check_interval` is how often
    /// the background task wakes up and inspects each model's event count.
    ///
    /// The framework already exposes the underlying primitives
    /// (`SnapshotStore`, `EventStore::truncate_events`, `event_count`); this
    /// flag just drives them. Default is **off** — calling this method opts
    /// in; not calling it preserves the previous behavior.
    ///
    /// # Panics
    ///
    /// Panics if `events_threshold == 0`. A threshold of zero would trigger
    /// compaction on every tick, defeating the purpose of having a
    /// threshold. For callers that prefer error-handling over panic, use
    /// [`Self::with_auto_compaction_config`] with an explicitly constructed
    /// [`crate::engine::AutoCompactionConfig`].
    ///
    /// # Example
    /// ```ignore
    /// use std::time::Duration;
    /// LithairServer::new()
    ///     .with_model::<Mail>("./data/mails", "/api/mails")
    ///     .with_auto_compaction(10_000, Duration::from_secs(300))
    ///     .serve()
    ///     .await?;
    /// ```
    pub fn with_auto_compaction(
        mut self,
        events_threshold: usize,
        check_interval: std::time::Duration,
    ) -> Self {
        let cfg = crate::engine::AutoCompactionConfig::new(events_threshold, check_interval)
            .expect("with_auto_compaction: events_threshold must be > 0");
        self.auto_compaction = Some(cfg);
        self
    }

    /// Enable builder-driven auto-compaction with an already-validated
    /// [`crate::engine::AutoCompactionConfig`] (issue #69).
    ///
    /// Prefer [`Self::with_auto_compaction`] for the common case; this
    /// variant exists so callers that construct a config dynamically
    /// (e.g. from a settings file) can surface validation errors
    /// themselves and avoid the panic-on-zero behavior of the convenience
    /// method.
    pub fn with_auto_compaction_config(mut self, cfg: crate::engine::AutoCompactionConfig) -> Self {
        self.auto_compaction = Some(cfg);
        self
    }

    // ========================================================================
    // RAFT CLUSTER (Distributed Consensus)
    // ========================================================================

    /// Configure Raft cluster mode with peers
    ///
    /// This enables distributed consensus with automatic leader election,
    /// heartbeat monitoring, and write redirection to the leader.
    ///
    /// # Example
    /// ```rust,ignore
    /// LithairServer::new()
    ///     .with_port(8080)
    ///     .with_raft_cluster(1, vec!["127.0.0.1:8081", "127.0.0.1:8082"])
    ///     .with_model::<Product>("./data/products", "/api/products")
    ///     .serve()
    ///     .await?;
    /// ```
    pub fn with_raft_cluster(mut self, node_id: u64, peers: Vec<impl Into<String>>) -> Self {
        self.node_id = Some(node_id);
        self.cluster_peers = peers.into_iter().map(|p| p.into()).collect();
        self.config.raft.enabled = true;
        self
    }

    /// Set the Raft configuration (path, auth, timeouts)
    ///
    /// # Example
    /// ```rust,ignore
    /// use lithair_core::config::RaftConfig;
    ///
    /// LithairServer::new()
    ///     .with_raft_cluster(1, vec!["127.0.0.1:8081"])
    ///     .with_raft_config(RaftConfig::new()
    ///         .with_path("/_internal/raft")
    ///         .with_auth("secret-token"))
    ///     .serve()
    ///     .await?;
    /// ```
    pub fn with_raft_config(mut self, config: crate::config::RaftConfig) -> Self {
        self.config.raft = config;
        self
    }

    /// Set Raft endpoint path (default: "/raft")
    pub fn with_raft_path(mut self, path: impl Into<String>) -> Self {
        self.config.raft.path = path.into();
        self
    }

    /// Enable Raft authentication with token
    pub fn with_raft_auth(mut self, token: impl Into<String>) -> Self {
        self.config.raft.auth_required = true;
        self.config.raft.auth_token = Some(token.into());
        self
    }

    /// Set schema vote policy for cluster-wide schema consensus
    ///
    /// Controls how schema changes are handled in a cluster:
    /// - `additive`: Policy for safe changes (nullable fields, fields with defaults)
    /// - `breaking`: Policy for dangerous changes (removing fields, adding required fields)
    /// - `versioned`: Policy for structural changes (type changes, renames)
    ///
    /// # Example
    /// ```rust,ignore
    /// use lithair_core::schema::{SchemaVotePolicy, VoteStrategy};
    ///
    /// LithairServer::new()
    ///     .with_raft_cluster(1, vec!["127.0.0.1:8081"])
    ///     .with_schema_policy(SchemaVotePolicy {
    ///         additive: VoteStrategy::AutoAccept,      // Safe changes auto-accepted
    ///         breaking: VoteStrategy::Reject,          // Dangerous changes rejected
    ///         versioned: VoteStrategy::Consensus,      // Structural changes need vote
    ///     })
    ///     .serve()
    ///     .await?;
    /// ```
    pub fn with_schema_policy(mut self, policy: crate::schema::SchemaVotePolicy) -> Self {
        self.schema_vote_policy = Some(policy);
        self
    }

    // ========================================================================
    // ADMIN PANEL
    // ========================================================================

    /// Enable/disable admin panel
    pub fn with_admin_panel(mut self, enabled: bool) -> Self {
        self.config.admin.enabled = enabled;
        self
    }

    /// Set admin panel path
    pub fn with_admin_path(mut self, path: impl Into<String>) -> Self {
        self.config.admin.path = path.into();
        self
    }

    /// Enable/disable admin authentication
    pub fn with_admin_auth(mut self, enabled: bool) -> Self {
        self.config.admin.auth_required = enabled;
        self
    }

    /// Enable/disable metrics
    pub fn with_metrics(mut self, enabled: bool) -> Self {
        self.config.admin.metrics_enabled = enabled;
        self
    }

    // ========================================================================
    // STORAGE
    // ========================================================================

    /// Set data directory
    pub fn with_data_dir(mut self, dir: impl Into<String>) -> Self {
        self.config.storage.data_dir = dir.into();
        self
    }

    // ========================================================================
    // HTTP FEATURES
    // ========================================================================

    /// Enable HTTP access logging (structured JSON via `log::info!`).
    ///
    /// Can also be enabled via `LT_HTTP_ACCESS_LOG=1` environment variable.
    pub fn with_access_log(mut self, enabled: bool) -> Self {
        self.access_log = enabled;
        self
    }

    /// Set the capacity of the in-memory access log ring buffer.
    ///
    /// Default: 50,000 entries. Only relevant when access logging is enabled.
    pub fn with_access_log_capacity(mut self, capacity: usize) -> Self {
        self.access_log_capacity = capacity;
        self
    }

    /// Enable SSE real-time subscriptions for model changes
    ///
    /// When enabled, each model gets a `GET /api/{model}/stream` endpoint
    /// that streams create/update/delete events via Server-Sent Events.
    pub fn with_sse(mut self, enabled: bool) -> Self {
        self.sse_enabled = enabled;
        self
    }

    /// Enable OpenAPI spec generation at `/openapi.json`
    ///
    /// When enabled, the server auto-generates an OpenAPI 3.1 specification
    /// from all registered DeclarativeModel definitions and serves it at
    /// `GET /openapi.json`. A minimal Swagger UI is also served at `GET /docs`.
    ///
    /// Only models that expose `schema_spec()` through their handler
    /// are included in the generated spec.
    pub fn with_openapi(mut self, enabled: bool) -> Self {
        self.openapi_enabled = enabled;
        self
    }

    /// Configure firewall
    pub fn with_firewall_config(mut self, config: crate::http::FirewallConfig) -> Self {
        self.firewall_config = Some(config);
        self
    }

    /// Configure anti-DDoS protection
    pub fn with_anti_ddos_config(
        mut self,
        config: crate::security::anti_ddos::AntiDDoSConfig,
    ) -> Self {
        self.anti_ddos_config = Some(config);
        self
    }

    /// Configure RBAC with automatic auth routes generation
    ///
    /// This method automatically:
    /// - Creates PersistentSessionStore for sessions
    /// - Registers POST /auth/login handler
    /// - Registers POST /auth/logout handler
    /// - Registers GET /auth/validate handler (session validation)
    /// - Returns PermissionChecker for use with models
    ///
    /// # Example
    /// ```ignore
    /// use lithair_core::rbac::{ServerRbacConfig, RbacUser};
    ///
    /// LithairServer::new()
    ///     .with_rbac_config(ServerRbacConfig::new()
    ///         .with_roles(vec![
    ///             ("Admin".to_string(), vec!["*".to_string()]),
    ///             ("Editor".to_string(), vec!["Read".to_string(), "Write".to_string()]),
    ///         ])
    ///         .with_users(vec![
    ///             RbacUser::new("admin", "password123", "Admin"),
    ///             RbacUser::new("editor", "password123", "Editor"),
    ///         ])
    ///         .with_session_store("./data/sessions")
    ///     )
    ///     .serve()
    ///     .await?;
    /// ```
    pub fn with_rbac_config(mut self, config: crate::rbac::ServerRbacConfig) -> Self {
        use crate::rbac::{handle_rbac_login, handle_rbac_logout};
        use std::sync::Arc;

        // Create session store path (default if not provided)
        let session_path = config
            .session_store_path
            .clone()
            .unwrap_or_else(|| "./data/sessions".to_string());

        // Store session duration for handlers
        let session_duration = config.session_duration;

        // Clone users for handlers
        let users_login = config.users.clone();
        let users_clone = config.users.clone();

        // Create permission checker from config
        let permission_checker = config.create_permission_checker();

        // CRITICAL: Create SHARED session store for login AND models
        // This ensures all components use the SAME session storage
        let session_store_shared = {
            let path = std::path::PathBuf::from(session_path.clone());
            // Create it synchronously here (PersistentSessionStore::new is sync)
            match PersistentSessionStore::new(path) {
                Ok(store) => Arc::new(store) as Arc<dyn std::any::Any + Send + Sync>,
                Err(e) => {
                    log::error!("Failed to create session store: {}", e);
                    panic!("Cannot start without session store: {}", e);
                }
            }
        };

        // Store session manager AND permission checker for use by models
        self.session_manager = Some(session_store_shared.clone());
        self.permission_checker = Some(permission_checker);

        // Add login route
        let session_store_login = session_store_shared.clone();
        #[cfg(feature = "mfa")]
        let mfa_storage_login = self.mfa_storage.clone();
        #[cfg(not(feature = "mfa"))]
        let mfa_storage_login: Option<()> = None;
        self = self.with_route(http::Method::POST, "/auth/login", move |req| {
            let users = users_login.clone();
            let duration = session_duration;
            let store_clone = session_store_login.clone();
            #[allow(clippy::clone_on_copy)] // Type changes based on feature flag
            let mfa_clone = mfa_storage_login.clone();

            Box::pin(async move {
                // Use shared session store (already created above)
                let session_store: Arc<PersistentSessionStore> = store_clone
                    .downcast()
                    .map_err(|_| anyhow::anyhow!("Failed to downcast session store"))?;

                match handle_rbac_login(req, session_store, &users, duration, mfa_clone).await {
                    Ok(resp) => {
                        use http_body_util::BodyExt;
                        let (parts, body) = resp.into_parts();
                        Ok(hyper::Response::from_parts(parts, body.boxed()))
                    }
                    Err(e) => {
                        log::error!("Login error: {}", e);
                        Ok(hyper::Response::builder()
                            .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                            .body(super::boxed_full(bytes::Bytes::from(format!(
                                "Internal error: {}",
                                e
                            ))))
                            .unwrap())
                    }
                }
            })
        });

        // Add logout route
        let session_store_logout = session_store_shared.clone();
        self = self.with_route(http::Method::POST, "/auth/logout", move |req| {
            let store_clone = session_store_logout.clone();

            Box::pin(async move {
                // Use shared session store
                let session_store: Arc<PersistentSessionStore> = store_clone
                    .downcast()
                    .map_err(|_| anyhow::anyhow!("Failed to downcast session store"))?;

                match handle_rbac_logout(req, session_store).await {
                    Ok(resp) => {
                        use http_body_util::BodyExt;
                        let (parts, body) = resp.into_parts();
                        Ok(hyper::Response::from_parts(parts, body.boxed()))
                    }
                    Err(e) => {
                        log::error!("Logout error: {}", e);
                        Ok(hyper::Response::builder()
                            .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                            .body(super::boxed_full(bytes::Bytes::from(format!(
                                "Internal error: {}",
                                e
                            ))))
                            .unwrap())
                    }
                }
            })
        });

        // Add validate route (GET endpoint for session validation)
        let session_store_validate = session_store_shared.clone();
        self = self.with_route(http::Method::GET, "/auth/validate", move |req| {
            let store_clone = session_store_validate.clone();

            Box::pin(async move {
                use crate::session::SessionStore;
                use bytes::Bytes;

                // Use shared session store
                let session_store: Arc<PersistentSessionStore> = store_clone
                    .downcast()
                    .map_err(|_| anyhow::anyhow!("Failed to downcast session store"))?;

                // Extract token from Authorization header or Cookie
                let token = req
                    .headers()
                    .get(hyper::header::AUTHORIZATION)
                    .and_then(|h| h.to_str().ok())
                    .and_then(|h| h.strip_prefix("Bearer "))
                    .or_else(|| {
                        req.headers()
                            .get(hyper::header::COOKIE)
                            .and_then(|h| h.to_str().ok())
                            .and_then(|cookies| {
                                cookies
                                    .split(';')
                                    .find(|c| c.trim().starts_with("session_token="))
                                    .and_then(|c| c.split('=').nth(1))
                            })
                    });

                let is_valid = if let Some(token) = token {
                    session_store.get(token).await.ok().flatten().is_some()
                } else {
                    false
                };

                Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::OK)
                    .header("Content-Type", "application/json")
                    .body(super::boxed_full(Bytes::from(format!(r#"{{"valid":{}}}"#, is_valid))))
                    .unwrap())
            })
        });

        log::info!(
            "RBAC configured with {} roles and {} users",
            config.roles.len(),
            users_clone.len()
        );
        log::info!("   POST /auth/login - Authentication endpoint");
        log::info!("   POST /auth/logout - Logout endpoint");
        log::info!("   GET /auth/validate - Session validation endpoint");

        self
    }

    /// Configure MFA/TOTP with automatic route generation
    ///
    /// This method automatically:
    /// - Creates MfaStorage for secure secret persistence
    /// - Registers GET /auth/mfa/status - Check MFA status
    /// - Registers POST /auth/mfa/setup - Generate secret + QR code
    /// - Registers POST /auth/mfa/enable - Activate MFA (verify code)
    /// - Registers POST /auth/mfa/disable - Deactivate MFA
    /// - Registers POST /auth/mfa/verify - Validate TOTP code
    ///
    /// # Example
    /// ```ignore
    /// use lithair_core::mfa::MfaConfig;
    ///
    /// LithairServer::new()
    ///     .with_rbac_config(rbac_config)
    ///     .with_mfa_totp(MfaConfig {
    ///         issuer: "Lithair Blog".to_string(),
    ///         enforce_for_roles: vec!["Admin".to_string()],
    ///         ..Default::default()
    ///     })
    ///     .serve()
    ///     .await?;
    /// ```
    #[cfg(feature = "mfa")]
    pub fn with_mfa_totp(mut self, config: crate::mfa::MfaConfig) -> Self {
        use crate::mfa::{handlers, MfaStorage};
        use std::sync::Arc;

        // Create MFA storage
        let storage = MfaStorage::new(&config.storage_path).expect("Failed to create MFA storage");
        let storage_arc = Arc::new(storage);

        // Store for use in other routes
        self.mfa_storage = Some(storage_arc.clone());

        let config_arc = Arc::new(config.clone());

        // Auto-generate MFA routes

        // GET /auth/mfa/status
        let storage_status = storage_arc.clone();
        let config_status = config_arc.clone();
        self = self.with_route(http::Method::GET, "/auth/mfa/status", move |req| {
            let storage = storage_status.clone();
            let config = config_status.clone();
            Box::pin(async move {
                handlers::handle_mfa_status(storage, config, req)
                    .await
                    .map(|resp| {
                        use http_body_util::BodyExt;
                        resp.map(|b| b.boxed())
                    })
                    .map_err(|e| anyhow::anyhow!("MFA status error: {}", e))
            })
        });

        // POST /auth/mfa/setup
        let storage_setup = storage_arc.clone();
        let config_setup = config_arc.clone();
        self = self.with_route(http::Method::POST, "/auth/mfa/setup", move |req| {
            let storage = storage_setup.clone();
            let config = config_setup.clone();
            Box::pin(async move {
                handlers::handle_mfa_setup(storage, config, req)
                    .await
                    .map(|resp| {
                        use http_body_util::BodyExt;
                        resp.map(|b| b.boxed())
                    })
                    .map_err(|e| anyhow::anyhow!("MFA setup error: {}", e))
            })
        });

        // POST /auth/mfa/enable
        let storage_enable = storage_arc.clone();
        let config_enable = config_arc.clone();
        self = self.with_route(http::Method::POST, "/auth/mfa/enable", move |req| {
            let storage = storage_enable.clone();
            let config = config_enable.clone();
            Box::pin(async move {
                handlers::handle_mfa_enable(storage, config, req)
                    .await
                    .map(|resp| {
                        use http_body_util::BodyExt;
                        resp.map(|b| b.boxed())
                    })
                    .map_err(|e| anyhow::anyhow!("MFA enable error: {}", e))
            })
        });

        // POST /auth/mfa/disable
        let storage_disable = storage_arc.clone();
        let config_disable = config_arc.clone();
        self = self.with_route(http::Method::POST, "/auth/mfa/disable", move |req| {
            let storage = storage_disable.clone();
            let config = config_disable.clone();
            Box::pin(async move {
                handlers::handle_mfa_disable(storage, config, req)
                    .await
                    .map(|resp| {
                        use http_body_util::BodyExt;
                        resp.map(|b| b.boxed())
                    })
                    .map_err(|e| anyhow::anyhow!("MFA disable error: {}", e))
            })
        });

        // POST /auth/mfa/verify
        let storage_verify = storage_arc.clone();
        let config_verify = config_arc.clone();
        self = self.with_route(http::Method::POST, "/auth/mfa/verify", move |req| {
            let storage = storage_verify.clone();
            let config = config_verify.clone();
            Box::pin(async move {
                handlers::handle_mfa_verify(storage, config, req)
                    .await
                    .map(|resp| {
                        use http_body_util::BodyExt;
                        resp.map(|b| b.boxed())
                    })
                    .map_err(|e| anyhow::anyhow!("MFA verify error: {}", e))
            })
        });

        log::info!("MFA/TOTP configured");
        log::info!("   Issuer: {}", config.issuer);
        log::info!("   GET /auth/mfa/status - Check MFA status");
        log::info!("   POST /auth/mfa/setup - Generate secret + QR code");
        log::info!("   POST /auth/mfa/enable - Activate MFA");
        log::info!("   POST /auth/mfa/disable - Deactivate MFA");
        log::info!("   POST /auth/mfa/verify - Validate TOTP code");
        log::info!("   Storage: {}", config.storage_path);

        self
    }

    /// Serve static files from a directory at root path (memory-first with SCC2)
    ///
    /// This loads all files from the specified directory into memory
    /// and serves them with ultra-low latency (40M+ ops/sec) at the root path "/".
    ///
    /// # Example
    /// ```ignore
    /// LithairServer::new()
    ///     .with_frontend("public")  // Serves public/ from memory at /
    ///     .serve()
    ///     .await?;
    /// ```
    pub fn with_frontend(mut self, static_dir: impl Into<String>) -> Self {
        // Store the static directory path and enable frontend (legacy support)
        self.config.frontend.static_dir = Some(static_dir.into());
        self.config.frontend.enabled = true;
        self
    }

    /// Serve static files from a directory at a specific path prefix (memory-first with SCC2)
    ///
    /// This loads all files from the specified directory into memory
    /// and serves them with ultra-low latency (40M+ ops/sec) at the specified route prefix.
    ///
    /// You can call this multiple times to serve different frontends at different paths.
    ///
    /// # Arguments
    /// * `route_prefix` - URL path prefix (e.g., "/", "/admin", "/dashboard")
    /// * `static_dir` - Filesystem directory containing static files
    ///
    /// # Example
    /// ```ignore
    /// LithairServer::new()
    ///     .with_frontend_at("/", "public")         // Public frontend at /
    ///     .with_frontend_at("/admin", "admin-ui")  // Admin frontend at /admin/*
    ///     .serve()
    ///     .await?;
    /// ```
    pub fn with_frontend_at(
        mut self,
        route_prefix: impl Into<String>,
        static_dir: impl Into<String>,
    ) -> Self {
        self.frontend_configs.push((route_prefix.into(), static_dir.into()));
        self
    }

    // ========================================================================
    // VIRTUAL HOSTS (Host-header based routing)
    // ========================================================================
    //
    // See `lithair_core::http::host_router` for the underlying primitive.
    //
    // The vhost API is additive: existing apps that only use
    // `.with_frontend_at(...)` continue to serve the same content on every
    // Host. The vhost-scoped configs are consulted *first* at dispatch time;
    // if none match and no default vhost is configured, we fall through to
    // the host-agnostic frontends and the rest of the routing pipeline.

    /// Declare a virtual host bound to a specific `Host:` header value.
    ///
    /// The provided closure receives a [`VhostBuilder`] with a subset of
    /// the `LithairServerBuilder` API (currently: per-host frontends). All
    /// configuration declared inside the closure will only respond to
    /// requests whose `Host:` header matches `host`.
    ///
    /// Matching is case-insensitive and strips the `:port` suffix — see
    /// [`crate::http::host_router`] for the exact semantics.
    ///
    /// # Example
    ///
    /// ```ignore
    /// LithairServer::new()
    ///     .with_vhost("arcker.org",  |v| v.with_frontend_at("/", "sites/arcker.org"))
    ///     .with_vhost("lithair.net", |v| v.with_frontend_at("/", "public"))
    ///     .serve()
    ///     .await?;
    /// ```
    pub fn with_vhost<F>(mut self, host: impl Into<String>, configure: F) -> Self
    where
        F: FnOnce(VhostBuilder) -> VhostBuilder,
    {
        let scope = VhostScope::Host(crate::http::normalize_host(&host.into()));
        let vhost = configure(VhostBuilder::new(scope.clone()));
        for (prefix, dir) in vhost.frontend_configs {
            self.vhost_frontend_configs.push((scope.clone(), prefix, dir));
        }
        self
    }

    /// Declare the default virtual host, used when the request's `Host:`
    /// header does not match any vhost registered via
    /// [`Self::with_vhost`].
    ///
    /// Without a default vhost, requests for unknown hosts fall through
    /// to the host-agnostic routes (`.with_frontend_at`, custom routes,
    /// models). A default vhost is only useful if you want different
    /// content served to unknown hosts than to the known ones — for
    /// example a redirect-to-canonical page.
    ///
    /// Only one default vhost is supported; later calls overwrite
    /// earlier ones.
    ///
    /// # Example
    ///
    /// ```ignore
    /// LithairServer::new()
    ///     .with_vhost("lithair.net", |v| v.with_frontend_at("/", "public"))
    ///     .with_default_vhost(|v| v.with_frontend_at("/", "redirect"))
    ///     .serve()
    ///     .await?;
    /// ```
    pub fn with_default_vhost<F>(mut self, configure: F) -> Self
    where
        F: FnOnce(VhostBuilder) -> VhostBuilder,
    {
        // Remove any previously-declared default entries.
        self.vhost_frontend_configs
            .retain(|(scope, _, _)| !matches!(scope, VhostScope::Default));
        let vhost = configure(VhostBuilder::new(VhostScope::Default));
        for (prefix, dir) in vhost.frontend_configs {
            self.vhost_frontend_configs.push((VhostScope::Default, prefix, dir));
        }
        self
    }

    /// Declare a permanent (`301 Moved Permanently`) redirect from one
    /// host to another, preserving the request path and query string.
    ///
    /// Typical use-case: canonical-URL hygiene — sending `www.example.com`
    /// traffic to the bare `example.com`, or the other way around. The
    /// redirect is matched on the request's `Host:` header *before* any
    /// vhost frontend lookup, so it short-circuits the normal vhost
    /// dispatch entirely.
    ///
    /// Both arguments are normalized via [`crate::http::normalize_host`]
    /// (case-insensitive, port stripped, trailing dot removed). Matching
    /// is case-insensitive and applies to **all HTTP methods** — clients
    /// may follow redirects on any verb.
    ///
    /// # Loop guard
    ///
    /// Self-redirects (`with_redirect("a", "a")` after normalization) are
    /// rejected at registration time: the entry is **not** stored and a
    /// `log::warn!` is emitted. This prevents the most common foot-gun —
    /// a config pass that ends up pointing a host at itself — without
    /// requiring callers to wrap the call in a `Result`. Cross-host loops
    /// (`a -> b -> a`) are still the caller's responsibility.
    ///
    /// # Status code: 301 vs 308
    ///
    /// We emit `301 Moved Permanently`. RFC 7231 historically allowed
    /// clients to rewrite the method to `GET` on 301, which is why
    /// `308 Permanent Redirect` (RFC 7538) was introduced for cases that
    /// must preserve the verb. In practice all modern clients preserve
    /// the method on 301 too, and 301 is the universally-understood
    /// canonical-URL signal for caches, CDNs, and SEO tooling. If your
    /// use-case strictly requires method preservation by spec rather
    /// than by convention, fall back to [`Self::with_route`] and emit
    /// 308 yourself.
    ///
    /// # Scheme
    ///
    /// The generated `Location:` header always uses `https://`. This
    /// matches the dominant deployment shape (TLS in front of the server)
    /// and avoids accidental downgrades. If you need an HTTP target,
    /// implement the redirect with [`Self::with_route`] instead.
    ///
    /// # Interaction with vhosts
    ///
    /// A host listed as a redirect source should not also have a vhost
    /// (`with_vhost("www.example.com", ...)`) — the redirect runs first
    /// and the vhost would be unreachable. The opposite is fine: the
    /// target host typically has its own vhost serving real content.
    ///
    /// # Example
    ///
    /// ```ignore
    /// LithairServer::new()
    ///     .with_vhost("example.com", |v| v.with_frontend_at("/", "public"))
    ///     // Canonical-URL: send www. traffic to the bare host.
    ///     .with_redirect("www.example.com", "example.com")
    ///     .serve()
    ///     .await?;
    /// ```
    pub fn with_redirect(mut self, from: impl Into<String>, to: impl Into<String>) -> Self {
        let from_norm = crate::http::normalize_host(&from.into());
        let to_norm = crate::http::normalize_host(&to.into());

        // Empty-host guard: `normalize_host` only trims/lowercases — it does
        // not reject malformed inputs. An empty `from` would register an
        // empty-string key in the router, which `host_from_request` returns
        // when the request has neither a URI authority nor a `Host:` header.
        // That's a silent foot-gun: every host-less request would suddenly
        // 301 to whatever was configured. Refuse both ends. Returning
        // `Self` (not `Result`) keeps the builder ergonomic, so we log
        // and skip.
        if from_norm.is_empty() || to_norm.is_empty() {
            log::warn!(
                "with_redirect: ignoring redirect with empty host (from='{}', to='{}')",
                from_norm,
                to_norm
            );
            return self;
        }

        // Loop guard: a host pointing at itself would yield a 301 → same
        // host → 301 → ... loop until the client gives up. Refuse the
        // entry rather than silently shipping a broken config.
        if from_norm == to_norm {
            log::warn!(
                "with_redirect: ignoring self-redirect for host '{}' (would loop)",
                from_norm
            );
            return self;
        }

        // Last-write-wins on the same source host, mirroring the
        // semantics of `HostRouter::insert`.
        self.host_redirects.retain(|(f, _)| f != &from_norm);
        self.host_redirects.push((from_norm, to_norm));
        self
    }

    // ========================================================================
    // ROUTES & MODELS
    // ========================================================================

    /// Register an existing DeclarativeHttpHandler with automatic CRUD route generation
    ///
    /// This method automatically registers all CRUD routes for the given handler:
    /// - `GET /base_path` - List all items
    /// - `POST /base_path` - Create new item
    /// - `GET /base_path/*` - Get single item by ID
    /// - `PUT /base_path/*` - Update item by ID
    /// - `DELETE /base_path/*` - Delete item by ID
    ///
    /// Unlike `with_model<T>`, this method allows you to keep a reference to the handler
    /// for use in custom routes (e.g., FK queries, JOIN queries).
    ///
    /// # Session gating
    ///
    /// When the builder has `with_models_require_session(true)` set, handlers
    /// registered here are gated identically to those registered via
    /// `with_model<T>`: every non-OPTIONS request to the auto-generated
    /// `/api/{model}` endpoints must carry a valid session, otherwise the
    /// handler returns HTTP 401. The order of `.with_handler(...)`,
    /// `.with_sessions(...)`, and `.with_models_require_session(...)` on the
    /// chain does not matter — the gate is applied at `serve()` time, after
    /// all builder methods have run (issue #86).
    ///
    /// Note: this path does NOT auto-wire the builder-level session store onto
    /// the externally-constructed handler — that responsibility stays with
    /// the caller. If you want positive-path session validation (valid token
    /// → 200) and not just gate-fail (no token → 401), call
    /// `handler.with_session_store(...)` on your handler before passing it to
    /// `with_handler`.
    ///
    /// # Example
    /// ```ignore
    /// use lithair_core::http::DeclarativeHttpHandler;
    /// use std::sync::Arc;
    ///
    /// let products_handler = Arc::new(DeclarativeHttpHandler::<Product>::new_with_replay("./data/products")?);
    /// let orders_handler = Arc::new(DeclarativeHttpHandler::<Order>::new_with_replay("./data/orders")?);
    ///
    /// // Clone handlers before passing to with_handler (it takes ownership via move)
    /// let products_for_custom = products_handler.clone();
    ///
    /// LithairServer::new()
    ///     .with_handler(products_handler, "/api/products")
    ///     .with_handler(orders_handler.clone(), "/api/orders")
    ///     // Custom routes using the cloned handler reference
    ///     .with_route(GET, "/api/orders/*/expanded", move |req| {
    ///         let handler = orders_handler.clone();
    ///         // ... use handler for custom queries
    ///     })
    ///     .serve()
    ///     .await?;
    /// ```
    pub fn with_handler<T>(
        mut self,
        handler: std::sync::Arc<crate::http::DeclarativeHttpHandler<T>>,
        base_path: impl Into<String>,
    ) -> Self
    where
        T: Clone
            + Send
            + Sync
            + crate::http::HttpExposable
            + crate::lifecycle::LifecycleAware
            + crate::consensus::ReplicatedModel
            + crate::lifecycle::RetentionAware
            + 'static,
    {
        let base_path_str = base_path.into();
        let base_path_normalized = base_path_str.trim_end_matches('/').to_string();

        // Issue #86: register a deferred session-gate applier so that
        // `with_models_require_session(true)` actually engages this
        // handler's `/api/{model}` routes. Pre-fix, `with_handler` wired
        // its CRUD routes via raw `with_route(...)` calls that internally
        // called `handler.handle_request(...)` — and that path *does*
        // honor `require_session`, but the flag was never flipped, so
        // the gate silently never fired. Now we capture an Arc clone here
        // and let `serve()` flip the flag at startup once all builder
        // methods have been chained (so order of `with_handler` vs
        // `with_models_require_session` no longer matters).
        //
        // The flag flip goes through `AtomicBool` (interior mutability)
        // because the caller has already received their own Arc clone
        // from outside the builder, so `Arc::get_mut` would fail here.
        let gate_handler = handler.clone();
        self.external_handler_gates.push(Box::new(move |require| {
            gate_handler.set_require_session(require);
        }));

        // Issue #91: register a deferred SSE-broadcaster wiring closure so
        // `LithairServer::serve()` can install the builder-level broadcaster
        // onto this handler. Pre-fix, the broadcaster was only attached on
        // the `with_model` factory path (`app/mod.rs:776` via
        // `Arc::get_mut` + `ModelHandler::set_sse_broadcaster`). Handlers
        // registered through `with_handler` (and therefore through
        // `with_model_ref`, which delegates to `with_handler`) started with
        // an empty `sse_broadcaster` `OnceLock` regardless of whether
        // `.with_sse(true)` was called — so `apply_replicated_*` broadcasts
        // from issue #89 became no-ops, and `GET /api/{model}/stream`
        // returned 404 ("SSE not enabled") even when SSE was enabled on
        // the server.
        //
        // The install goes through `OnceLock::set` (interior mutability,
        // same shape as the `AtomicBool` for `require_session`) so it
        // works through a shared `Arc<DeclarativeHttpHandler<T>>` — which
        // is exactly the state we're in here, since the caller already
        // holds an Arc clone.
        //
        // The closure is only invoked at `serve()` time if the builder
        // has an SSE broadcaster (`with_sse(true)` was called). Backward-
        // compat for existing `with_handler` consumers who never enable
        // SSE: no wiring fires, the handler stays at `OnceLock::new()`,
        // behavior is byte-for-byte identical to v0.9.1.
        //
        // Note: the `/api/{model}/stream` HTTP route does NOT need a
        // separate route registration. The wildcard `GET {base_path}/*`
        // route below already forwards to `handle_request`, which
        // dispatches `(GET, segments=["stream"])` to `handle_sse_stream`
        // (`declarative.rs:918`). Once the broadcaster is wired,
        // `handle_sse_stream` switches from a 404 ("SSE not enabled") to
        // the streaming response. Verified by the end-to-end test in
        // `tests/issue_91_with_handler_sse_e2e_test.rs`.
        let sse_handler = handler.clone();
        self.external_handler_sse_wirings.push(Box::new(move |broadcaster| {
            // Idempotent — if a broadcaster was somehow already installed
            // (e.g. the caller used `with_sse_broadcaster` before passing
            // the handler in), the `OnceLock` keeps the first one and
            // this call is a silent no-op. That's the right semantics:
            // one broadcaster per handler, lifetime-of-process.
            let _ = sse_handler.set_sse_broadcaster_shared(broadcaster);
        }));

        // GET /base_path - List all
        let handler_list = handler.clone();
        let path_list = base_path_normalized.clone();
        self = self.with_route(http::Method::GET, base_path_normalized.clone(), move |req| {
            let h = handler_list.clone();
            let bp = path_list.clone();
            Box::pin(async move {
                let segments: Vec<&str> = Vec::new();
                match h.handle_request(req, &segments).await {
                    Ok(resp) => Ok(resp),
                    Err(_infallible) => {
                        log::error!("Handler error for GET {}", bp);
                        Ok(hyper::Response::builder()
                            .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                            .body(super::boxed_full(bytes::Bytes::from("Internal error")))
                            .unwrap())
                    }
                }
            })
        });

        // POST /base_path - Create
        let handler_create = handler.clone();
        let path_create = base_path_normalized.clone();
        self = self.with_route(http::Method::POST, base_path_normalized.clone(), move |req| {
            let h = handler_create.clone();
            let bp = path_create.clone();
            Box::pin(async move {
                let segments: Vec<&str> = Vec::new();
                match h.handle_request(req, &segments).await {
                    Ok(resp) => Ok(resp),
                    Err(_infallible) => {
                        log::error!("Handler error for POST {}", bp);
                        Ok(hyper::Response::builder()
                            .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                            .body(super::boxed_full(bytes::Bytes::from("Internal error")))
                            .unwrap())
                    }
                }
            })
        });

        // GET /base_path/* - Get single (also handles /stream for SSE)
        let handler_get = handler.clone();
        let base_for_get = base_path_normalized.clone();
        self =
            self.with_route(http::Method::GET, format!("{}/*", base_path_normalized), move |req| {
                let h = handler_get.clone();
                let bp = base_for_get.clone();
                Box::pin(async move {
                    // Extract path segment (ID) from URL
                    let path = req.uri().path().to_string();
                    let segments: Vec<&str> = path
                        .strip_prefix(&bp)
                        .unwrap_or("")
                        .trim_start_matches('/')
                        .split('/')
                        .filter(|s| !s.is_empty())
                        .collect();

                    match h.handle_request(req, &segments).await {
                        Ok(resp) => Ok(resp),
                        Err(_infallible) => {
                            log::error!("Handler error for GET {}/*", bp);
                            Ok(hyper::Response::builder()
                                .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                                .body(super::boxed_full(bytes::Bytes::from("Internal error")))
                                .unwrap())
                        }
                    }
                })
            });

        // PUT /base_path/* - Update
        let handler_put = handler.clone();
        let base_for_put = base_path_normalized.clone();
        self =
            self.with_route(http::Method::PUT, format!("{}/*", base_path_normalized), move |req| {
                let h = handler_put.clone();
                let bp = base_for_put.clone();
                Box::pin(async move {
                    let path = req.uri().path().to_string();
                    let segments: Vec<&str> = path
                        .strip_prefix(&bp)
                        .unwrap_or("")
                        .trim_start_matches('/')
                        .split('/')
                        .filter(|s| !s.is_empty())
                        .collect();

                    match h.handle_request(req, &segments).await {
                        Ok(resp) => Ok(resp),
                        Err(_infallible) => {
                            log::error!("Handler error for PUT {}/*", bp);
                            Ok(hyper::Response::builder()
                                .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                                .body(super::boxed_full(bytes::Bytes::from("Internal error")))
                                .unwrap())
                        }
                    }
                })
            });

        // DELETE /base_path/* - Delete
        let handler_del = handler.clone();
        let base_for_del = base_path_normalized.clone();
        self = self.with_route(
            http::Method::DELETE,
            format!("{}/*", base_path_normalized),
            move |req| {
                let h = handler_del.clone();
                let bp = base_for_del.clone();
                Box::pin(async move {
                    let path = req.uri().path().to_string();
                    let segments: Vec<&str> = path
                        .strip_prefix(&bp)
                        .unwrap_or("")
                        .trim_start_matches('/')
                        .split('/')
                        .filter(|s| !s.is_empty())
                        .collect();

                    match h.handle_request(req, &segments).await {
                        Ok(resp) => Ok(resp),
                        Err(_infallible) => {
                            log::error!("Handler error for DELETE {}/*", bp);
                            Ok(hyper::Response::builder()
                                .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                                .body(super::boxed_full(bytes::Bytes::from("Internal error")))
                                .unwrap())
                        }
                    }
                })
            },
        );

        let name = std::any::type_name::<T>().split("::").last().unwrap_or("Unknown");
        log::info!("Registered handler for {} at {}", name, base_path_normalized);
        log::info!("   GET {} - List all", base_path_normalized);
        log::info!("   POST {} - Create", base_path_normalized);
        log::info!("   GET {}/* - Get by ID", base_path_normalized);
        log::info!("   PUT {}/* - Update by ID", base_path_normalized);
        log::info!("   DELETE {}/* - Delete by ID", base_path_normalized);

        self
    }

    /// Register a model with automatic CRUD generation **and** return an
    /// `Arc<DeclarativeHttpHandler<T>>` so the caller can drive the model
    /// programmatically (background workers, OAuth callbacks, scheduled
    /// jobs, server-startup seeding) — without giving up the session gate
    /// that `with_models_require_session(true)` provides.
    ///
    /// This is the missing "both at once" path between `with_model::<T>(...)`
    /// (gated CRUD, no handle) and `with_handler(arc, ...)` (handle, no
    /// auto-wired session store and historically — pre-#86 — no gate either).
    /// See issue #85 for the motivating use case (LensMail IMAP sync worker
    /// and Gmail OAuth callback both needing programmatic write access to
    /// session-gated models).
    ///
    /// The chain breaks at this method (it returns a tuple), but the
    /// returned builder can be re-bound and continue:
    ///
    /// ```ignore
    /// let (builder, mail_handler) = LithairServer::new()
    ///     .with_sessions(session_manager)
    ///     .with_models_require_session(true)
    ///     .with_model_ref::<Mail>("./data/mails", "/api/mails")
    ///     .await?;
    ///
    /// // mail_handler.apply_replicated_item(...).await — usable anywhere
    /// // (background sync worker, OAuth callback, scheduled job).
    ///
    /// builder
    ///     .with_route_async(Method::POST, "/admin/import", { /* ... */ })
    ///     .serve()
    ///     .await?;
    /// ```
    ///
    /// # Wiring
    ///
    /// Unlike `with_handler`, `with_model_ref` constructs the handler
    /// internally, so it can wire everything uniformly:
    ///
    /// - **Session store**: the builder-level session store (set via
    ///   `with_sessions(...)`) is attached to the handler before the Arc
    ///   is shared, so positive-path session validation (valid token →
    ///   200) works out of the box. With `with_handler`, the caller has to
    ///   wire the store themselves.
    /// - **Session gate**: `with_models_require_session(true)` engages this
    ///   handler's `/api/{model}` routes identically to `with_model`. The
    ///   flag is applied at `serve()` time through the same deferred
    ///   applier mechanism used by `with_handler` (issue #86), so the
    ///   order of `.with_model_ref(...)`, `.with_sessions(...)`, and
    ///   `.with_models_require_session(...)` does not matter.
    ///
    /// # Async
    ///
    /// `with_model_ref` is `async` because `DeclarativeHttpHandler::new_with_replay`
    /// performs I/O (replay of the persisted event log to reconstruct
    /// in-memory state). The fallible `Result` covers event-store I/O
    /// errors at construction time — the same failures `with_model`'s
    /// factory hits, but surfaced eagerly at registration rather than
    /// deferred to `serve()`.
    ///
    /// # When to use what
    ///
    /// - **`with_model::<T>(...)`** — gated CRUD, no programmatic handle needed.
    /// - **`with_model_ref::<T>(...)`** — gated CRUD + programmatic handle (this method).
    /// - **`with_handler(arc, ...)`** — caller fully owns construction
    ///   (e.g. custom permission extractor, custom session store, sharing
    ///   the same handler across multiple base paths). The caller is
    ///   responsible for wiring the session store onto the handler before
    ///   passing it in.
    /// - **`with_model_full::<T>(...)`** — RBAC path (separate concern;
    ///   not covered by `with_models_require_session`).
    pub async fn with_model_ref<T>(
        mut self,
        data_path: impl Into<String>,
        base_path: impl Into<String>,
    ) -> Result<(Self, std::sync::Arc<crate::http::DeclarativeHttpHandler<T>>)>
    where
        T: Clone
            + Send
            + Sync
            + crate::http::HttpExposable
            + crate::lifecycle::LifecycleAware
            + crate::consensus::ReplicatedModel
            + crate::lifecycle::RetentionAware
            + 'static,
    {
        let data_path_str = data_path.into();

        // Construct the handler eagerly (mirrors `with_model`'s factory
        // body, but runs here rather than deferred to `serve()`). We need
        // the handle in our caller's hands before `.serve()` is invoked.
        let mut handler = crate::http::DeclarativeHttpHandler::<T>::new_with_replay(&data_path_str)
            .await
            .map_err(|e| anyhow::anyhow!("with_model_ref: failed to create handler: {}", e))?;

        // Wire the builder-level session store onto the handler *before*
        // wrapping in Arc, so it's attached even after the Arc is cloned
        // five times into the route closures. This is exactly the wiring
        // that `LithairServer::serve()` performs for `with_model`
        // registrations via the `ModelHandler::set_session_store_any`
        // trait method — but for this path we do it here because we own
        // the handler's `&mut self` ergonomics at this moment, and once
        // it's in an Arc we'd need `Arc::get_mut` (which would race with
        // our caller's returned clone).
        //
        // If the builder has no session store registered (e.g. the user
        // hasn't called `with_sessions(...)` yet, or never will), the
        // handler simply has `session_store == None`, and the gate (if
        // ever turned on) will fail closed — same as `with_handler`.
        if let Some(ref store_any) = self.session_manager {
            handler.session_store = Some(store_any.clone());
        }

        let handler_arc = std::sync::Arc::new(handler);

        // Reuse `with_handler` for route registration and the
        // issue-#86 gate-applier mechanism. This guarantees the
        // `with_model_ref` path stays gate-correct as long as
        // `with_handler` does — one code path, one regression surface.
        self = self.with_handler(handler_arc.clone(), base_path);

        Ok((self, handler_arc))
    }

    /// Add a custom route with an async handler.
    ///
    /// Custom routes are dispatched **before** model routes, so a custom
    /// `/api/items/search` will take priority over the DeclarativeModel
    /// prefix match on `/api/items`.
    ///
    /// The handler receives a [`RouteRequest`] and returns a
    /// `Result<`[`RouteResponse`]`>`. Both are public type aliases
    /// re-exported from [`crate::app`], so consumers don't need to depend on
    /// `hyper`, `http-body-util`, or `bytes` directly to type their handlers.
    ///
    /// `with_route` requires the handler to return a manually pinned future
    /// (`Box::pin(async move { ... })`). For the common case where you just
    /// want to write `|req| async move { ... }`, prefer
    /// [`Self::with_route_async`].
    ///
    /// # Example
    /// ```ignore
    /// use lithair_core::app::{Method, RouteRequest, RouteResponse, StatusCode, response};
    ///
    /// fn health(
    ///     _req: RouteRequest,
    /// ) -> std::pin::Pin<Box<dyn std::future::Future<
    ///     Output = anyhow::Result<RouteResponse>,
    /// > + Send>> {
    ///     Box::pin(async {
    ///         Ok(response::json(StatusCode::OK, r#"{"status":"ok"}"#))
    ///     })
    /// }
    ///
    /// LithairServer::new()
    ///     .with_route(Method::GET, "/health", health)
    ///     .serve()
    ///     .await?;
    /// ```
    pub fn with_route<F>(
        mut self,
        method: http::Method,
        path: impl Into<String>,
        handler: F,
    ) -> Self
    where
        F: Fn(
                RouteRequest,
            )
                -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<RouteResponse>> + Send>>
            + Send
            + Sync
            + 'static,
    {
        self.custom_routes.push(CustomRoute {
            method,
            path: path.into(),
            handler: Arc::new(handler),
        });
        self
    }

    /// Add a custom route with an async-closure handler — no `Box::pin`
    /// boilerplate required.
    ///
    /// This is the ergonomic counterpart of [`Self::with_route`]: the handler
    /// is an `async` closure (or a regular closure returning a future), and
    /// the builder applies the `Box::pin` internally. Behaviour is
    /// identical — `with_route_async` forwards into the same dispatch path,
    /// preserving the "custom routes win over model and ops endpoints"
    /// precedence.
    ///
    /// Use [`Self::with_route`] when you need to construct the future from
    /// outside (e.g. an existing pinned future, or a free `fn` that already
    /// returns `Pin<Box<dyn Future<...>>>`). Use `with_route_async` for everything
    /// else.
    ///
    /// # Example
    /// ```ignore
    /// use lithair_core::app::{LithairServer, Method, RouteRequest, StatusCode, response};
    ///
    /// LithairServer::new()
    ///     .with_route_async(Method::POST, "/api/jobs/:name/run", |_req: RouteRequest| async move {
    ///         Ok(response::json(StatusCode::ACCEPTED, r#"{"status":"queued"}"#))
    ///     })
    ///     .serve()
    ///     .await?;
    /// ```
    pub fn with_route_async<F, Fut>(
        self,
        method: http::Method,
        path: impl Into<String>,
        handler: F,
    ) -> Self
    where
        F: Fn(RouteRequest) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<RouteResponse>> + Send + 'static,
    {
        self.with_route(method, path, move |req| {
            Box::pin(handler(req))
                as std::pin::Pin<
                    Box<dyn std::future::Future<Output = Result<RouteResponse>> + Send>,
                >
        })
    }

    /// Set a custom handler for 404 Not Found responses.
    ///
    /// When set, this handler is called instead of the default JSON 404 response
    /// whenever no route matches the incoming request.
    ///
    /// The handler has the same signature as `with_route` handlers.
    ///
    /// # Example
    /// ```ignore
    /// use lithair_core::app::response;
    /// use http::StatusCode;
    ///
    /// LithairServer::new()
    ///     .with_not_found_handler(|_req| {
    ///         Box::pin(async {
    ///             Ok(response::html(StatusCode::NOT_FOUND, "<h1>Page not found</h1>"))
    ///         })
    ///     })
    ///     .serve()
    ///     .await?;
    /// ```
    pub fn with_not_found_handler<F>(mut self, handler: F) -> Self
    where
        F: Fn(
                RouteRequest,
            )
                -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<RouteResponse>> + Send>>
            + Send
            + Sync
            + 'static,
    {
        self.not_found_handler = Some(Arc::new(handler));
        self
    }

    /// Set a 404 handler from an async closure — no `Box::pin` boilerplate.
    ///
    /// Ergonomic counterpart of [`Self::with_not_found_handler`], mirroring
    /// the [`Self::with_route`] → [`Self::with_route_async`] pairing from
    /// v0.4.0 (#59, #60). The handler is a plain `async` closure (or a
    /// regular closure returning a future), and the builder applies the
    /// `Box::pin` internally.
    ///
    /// Use [`Self::with_not_found_handler`] when you need to construct the
    /// future from outside (an existing pinned future, or a free `fn` that
    /// already returns `Pin<Box<dyn Future<...>>>`). Use this for everything
    /// else.
    ///
    /// # Example
    /// ```ignore
    /// use lithair_core::app::{LithairServer, RouteRequest, StatusCode, response};
    /// use serde_json::json;
    ///
    /// LithairServer::new()
    ///     .with_not_found_handler_async(|req: RouteRequest| async move {
    ///         let path = req.uri().path().to_string();
    ///         Ok(response::json_value(
    ///             StatusCode::NOT_FOUND,
    ///             &json!({"error": "not_found", "path": path}),
    ///         ))
    ///     })
    ///     .serve()
    ///     .await?;
    /// ```
    pub fn with_not_found_handler_async<F, Fut>(self, handler: F) -> Self
    where
        F: Fn(RouteRequest) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<RouteResponse>> + Send + 'static,
    {
        self.with_not_found_handler(move |req| {
            Box::pin(handler(req))
                as std::pin::Pin<
                    Box<dyn std::future::Future<Output = Result<RouteResponse>> + Send>,
                >
        })
    }

    /// Add declarative route guard for authentication, authorization, etc.
    ///
    /// # Example
    /// ```ignore
    /// .with_route_guard("/admin/*", RouteGuard::RequireAuth {
    ///     redirect_to: Some("/login".to_string()),
    ///     exclude: vec!["/login".to_string()],
    /// })
    /// ```
    pub fn with_route_guard(
        mut self,
        pattern: impl Into<String>,
        guard: crate::http::RouteGuard,
    ) -> Self {
        self.route_guards.push(crate::http::RouteGuardMatcher {
            pattern: pattern.into(),
            methods: None,
            guard,
        });
        self
    }

    /// Add declarative route guard with specific HTTP methods
    pub fn with_route_guard_methods(
        mut self,
        pattern: impl Into<String>,
        methods: Vec<http::Method>,
        guard: crate::http::RouteGuard,
    ) -> Self {
        self.route_guards.push(crate::http::RouteGuardMatcher {
            pattern: pattern.into(),
            methods: Some(methods),
            guard,
        });
        self
    }

    /// Register a model with automatic CRUD generation
    pub fn with_model_full<T>(
        mut self,
        data_path: impl Into<String>,
        base_path: impl Into<String>,
        permission_checker: Option<Arc<dyn crate::rbac::PermissionChecker>>,
        session_store: Option<Arc<dyn std::any::Any + Send + Sync>>,
    ) -> Self
    where
        T: crate::http::HttpExposable
            + crate::lifecycle::LifecycleAware
            + crate::consensus::ReplicatedModel
            + crate::lifecycle::RetentionAware
            + 'static,
    {
        use crate::app::{DeclarativeModelHandler, ModelRegistrationInfo};
        use std::sync::Arc;

        let name = std::any::type_name::<T>().split("::").last().unwrap_or("Unknown");
        let data_path_str = data_path.into();
        let base_path_str = base_path.into();

        // Use provided OR fallback to builder's values
        let effective_session_store = session_store.or_else(|| self.session_manager.clone());
        let effective_permission_checker =
            permission_checker.or_else(|| self.permission_checker.clone());

        // Create factory that will create the handler async in serve()
        let factory: crate::app::ModelFactory = Arc::new(move |data_path: String| {
            let pc = effective_permission_checker.clone();
            let ss = effective_session_store.clone();
            Box::pin(async move {
                let mut handler = DeclarativeModelHandler::<T>::new(data_path)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to create handler: {}", e))?;

                // Configure permission checker and session store
                if let Some(checker) = pc {
                    handler = handler.with_permission_checker(checker);
                }
                if let Some(store) = ss {
                    handler = handler.set_session_store_any_owned(store);
                }

                Ok(Arc::new(handler) as Arc<dyn crate::app::ModelHandler>)
            })
        });

        self.model_infos.push(ModelRegistrationInfo {
            name: name.to_string(),
            base_path: base_path_str,
            data_path: data_path_str,
            factory,
            schema_extractor: None,
            // `with_model_full` is the RBAC-aware path; the issue #78
            // session gate intentionally does NOT apply here.
            // See `LithairServerBuilder::with_models_require_session`.
            require_session_applies: false,
        });

        self
    }

    /// Register a model with automatic CRUD generation (simple version without RBAC).
    ///
    /// Generates GET, POST, PUT, DELETE endpoints under `base_path`.
    /// Events are persisted to `data_path`.
    ///
    /// For schema migration support, use `with_declarative_model` instead.
    ///
    /// # Example
    /// ```ignore
    /// use lithair_core::app::LithairServer;
    /// use lithair_core::DeclarativeModel;
    /// use serde::{Serialize, Deserialize};
    ///
    /// #[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
    /// struct Todo {
    ///     #[http(expose)]
    ///     id: String,
    ///     #[http(expose)]
    ///     title: String,
    /// }
    ///
    /// LithairServer::new()
    ///     .with_model::<Todo>("./data/todos", "/api/todos")
    ///     .serve()
    ///     .await?;
    /// ```
    pub fn with_model<T>(
        mut self,
        data_path: impl Into<String>,
        base_path: impl Into<String>,
    ) -> Self
    where
        T: crate::http::HttpExposable
            + crate::lifecycle::LifecycleAware
            + crate::consensus::ReplicatedModel
            + crate::lifecycle::RetentionAware
            + 'static,
    {
        use crate::app::{DeclarativeModelHandler, ModelRegistrationInfo};
        use std::sync::Arc;

        let name = std::any::type_name::<T>().split("::").last().unwrap_or("Unknown");
        let data_path_str = data_path.into();
        let base_path_str = base_path.into();

        // Create factory that will create the handler async in serve().
        // Session-store wiring and the issue #78 require-session flag are
        // applied uniformly in `LithairServer::serve()` *after* this factory
        // runs (see `lithair-core/src/app/mod.rs`). Doing it there — rather
        // than capturing the builder state at this moment — means the order
        // of `.with_model::<T>()`, `.with_sessions(...)`, and
        // `.with_models_require_session(...)` no longer matters.
        let factory: crate::app::ModelFactory = Arc::new(move |data_path: String| {
            Box::pin(async move {
                let handler = DeclarativeModelHandler::<T>::new(data_path)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to create handler: {}", e))?;
                Ok(Arc::new(handler) as Arc<dyn crate::app::ModelHandler>)
            })
        });

        self.model_infos.push(ModelRegistrationInfo {
            name: name.to_string(),
            base_path: base_path_str,
            data_path: data_path_str,
            factory,
            schema_extractor: None,
            // Simple-CRUD path — issue #78 gate applies when the builder
            // flag is on.
            require_session_applies: true,
        });

        // Merge model-level #[firewall(...)] config if no explicit config is set
        if self.firewall_config.is_none() {
            if let Some(model_fw) = T::firewall_config() {
                self.firewall_config = Some(model_fw);
            }
        }

        self
    }

    /// Register a DeclarativeModel with schema migration support
    ///
    /// This method enables automatic schema change detection at startup.
    /// If the model's schema has changed since last run, changes are logged
    /// and optionally auto-migrated based on configuration.
    ///
    /// # Example
    /// ```rust,ignore
    /// LithairServer::new()
    ///     .with_port(8080)
    ///     .with_declarative_model::<Product>("./data/products", "/api/products")
    ///     .serve()
    ///     .await?;
    /// ```
    pub fn with_declarative_model<T>(
        mut self,
        data_path: impl Into<String>,
        base_path: impl Into<String>,
    ) -> Self
    where
        T: crate::http::HttpExposable
            + crate::lifecycle::LifecycleAware
            + crate::consensus::ReplicatedModel
            + crate::lifecycle::RetentionAware
            + crate::schema::HasSchemaSpec
            + 'static,
    {
        use crate::app::{DeclarativeModelHandler, ModelRegistrationInfo, SchemaSpecExtractor};
        use std::sync::Arc;

        let name = <T as crate::schema::HasSchemaSpec>::model_name();
        let data_path_str = data_path.into();
        let base_path_str = base_path.into();

        // Session-store and require-session (issue #78) are applied uniformly
        // in `LithairServer::serve()` — see the corresponding comment in
        // `with_model` above.
        let factory: crate::app::ModelFactory = Arc::new(move |data_path: String| {
            Box::pin(async move {
                let handler = DeclarativeModelHandler::<T>::new(data_path)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to create handler: {}", e))?
                    .with_schema_spec(T::schema_spec());
                Ok(Arc::new(handler) as Arc<dyn crate::app::ModelHandler>)
            })
        });

        // Create schema extractor for migration detection
        let schema_extractor: SchemaSpecExtractor = Arc::new(|| T::schema_spec());

        self.model_infos.push(ModelRegistrationInfo {
            name: name.to_string(),
            base_path: base_path_str,
            data_path: data_path_str,
            factory,
            schema_extractor: Some(schema_extractor),
            // Same simple-CRUD path as `with_model`; issue #78 gate applies.
            require_session_applies: true,
        });

        self
    }

    // ========================================================================
    // DATA ADMIN - Database management API for admin dashboards
    // ========================================================================

    /// Enable data admin endpoints for database management
    ///
    /// This adds the following admin endpoints:
    /// - `GET /_admin/data/models` - List all registered models with stats
    /// - `GET /_admin/data/models/{name}` - Get model info and data
    /// - `GET /_admin/data/models/{name}/export` - Export model data as JSON
    /// - `GET /_admin/data/routes` - List all registered API routes
    /// - `POST /_admin/data/backup` - Trigger full data backup
    ///
    /// These endpoints require authentication if RBAC is configured.
    ///
    /// # Example
    /// ```rust,ignore
    /// LithairServer::new()
    ///     .with_model::<Article>("./data/articles", "/api/articles")
    ///     .with_data_admin() // Enable admin endpoints
    ///     .serve()
    ///     .await?;
    /// ```
    pub fn with_data_admin(mut self) -> Self {
        log::info!("Data Admin API enabled");
        log::info!("   GET  /_admin/data/models        - List models");
        log::info!("   GET  /_admin/data/models/{{name}} - Model data");
        log::info!("   GET  /_admin/data/routes        - List routes");
        log::info!("   POST /_admin/data/backup        - Backup all");
        log::info!("   POST /_admin/data/import        - Import a backup");

        // Note: The actual endpoint handlers are registered in LithairServer::serve()
        // after models are initialized. Here we just set a flag.
        self.config.admin.data_admin_enabled = true;

        // Secure by default (issue #143): guard the mutating admin API planes
        // with RequireAuth so a server configured with sessions/RBAC does not
        // expose them unauthenticated. Use `with_data_admin_public()` to opt
        // out for local/dev use.
        self.guard_admin_api_planes();
        log::info!(
            "   Auth protection enabled (RequireAuth) — use with_data_admin_public() to disable"
        );

        self
    }

    /// Enable the Data Admin API WITHOUT the default auth guard
    /// (development / internal use only).
    ///
    /// WARNING: this leaves `/_admin/data/*` and `/_admin/frontend/*` open to
    /// anyone who can reach the server (subject only to the firewall, which is
    /// off by default). Only use behind a VPN/firewall or for local dev. For
    /// production use [`with_data_admin`](Self::with_data_admin), which
    /// auto-applies `RequireAuth`. Mirrors `with_data_admin_ui_public`.
    pub fn with_data_admin_public(mut self) -> Self {
        log::warn!("Data Admin API enabled (NO AUTH GUARD — development only!)");
        log::warn!("   /_admin/data/* and /_admin/frontend/* are exposed without authentication");
        self.config.admin.data_admin_enabled = true;
        self
    }

    /// Register secure-by-default `RequireAuth` guards over the mutating admin
    /// API planes (`/_admin/data/*` and `/_admin/frontend/*`), returning a 401
    /// JSON response rather than a login redirect (these are API surfaces).
    ///
    /// Idempotent: a pattern already guarded — by a prior call, by
    /// `with_data_admin_ui`, or by an explicit user `with_route_guard` — is not
    /// duplicated. When no session store is configured the guard is a no-op
    /// (there is nothing to authenticate against), so this never breaks an
    /// unauthenticated single-user deployment; it closes the hole for the
    /// RBAC/session-secured deployments described in issue #143.
    fn guard_admin_api_planes(&mut self) {
        for pattern in ["/_admin/data/*", "/_admin/frontend/*"] {
            let already = self.route_guards.iter().any(|g| g.pattern == pattern);
            if !already {
                self.route_guards.push(crate::http::RouteGuardMatcher {
                    pattern: pattern.to_string(),
                    methods: None,
                    guard: crate::http::RouteGuard::RequireAuth {
                        redirect_to: None, // 401 JSON for API planes
                        exclude: vec![],
                    },
                });
            }
        }
    }

    /// Enable embedded data admin UI at the specified path (with automatic auth protection)
    ///
    /// This serves an embedded dashboard for browsing and managing data.
    /// Requires the `admin-ui` feature to be enabled.
    ///
    /// **Security**: The dashboard and API are automatically protected with RouteGuard::RequireAuth
    /// when RBAC is configured. Use `.with_data_admin_ui_public()` if you want no auth.
    ///
    /// # Example
    /// ```rust,ignore
    /// LithairServer::new()
    ///     .with_rbac_config(rbac_config)  // Auth is automatically applied
    ///     .with_model::<Article>("./data/articles", "/api/articles")
    ///     .with_data_admin()              // Enable API endpoints
    ///     .with_data_admin_ui("/_data")   // Enable embedded dashboard (auto-secured)
    ///     .serve()
    ///     .await?;
    /// ```
    ///
    /// The dashboard will be available at:
    /// - `/_data/` - Main dashboard page (requires authentication)
    ///
    /// It uses the `/_admin/data/*` API endpoints automatically.
    #[cfg(feature = "admin-ui")]
    pub fn with_data_admin_ui(mut self, path: impl Into<String>) -> Self {
        let ui_path = path.into();
        log::info!("Data Admin UI enabled at {}", ui_path);

        // Also enable the API if not already enabled
        if !self.config.admin.data_admin_enabled {
            self.config.admin.data_admin_enabled = true;
            log::info!("   (API endpoints auto-enabled)");
        }

        // Store the UI path in admin config
        self.config.admin.data_admin_ui_path = Some(ui_path.clone());

        // Auto-apply authentication guard to data admin paths
        // This protects both the UI and API endpoints
        log::info!("   Auth protection enabled (RequireAuth)");

        // Protect the UI path
        self.route_guards.push(crate::http::RouteGuardMatcher {
            pattern: format!("{}/*", ui_path),
            methods: None,
            guard: crate::http::RouteGuard::RequireAuth {
                redirect_to: Some("/login".to_string()),
                exclude: vec![],
            },
        });

        // Also protect the exact UI path
        self.route_guards.push(crate::http::RouteGuardMatcher {
            pattern: ui_path,
            methods: None,
            guard: crate::http::RouteGuard::RequireAuth {
                redirect_to: Some("/login".to_string()),
                exclude: vec![],
            },
        });

        // Protect the API planes (data + frontend). Pushed AFTER the UI-path
        // guards above so a UI request that also matches an API pattern still
        // gets the login redirect (first matching guard to deny wins). Idempotent
        // — shared with `with_data_admin` (issue #143).
        self.guard_admin_api_planes();

        self
    }

    /// Enable embedded data admin UI WITHOUT authentication (for development/internal use only)
    ///
    /// WARNING: This exposes your data without any authentication!
    /// Only use this for local development or behind a VPN/firewall.
    ///
    /// For production, use `.with_data_admin_ui()` which auto-applies auth guards.
    #[cfg(feature = "admin-ui")]
    pub fn with_data_admin_ui_public(mut self, path: impl Into<String>) -> Self {
        let ui_path = path.into();
        log::warn!("Data Admin UI enabled at {} (NO AUTH - development only!)", ui_path);

        // Also enable the API if not already enabled
        if !self.config.admin.data_admin_enabled {
            self.config.admin.data_admin_enabled = true;
            log::info!("   (API endpoints auto-enabled)");
        }

        // Store the UI path in admin config
        self.config.admin.data_admin_ui_path = Some(ui_path);

        self
    }

    // ========================================================================
    // BUILD
    // ========================================================================

    /// Build the server
    pub fn build(self) -> Result<LithairServer> {
        Ok(LithairServer {
            config: self.config,
            session_manager: self.session_manager,
            custom_routes: self.custom_routes,
            not_found_handler: self.not_found_handler,
            route_guards: self.route_guards,
            model_infos: self.model_infos,
            models_require_session: self.models_require_session,
            external_handler_gates: self.external_handler_gates,
            external_handler_sse_wirings: self.external_handler_sse_wirings,
            models: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            frontend_configs: self.frontend_configs, // Frontend configs to load in serve()
            frontend_engines: std::collections::HashMap::new(), // Will be populated in serve()
            vhost_frontend_configs: self.vhost_frontend_configs,
            vhost_frontend_router: crate::http::HostRouter::new(), // populated in serve()
            host_redirects: {
                // Materialize the declared (from, to) pairs into a HostRouter
                // for O(1) lookup at dispatch time. `with_redirect` already
                // normalized both ends, but `HostRouter::insert` re-normalizes
                // defensively — see `host_router::normalize_host`.
                let mut router = crate::http::HostRouter::<String>::new();
                for (from, to) in self.host_redirects {
                    router.insert(&from, to);
                }
                router
            },
            firewall_config: self.firewall_config,
            anti_ddos_config: self.anti_ddos_config,
            access_log: self.access_log,
            access_log_capacity: self.access_log_capacity,
            openapi_enabled: self.openapi_enabled,
            openapi_spec_cache: std::sync::OnceLock::new(),
            // Raft cluster
            cluster_peers: self.cluster_peers.clone(),
            node_id: self.node_id,
            raft_state: None,       // Initialized in serve() if cluster mode enabled
            raft_crud_sender: None, // Initialized in serve() if cluster mode enabled
            // ── CONSENSUS LOG + WAL (cluster mode only) ──────────────────────
            //
            // The ConsensusLog is the in-memory ordered log of all CRUD operations.
            // The WAL (Write-Ahead Log) persists these operations to disk for durability.
            //
            // On startup, we:
            // 1. Create the empty ConsensusLog
            // 2. Open the WAL (which reads the file header)
            // 3. Replay WAL entries into the ConsensusLog to restore pre-crash state
            //
            // This ensures a node restart doesn't lose committed operations.
            // The WAL uses rkyv (zero-copy) serialization and group commit (5ms batches)
            // for high throughput with durability.
            consensus_log: if !self.cluster_peers.is_empty() {
                Some(Arc::new(crate::cluster::ConsensusLog::new()))
            } else {
                None
            },
            wal: if !self.cluster_peers.is_empty() {
                let wal_path =
                    format!("{}/raft/node_{}/wal", raft_base_dir(), self.node_id.unwrap_or(0));
                match crate::cluster::WriteAheadLog::new(&wal_path) {
                    Ok(wal) => {
                        log::info!("WAL initialized at {}", wal_path);
                        Some(Arc::new(wal))
                    }
                    Err(e) => {
                        log::warn!("Failed to initialize WAL: {}", e);
                        None
                    }
                }
            } else {
                None
            },
            // Initialize replication batcher for intelligent batching
            replication_batcher: if !self.cluster_peers.is_empty() {
                let batcher = crate::cluster::ReplicationBatcher::with_default_config();
                Some(Arc::new(batcher))
            } else {
                None
            },
            // Initialize snapshot manager for resync
            snapshot_manager: if !self.cluster_peers.is_empty() {
                let snapshot_path = format!(
                    "{}/raft/node_{}/snapshots",
                    raft_base_dir(),
                    self.node_id.unwrap_or(0)
                );
                match crate::cluster::SnapshotManager::new(&snapshot_path) {
                    Ok(mgr) => {
                        log::info!("Snapshot manager initialized at {}", snapshot_path);
                        Some(Arc::new(tokio::sync::RwLock::new(mgr)))
                    }
                    Err(e) => {
                        log::warn!("Failed to initialize snapshot manager: {}", e);
                        None
                    }
                }
            } else {
                None
            },
            // Initialize migration manager for rolling upgrades
            migration_manager: if !self.cluster_peers.is_empty() {
                log::info!("Migration manager initialized for rolling upgrades");
                Some(Arc::new(crate::cluster::MigrationManager::default()))
            } else {
                None
            },
            // Resync stats for observability
            resync_stats: Arc::new(crate::cluster::ResyncStats::new()),
            // Schema sync state for cluster-wide schema consensus
            schema_sync_state: Arc::new(tokio::sync::RwLock::new(
                self.schema_vote_policy
                    .map(crate::schema::SchemaSyncState::with_policy)
                    .unwrap_or_default(),
            )),
            // SSE broadcaster (shared across all model handlers)
            sse_broadcaster: if self.sse_enabled {
                Some(crate::http::create_broadcaster())
            } else {
                None
            },
            // Issue #69: auto-compaction config (None = feature off).
            auto_compaction: self.auto_compaction,
        })
    }

    /// Build and start the server
    pub async fn serve(self) -> Result<()> {
        let server = self.build()?;
        server.serve().await
    }

    /// Build and start the server, stopping the accept loop when `shutdown`
    /// resolves. Builder-level convenience over
    /// [`LithairServer::serve_with_graceful_shutdown`] (issue #112).
    pub async fn serve_with_graceful_shutdown<F>(self, shutdown: F) -> Result<()>
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let server = self.build()?;
        server.serve_with_graceful_shutdown(shutdown).await
    }
}

impl Default for LithairServerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl LithairServerBuilder {
    /// Test-only view of the host-agnostic frontend configs.
    pub(crate) fn frontend_configs_for_test(&self) -> &[(String, String)] {
        &self.frontend_configs
    }

    /// Test-only view of the vhost frontend configs.
    pub(crate) fn vhost_frontend_configs_for_test(&self) -> &[(VhostScope, String, String)] {
        &self.vhost_frontend_configs
    }

    /// Test-only view of the (from_host, to_host) redirect pairs.
    pub(crate) fn host_redirects_for_test(&self) -> &[(String, String)] {
        &self.host_redirects
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_only_apps_are_unaffected() {
        let builder = LithairServerBuilder::new()
            .with_frontend_at("/", "public")
            .with_frontend_at("/admin", "admin-ui");
        assert_eq!(builder.frontend_configs_for_test().len(), 2);
        assert!(builder.vhost_frontend_configs_for_test().is_empty());
    }

    #[test]
    fn with_vhost_records_scope_and_frontends() {
        let builder = LithairServerBuilder::new()
            .with_vhost("arcker.org", |v| v.with_frontend_at("/", "sites/arcker.org"))
            .with_vhost("lithair.net", |v| {
                v.with_frontend_at("/", "public").with_frontend_at("/admin", "admin-ui")
            });

        let configs = builder.vhost_frontend_configs_for_test();
        assert_eq!(configs.len(), 3);

        let arcker: Vec<_> = configs
            .iter()
            .filter(|(s, _, _)| matches!(s, VhostScope::Host(h) if h == "arcker.org"))
            .collect();
        assert_eq!(arcker.len(), 1);
        assert_eq!(arcker[0].1, "/");
        assert_eq!(arcker[0].2, "sites/arcker.org");

        let lithair: Vec<_> = configs
            .iter()
            .filter(|(s, _, _)| matches!(s, VhostScope::Host(h) if h == "lithair.net"))
            .collect();
        assert_eq!(lithair.len(), 2);
    }

    #[test]
    fn with_vhost_normalizes_host_key() {
        let builder = LithairServerBuilder::new()
            .with_vhost("ARCKER.ORG:443", |v| v.with_frontend_at("/", "dir"));
        let configs = builder.vhost_frontend_configs_for_test();
        assert_eq!(configs.len(), 1);
        match &configs[0].0 {
            VhostScope::Host(h) => assert_eq!(h, "arcker.org"),
            _ => panic!("expected Host scope"),
        }
    }

    #[test]
    fn with_default_vhost_replaces_previous_default() {
        let builder = LithairServerBuilder::new()
            .with_default_vhost(|v| v.with_frontend_at("/", "first"))
            .with_default_vhost(|v| v.with_frontend_at("/", "second"));
        let defaults: Vec<_> = builder
            .vhost_frontend_configs_for_test()
            .iter()
            .filter(|(s, _, _)| matches!(s, VhostScope::Default))
            .collect();
        assert_eq!(defaults.len(), 1);
        assert_eq!(defaults[0].2, "second");
    }

    #[test]
    fn vhost_and_path_only_coexist() {
        // Mixing the two styles must not cause duplication or loss.
        let builder = LithairServerBuilder::new()
            .with_frontend_at("/", "legacy-public")
            .with_vhost("arcker.org", |v| v.with_frontend_at("/", "sites/arcker.org"));

        assert_eq!(builder.frontend_configs_for_test().len(), 1);
        assert_eq!(builder.vhost_frontend_configs_for_test().len(), 1);
    }

    #[test]
    fn with_redirect_records_normalized_pair() {
        // Mixed casing + port on both ends must collapse to bare lower-case
        // hosts so dispatch lookup matches normalized request hosts.
        let builder = LithairServerBuilder::new().with_redirect("WWW.Arcker.ORG:443", "Arcker.ORG");
        let redirects = builder.host_redirects_for_test();
        assert_eq!(redirects.len(), 1);
        assert_eq!(redirects[0].0, "www.arcker.org");
        assert_eq!(redirects[0].1, "arcker.org");
    }

    #[test]
    fn with_redirect_last_write_wins_per_source_host() {
        // Re-declaring a redirect for the same source replaces the target.
        // This mirrors `HostRouter::insert` semantics so behaviour stays
        // predictable when configuration is built incrementally.
        let builder = LithairServerBuilder::new()
            .with_redirect("www.example.com", "example.com")
            .with_redirect("www.example.com", "canonical.example.com");
        let redirects = builder.host_redirects_for_test();
        assert_eq!(redirects.len(), 1);
        assert_eq!(redirects[0].0, "www.example.com");
        assert_eq!(redirects[0].1, "canonical.example.com");
    }

    #[test]
    fn multiple_redirects_are_all_recorded() {
        let builder = LithairServerBuilder::new()
            .with_redirect("www.arcker.org", "arcker.org")
            .with_redirect("www.lithair.net", "lithair.net");
        assert_eq!(builder.host_redirects_for_test().len(), 2);
    }

    #[test]
    fn redirects_default_empty_when_unused() {
        // Existing apps that never call `with_redirect` must not pay any
        // cost — the slice is empty, so the dispatch fast-path kicks in.
        let builder = LithairServerBuilder::new().with_frontend_at("/", "public");
        assert!(builder.host_redirects_for_test().is_empty());
    }

    #[test]
    fn with_redirect_rejects_self_loop() {
        // Self-redirects after normalization are silently dropped (the
        // builder still returns `Self`) so a misconfigured call doesn't
        // ship a 301 → same-host loop in production.
        let builder = LithairServerBuilder::new().with_redirect("Example.COM", "example.com:443");
        assert!(
            builder.host_redirects_for_test().is_empty(),
            "self-redirect should be rejected after normalization"
        );
    }

    #[test]
    fn with_redirect_self_loop_does_not_clobber_other_entries() {
        // Calling `with_redirect` with a self-loop must be a no-op —
        // including not clearing previously-registered, valid entries
        // for unrelated hosts. Otherwise users could lose redirects by
        // accidentally re-registering one of their existing sources.
        let builder = LithairServerBuilder::new()
            .with_redirect("www.a.test", "a.test")
            .with_redirect("a.test", "a.test"); // self-loop, ignored
        let r = builder.host_redirects_for_test();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].0, "www.a.test");
        assert_eq!(r[0].1, "a.test");
    }

    #[test]
    fn with_redirect_rejects_empty_hosts() {
        // `normalize_host` only trims/lowercases — it does not reject
        // malformed inputs. An empty source host would register an empty
        // key in the router, which `host_from_request` returns when the
        // request has neither URI authority nor a `Host:` header. That
        // would silently 301 every host-less request. Both empty source
        // and empty target must be refused.
        let builder = LithairServerBuilder::new()
            .with_redirect("", "example.com")
            .with_redirect("example.com", "")
            .with_redirect("   ", "example.com"); // whitespace -> empty after normalize
        assert!(
            builder.host_redirects_for_test().is_empty(),
            "redirects with empty (post-normalization) endpoints must be rejected"
        );
    }
}
