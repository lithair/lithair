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
//! use lithair_core::server::LithairServer;
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
use std::sync::Arc;

pub mod builder;
pub mod model_handler;
pub mod router;
mod schema_handlers;

pub use builder::LithairServerBuilder;
pub use model_handler::{DeclarativeModelHandler, ModelHandler};

/// Model registration with handler
pub struct ModelRegistration {
    pub name: String,
    pub base_path: String,
    pub data_path: String,
    pub handler: Arc<dyn ModelHandler>,
    pub schema_extractor: Option<SchemaSpecExtractor>,
}

/// Lithair multi-model server
pub struct LithairServer {
    config: LithairConfig,
    session_manager: Option<Arc<dyn std::any::Any + Send + Sync>>,
    custom_routes: Vec<CustomRoute>,
    route_guards: Vec<crate::http::RouteGuardMatcher>,
    model_infos: Vec<ModelRegistrationInfo>,
    models: Arc<tokio::sync::RwLock<Vec<ModelRegistration>>>,

    // Frontend configurations to load (path_prefix -> static_dir)
    frontend_configs: Vec<(String, String)>,

    // Frontend serving (SCC2 memory-first) - Multiple frontends with path prefixes
    // Key: route_prefix (e.g., "/", "/admin"), Value: FrontendEngine
    frontend_engines: std::collections::HashMap<String, Arc<crate::frontend::FrontendEngine>>,

    // HTTP Features
    logging_config: Option<crate::logging::LoggingConfig>,
    readiness_config: Option<crate::http::declarative_server::ReadinessConfig>,
    observe_config: Option<crate::http::declarative_server::ObserveConfig>,
    perf_config: Option<crate::http::declarative_server::PerfEndpointsConfig>,
    gzip_config: Option<crate::http::declarative_server::GzipConfig>,
    route_policies: std::collections::HashMap<String, crate::http::declarative_server::RoutePolicy>,
    firewall_config: Option<crate::http::FirewallConfig>,
    anti_ddos_config: Option<crate::security::anti_ddos::AntiDDoSConfig>,
    legacy_endpoints: bool,
    deprecation_warnings: bool,

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
}

/// A CRUD operation to be submitted through Raft consensus
#[derive(Debug)]
pub struct RaftCrudOperation {
    pub operation: crate::cluster::CrudOperation,
    pub response_tx: tokio::sync::oneshot::Sender<Result<serde_json::Value, String>>,
}

/// Type alias for async route handlers
pub type RouteHandler = Arc<
    dyn Fn(
            hyper::Request<hyper::body::Incoming>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<hyper::Response<http_body_util::Full<bytes::Bytes>>>,
                    > + Send,
            >,
        > + Send
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
}

impl LithairServer {
    /// Create a new Lithair server with default configuration
    ///
    /// Configuration is loaded with full supersedence:
    /// 1. Defaults
    /// 2. Config file (config.toml)
    /// 3. Environment variables
    /// 4. Code (builder methods)
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

        log::info!("🔍 Validating model schemas...");
        if is_cluster {
            log::info!("   🌐 Cluster mode: schema changes will be synchronized");
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
                            "   ✅ {} - schema unchanged (v{})",
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
                                    "   🔒 {} - schema changes BLOCKED (migrations locked)",
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
                            "   ⚠️  {} - {} schema change(s) detected:",
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
                                        "      🌐 Cluster: auto-accepting {:?} change",
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
                                        "      🌐 Cluster: rejecting {:?} change (policy)",
                                        pending.overall_strategy
                                    );
                                    has_breaking_changes = true;
                                }
                                _ => {
                                    // Consensus or ManualApproval required
                                    log::info!("      🌐 Cluster: change requires {:?}", strategy);
                                    state.add_pending(pending);
                                    // Node should wait or be blocked until approval
                                    // For now, we'll just log and continue (TODO: implement blocking)
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
                                            "      📝 Schema updated to v{}",
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
                                        "      🔒 Manual mode: change pending approval (id: {})",
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
                        "   📝 {} - first run, saving schema v{}",
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

        log::info!("✅ Schema validation complete");
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
                            "📜 Loaded {} schema change(s) from history",
                            state.change_history.len()
                        );
                    }
                }
                Err(e) => {
                    log::warn!("⚠️  Failed to load schema history: {}", e);
                }
            }

            // Load lock status
            match load_lock_status(base_path) {
                Ok(lock) => {
                    let mut state = self.schema_sync_state.write().await;
                    state.lock_status = lock;
                    if state.lock_status.is_locked() {
                        log::warn!("🔒 Schema migrations are LOCKED (persisted state)");
                    }
                }
                Err(e) => {
                    log::warn!("⚠️  Failed to load schema lock status: {}", e);
                }
            }
        }

        // Schema validation for models with schema extractors
        if self.config.storage.schema_validation_enabled {
            self.validate_schemas().await?;
        }

        // Create model handlers from factories
        for info in &self.model_infos {
            log::info!("📦 Creating handler for model: {}", info.name);
            match (info.factory)(info.data_path.clone()).await {
                Ok(handler) => {
                    let mut models = self.models.write().await;
                    models.push(ModelRegistration {
                        name: info.name.clone(),
                        base_path: info.base_path.clone(),
                        data_path: info.data_path.clone(),
                        handler,
                        schema_extractor: info.schema_extractor.clone(),
                    });
                    log::info!("✅ Handler created for {}", info.name);
                }
                Err(e) => {
                    log::error!("❌ Failed to create handler for {}: {}", info.name, e);
                    return Err(e.context(format!("Failed to create handler for {}", info.name)));
                }
            }
        }
        self.model_infos.clear(); // Clear infos, we have the models now
                                  // Initialize default logger if not already initialized
        let _ = env_logger::Builder::from_default_env()
            .format_timestamp_millis()
            .format_module_path(false)
            .try_init(); // Use try_init to avoid panic if already initialized

        // Apply logging config if provided
        if let Some(ref logging_config) = self.logging_config {
            log::info!("📝 Applying custom logging configuration");
            // TODO: Actually apply the logging config
            let _ = logging_config; // Suppress unused warning for now
        }

        // Validate configuration
        self.config.validate()?;

        log::info!("🚀 Starting Lithair Server");
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
            log::info!("📦 Loading frontend at '{}' from {}...", route_prefix, static_dir);

            // Create unique host_id from route_prefix
            let host_id = if route_prefix == "/" {
                "default".to_string()
            } else {
                route_prefix.trim_matches('/').replace('/', "_")
            };

            // Create FrontendEngine (SCC2 lock-free with event sourcing)
            match crate::frontend::FrontendEngine::new(&host_id, "./data/frontend").await {
                Ok(engine) => {
                    log::info!("   ✅ FrontendEngine created (host_id: {})", host_id);

                    // Load assets into memory
                    match engine.load_directory(&static_dir).await {
                        Ok(count) => {
                            log::info!("   ✅ {} assets loaded (40M+ ops/sec)", count);
                            self.frontend_engines.insert(route_prefix.clone(), Arc::new(engine));
                        }
                        Err(e) => {
                            log::warn!("   ⚠️ Could not load frontend assets: {}", e);
                        }
                    }
                }
                Err(e) => {
                    log::warn!("   ⚠️ Could not create frontend engine: {}", e);
                }
            }
        }

        // Log HTTP features
        if self.readiness_config.is_some() {
            log::info!("   ✓ Readiness checks enabled");
        }
        if self.observe_config.is_some() {
            log::info!("   ✓ Observability endpoints enabled");
        }
        if self.perf_config.is_some() {
            log::info!("   ✓ Performance endpoints enabled");
        }
        if let Some(ref gzip) = self.gzip_config {
            log::info!("   ✓ Gzip compression enabled (min: {} bytes)", gzip.min_bytes);
        }
        if !self.route_policies.is_empty() {
            log::info!("   ✓ Route policies: {} configured", self.route_policies.len());
        }
        if self.firewall_config.is_some() {
            log::info!("   ✓ Firewall enabled");
        }
        if self.anti_ddos_config.is_some() {
            log::info!("   ✓ Anti-DDoS protection enabled");
        }
        if self.legacy_endpoints {
            log::info!("   ⚠ Legacy endpoints enabled");
        }
        if self.deprecation_warnings {
            log::info!("   ⚠ Deprecation warnings enabled");
        }

        // Initialize Raft cluster if configured
        if self.config.raft.enabled && self.node_id.is_some() && !self.cluster_peers.is_empty() {
            let node_id = self.node_id.unwrap();
            let port = self.config.server.port;

            log::info!("🗳️ Initializing Raft cluster...");
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
                log::info!("👑 THIS NODE IS THE LEADER");
            } else {
                log::info!(
                    "👥 This node is a FOLLOWER (leader port: {})",
                    raft_state.get_leader_port()
                );
            }

            self.raft_state = Some(raft_state);

            // Initialize replication batcher with peers
            if let Some(ref batcher) = self.replication_batcher {
                batcher.initialize(&self.cluster_peers).await;
                log::info!(
                    "📊 Replication batcher initialized with {} peers",
                    self.cluster_peers.len()
                );
            }

            // Start WAL background flush task (group commit)
            if let Some(ref wal) = self.wal {
                let _flush_handle = wal.spawn_flush_task();
                log::info!("💾 WAL group commit enabled (flush interval: 5ms)");
            }

            // Log snapshot status
            if self.snapshot_manager.is_some() {
                log::info!("📸 Snapshot manager enabled for resync");
            }

            // Start background replication task for lagging followers
            if let Some(ref batcher) = self.replication_batcher {
                let batcher_clone = Arc::clone(batcher);
                let peers = self.cluster_peers.clone();
                let consensus_log = self.consensus_log.clone();
                let node_id = self.node_id.unwrap_or(0);
                let raft_state = self.raft_state.clone();
                let snapshot_manager = self.snapshot_manager.clone();
                let models = Arc::clone(&self.models);
                let replication_config = self.config.replication.clone();
                let resync_stats = Arc::clone(&self.resync_stats);

                tokio::spawn(async move {
                    use std::collections::HashMap;
                    use std::time::Duration;
                    use tokio::time::interval;

                    let mut ticker = interval(Duration::from_millis(100)); // Check every 100ms
                    let mut _catchup_counter = 0u64; // Reserved for future use
                    let mut resync_counter = 0u64; // For periodic snapshot resync

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
                            let desynced = batcher_clone.get_desynced_followers(commit_index).await;

                            if !desynced.is_empty() && snapshot_manager.is_some() {
                                // Filter out followers that are in cooldown
                                let cooldown_duration =
                                    Duration::from_secs(replication_config.resync_cooldown_secs);
                                let now = std::time::Instant::now();
                                let eligible_for_resync: Vec<_> = desynced
                                    .into_iter()
                                    .filter(|peer| {
                                        match last_resync.get(peer) {
                                            Some(last_time) => {
                                                now.duration_since(*last_time) >= cooldown_duration
                                            }
                                            None => true, // Never resynced, eligible
                                        }
                                    })
                                    .collect();

                                if !eligible_for_resync.is_empty() {
                                    log::info!(
                                        "🔄 Found {} desynced followers eligible for resync",
                                        eligible_for_resync.len()
                                    );

                                    let snapshot_mgr = snapshot_manager.as_ref().unwrap().clone();
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
                                        log::warn!("Failed to create snapshot for resync: {}", e);
                                    } else {
                                        // Track snapshot creation
                                        resync_stats.record_snapshot_created();

                                        // Send snapshot to each desynced follower (in parallel, with configurable rate limit)
                                        let max_concurrent =
                                            replication_config.max_concurrent_resyncs;
                                        let snapshot_timeout_secs =
                                            replication_config.snapshot_send_timeout_secs;

                                        for peer in
                                            eligible_for_resync.into_iter().take(max_concurrent)
                                        {
                                            // Mark as resyncing with current timestamp
                                            last_resync.insert(peer.clone(), now);

                                            let peer_clone = peer.clone();
                                            let snapshot_mgr_clone = snapshot_mgr.clone();
                                            let batcher_resync = Arc::clone(&batcher_for_resync);
                                            let stats_clone = Arc::clone(&resync_stats);

                                            // Track send attempt
                                            resync_stats.record_send_attempt(commit_index);

                                            tokio::spawn(async move {
                                                log::info!(
                                                    "📸 Sending snapshot to desynced follower: {}",
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
                                                            "✅ Snapshot installed on {}",
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
                                                            "❌ Snapshot send to {} failed: {}",
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

                        // === INCREMENTAL CATCH-UP FOR LAGGING FOLLOWERS ===
                        // Only send entries that followers are actually missing
                        if commit_index > 0 {
                            // Get health status to skip desynced followers
                            let health_summary = batcher_clone.get_health_summary().await;

                            for peer in &peers {
                                // Skip desynced followers - they'll get snapshots instead
                                if let Some(health) = health_summary.get(peer) {
                                    if *health == crate::cluster::replication_batcher::FollowerHealth::Desynced {
                                        log::debug!("⏭️ Skipping desynced follower {} (will use snapshot)", peer);
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
                                let missing_entries =
                                    consensus_log_ref.get_entries_from(follower_index + 1).await;
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
                                            let last_index =
                                                entries.last().map(|e| e.log_id.index).unwrap_or(0);
                                            batcher
                                                .record_success(&peer, last_index, latency)
                                                .await;
                                            log::debug!(
                                                "📤 Background catch-up: {} entries to {} ({}ms)",
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

                                let request = crate::cluster::consensus_log::AppendEntriesRequest {
                                    term,
                                    leader_id: node_id,
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
                                        batcher.record_success(&peer, last_index, latency).await;
                                        log::debug!(
                                            "📤 Background replicated {} entries to {} ({}ms)",
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
                log::info!("🔄 Background replication task started");
            }
        }

        // Build server address
        let addr = format!("{}:{}", self.config.server.host, self.config.server.port);

        // Start HTTP server
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .with_context(|| format!("Failed to bind to {}", addr))?;

        log::info!("✅ Server listening on http://{}", addr);

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
                            log::info!("💔 No longer leader, stopping heartbeat sender");
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

        // Share server state
        let server = Arc::new(self);

        // Accept connections
        loop {
            let (stream, remote_addr) = listener.accept().await?;
            let server = server.clone();

            tokio::spawn(async move {
                let io = hyper_util::rt::TokioIo::new(stream);

                let service = hyper::service::service_fn(move |req| {
                    let server = server.clone();
                    async move { server.handle_request(req).await }
                });

                if let Err(err) =
                    hyper::server::conn::http1::Builder::new().serve_connection(io, service).await
                {
                    log::error!("Connection error from {}: {}", remote_addr, err);
                }
            });
        }
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
            if pattern.ends_with("/**") {
                let prefix = &pattern[..pattern.len() - 3];
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

        log::debug!("{} {}", method, path);

        // 🗳️ Raft Cluster: Check for write redirection and Raft endpoints
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
                        .unwrap());
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
                            "💓 Heartbeat: updating leader to node {} (port {})",
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
                    .unwrap());
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
                        .unwrap());
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
                    .unwrap());
            }

            // Redirect writes to leader if we're a follower
            // Exception: /internal/* and /_raft/* endpoints are internal cluster communication
            let is_write =
                matches!(method, hyper::Method::POST | hyper::Method::PUT | hyper::Method::DELETE);
            let is_internal = path.starts_with("/internal/") || path.starts_with("/_raft/");

            if is_write && !raft_state.is_leader() && !is_internal {
                let leader_port = raft_state.get_leader_port();
                let redirect_url = format!(
                    "http://127.0.0.1:{}{}",
                    leader_port,
                    req.uri().path_and_query().map(|pq| pq.as_str()).unwrap_or(&path)
                );

                log::debug!("🔀 Redirecting write to leader on port {}", leader_port);

                return Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::TEMPORARY_REDIRECT)
                    .header(hyper::header::LOCATION, redirect_url.clone())
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(format!(
                        r#"{{"message":"Redirected to leader","leader_url":"{}"}}"#,
                        redirect_url
                    ))))
                    .unwrap());
            }
        }

        // 📦 Internal replication endpoints (for followers to receive data from leader)
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

        // 📜 Raft consensus log append entries endpoint (for followers to receive log entries from leader)
        if path == "/_raft/append" && method == hyper::Method::POST {
            return self.handle_raft_append_entries(req).await;
        }

        // 📸 Snapshot endpoints for resync of desynced followers
        if path == "/_raft/snapshot" && method == hyper::Method::GET {
            return self.handle_get_snapshot(req).await;
        }
        if path == "/_raft/snapshot" && method == hyper::Method::POST {
            return self.handle_install_snapshot(req).await;
        }

        // 📊 Cluster health endpoint (follower status)
        if path == "/_raft/health" && method == hyper::Method::GET {
            return self.handle_cluster_health().await;
        }

        // 📊 Resync stats endpoint (snapshot resync observability)
        if path == "/_raft/resync_stats" && method == hyper::Method::GET {
            return self.handle_resync_stats().await;
        }

        // 🔄 Migration operation endpoint (for rolling upgrades)
        if path == "/_raft/migrate" && method == hyper::Method::POST {
            return self.handle_migrate_operation(req).await;
        }

        // 📊 Sync status endpoint (detailed follower sync state for ops)
        if path == "/_raft/sync-status" && method == hyper::Method::GET {
            return self.handle_sync_status().await;
        }

        // 🔄 Force resync endpoint (manually trigger snapshot resync)
        if path.starts_with("/_raft/force-resync") && method == hyper::Method::POST {
            return self.handle_force_resync(req).await;
        }

        // 📋 Schema sync endpoints (cluster-internal)
        if path == "/_raft/schema/propose" && method == hyper::Method::POST {
            return self.handle_schema_propose(req).await;
        }
        if path == "/_raft/schema/vote" && method == hyper::Method::POST {
            return self.handle_schema_vote(req).await;
        }
        if path == "/_raft/schema/current" && method == hyper::Method::GET {
            return self.handle_schema_current(req).await;
        }

        // 📋 Schema admin endpoints (external management)
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

        // 🛡️ Route Guards - Declarative protection (authentication, authorization, etc.)
        for guard_matcher in &self.route_guards {
            if guard_matcher.matches(&req) {
                log::debug!("🔐 Evaluating guard for pattern: {}", guard_matcher.pattern);
                match guard_matcher.guard.check(&req, self.session_manager.clone()).await {
                    Ok(crate::http::GuardResult::Allow) => {
                        log::debug!("✅ Guard allowed request");
                        // Continue to next guard or routing
                    }
                    Ok(crate::http::GuardResult::Deny(response)) => {
                        log::debug!("🚫 Guard denied request");
                        return Ok(response);
                    }
                    Err(e) => {
                        log::error!("❌ Guard check failed: {}", e);
                        return Ok(hyper::Response::builder()
                            .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                            .header("Content-Type", "application/json")
                            .body(Full::new(Bytes::from(r#"{"error":"Internal server error"}"#)))
                            .unwrap());
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
                .unwrap());
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

        // Model routes (API endpoints checked first)
        let models = self.models.read().await;
        for model in models.iter() {
            if path.starts_with(&model.base_path) {
                return self.handle_model_request(req, model).await;
            }
        }
        drop(models); // Release read lock before custom routes

        // Custom routes (with wildcard support)
        for route in &self.custom_routes {
            if route.method == method && Self::path_matches(&route.path, &path) {
                // Call custom handler
                return (route.handler)(req).await;
            }
        }

        // Frontend assets (memory-first serving with SCC2)
        // Checked AFTER API routes so /admin/login.html is served but /admin/api/* can still work
        // Try to match frontend engine by path prefix (longest match first)
        if method == hyper::Method::GET && !self.frontend_engines.is_empty() {
            // Sort prefixes by length (longest first) for proper matching
            let mut prefixes: Vec<_> = self.frontend_engines.keys().collect();
            prefixes.sort_by(|a, b| b.len().cmp(&a.len()));

            // Special handling for _astro assets: try ALL frontends as fallback
            // This allows admin frontend to reference /_astro/* even when served at /secure-xy3xir/
            if path.starts_with("/_astro/") {
                // Try each frontend engine directly via SCC2 lookup
                for prefix in &prefixes {
                    if let Some(engine) = self.frontend_engines.get(*prefix) {
                        // Check if this engine has the asset in its SCC2 storage
                        if let Some(asset) = engine.get_asset(&path).await {
                            // Use mime_type from asset
                            return Ok(hyper::Response::builder()
                                .status(200)
                                .header("Content-Type", asset.mime_type)
                                .header("Cache-Control", "public, max-age=31536000, immutable")
                                .body(Full::new(Bytes::from(asset.content)))
                                .unwrap());
                        }
                    }
                }
                // All frontends returned 404, fall through to final 404
            } else {
                // Normal path matching: find the first matching prefix
                for prefix in prefixes {
                    if path.starts_with(prefix) {
                        if let Some(engine) = self.frontend_engines.get(prefix) {
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
                            parts.uri = asset_path.parse().unwrap();
                            let modified_req = hyper::Request::from_parts(parts, body);

                            // Call frontend server (returns BoxBody, includes 404 if not found)
                            match frontend_server.handle_request(modified_req).await {
                                Ok(response) => {
                                    // Convert BoxBody to Full<Bytes>
                                    use http_body_util::BodyExt;
                                    let (parts, body) = response.into_parts();
                                    let bytes = body.collect().await.unwrap().to_bytes();
                                    let full_response =
                                        hyper::Response::from_parts(parts, Full::new(bytes));
                                    return Ok(full_response);
                                }
                                Err(_) => {} // Infallible, won't happen
                            }
                        }
                        // Break after first prefix match to avoid consuming req multiple times
                        break;
                    }
                }
            }
        }

        // 404 Not Found (if no frontend or non-GET request)
        Ok(hyper::Response::builder()
            .status(404)
            .body(Full::new(Bytes::from(r#"{"error":"Not found"}"#)))
            .unwrap())
    }

    /// Handle admin panel request
    #[allow(dead_code)]
    async fn handle_admin_request(
        &self,
        _req: hyper::Request<hyper::body::Incoming>,
    ) -> Result<hyper::Response<http_body_util::Full<bytes::Bytes>>> {
        use http_body_util::Full;

        // TODO: Implement admin panel
        Ok(hyper::Response::builder()
            .status(200)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(r#"{"status":"ok","admin":"panel"}"#)))
            .unwrap())
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
                    .unwrap());
            }
        };

        let message: serde_json::Value = match serde_json::from_slice(&body_bytes) {
            Ok(v) => v,
            Err(e) => {
                return Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::BAD_REQUEST)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(format!(r#"{{"error":"Invalid JSON: {}"}}"#, e))))
                    .unwrap());
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
                            log::debug!("📥 CREATE replication applied for model {}", model.name);
                            return Ok(hyper::Response::builder()
                                .status(hyper::StatusCode::OK)
                                .header("Content-Type", "application/json")
                                .body(Full::new(Bytes::from(r#"{"status":"ok"}"#)))
                                .unwrap());
                        }
                        Err(e) => {
                            log::error!("❌ CREATE replication failed: {}", e);
                            return Ok(hyper::Response::builder()
                                .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                                .header("Content-Type", "application/json")
                                .body(Full::new(Bytes::from(format!(r#"{{"error":"{}"}}"#, e))))
                                .unwrap());
                        }
                    }
                } else if let Some(update_data) = op.get("Update") {
                    let item_data =
                        update_data.get("item").cloned().unwrap_or(serde_json::Value::Null);
                    let primary_key =
                        update_data.get("primary_key").and_then(|v| v.as_str()).unwrap_or("");
                    match model.handler.apply_replicated_update_json(primary_key, item_data).await {
                        Ok(()) => {
                            log::debug!("📥 UPDATE replication applied for model {}", model.name);
                            return Ok(hyper::Response::builder()
                                .status(hyper::StatusCode::OK)
                                .header("Content-Type", "application/json")
                                .body(Full::new(Bytes::from(r#"{"status":"ok"}"#)))
                                .unwrap());
                        }
                        Err(e) => {
                            log::error!("❌ UPDATE replication failed: {}", e);
                            return Ok(hyper::Response::builder()
                                .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                                .header("Content-Type", "application/json")
                                .body(Full::new(Bytes::from(format!(r#"{{"error":"{}"}}"#, e))))
                                .unwrap());
                        }
                    }
                } else if let Some(delete_data) = op.get("Delete") {
                    let primary_key =
                        delete_data.get("primary_key").and_then(|v| v.as_str()).unwrap_or("");
                    match model.handler.apply_replicated_delete_json(primary_key).await {
                        Ok(_) => {
                            log::debug!("📥 DELETE replication applied for model {}", model.name);
                            return Ok(hyper::Response::builder()
                                .status(hyper::StatusCode::OK)
                                .header("Content-Type", "application/json")
                                .body(Full::new(Bytes::from(r#"{"status":"ok"}"#)))
                                .unwrap());
                        }
                        Err(e) => {
                            log::error!("❌ DELETE replication failed: {}", e);
                            return Ok(hyper::Response::builder()
                                .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                                .header("Content-Type", "application/json")
                                .body(Full::new(Bytes::from(format!(r#"{{"error":"{}"}}"#, e))))
                                .unwrap());
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
                        .unwrap());
                }
            };

            match model.handler.apply_replicated_item_json(item_data).await {
                Ok(()) => {
                    log::debug!("📥 Replication applied for model {}", model.name);
                    Ok(hyper::Response::builder()
                        .status(hyper::StatusCode::OK)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(r#"{"status":"ok"}"#)))
                        .unwrap())
                }
                Err(e) => {
                    log::error!("❌ Replication failed: {}", e);
                    Ok(hyper::Response::builder()
                        .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(format!(r#"{{"error":"{}"}}"#, e))))
                        .unwrap())
                }
            }
        } else {
            Ok(hyper::Response::builder()
                .status(hyper::StatusCode::NOT_FOUND)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(r#"{"error":"No model handler found"}"#)))
                .unwrap())
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
                    .unwrap());
            }
        };

        let message: serde_json::Value = match serde_json::from_slice(&body_bytes) {
            Ok(v) => v,
            Err(e) => {
                return Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::BAD_REQUEST)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(format!(r#"{{"error":"Invalid JSON: {}"}}"#, e))))
                    .unwrap());
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
                    .unwrap());
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
                        "📥 Bulk replication applied: {} items for model {} (batch: {})",
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
                        .unwrap())
                }
                Err(e) => {
                    log::error!("❌ Bulk replication failed: {}", e);
                    Ok(hyper::Response::builder()
                        .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(format!(r#"{{"error":"{}"}}"#, e))))
                        .unwrap())
                }
            }
        } else {
            Ok(hyper::Response::builder()
                .status(hyper::StatusCode::NOT_FOUND)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(r#"{"error":"No model handler found"}"#)))
                .unwrap())
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
                    .unwrap());
            }
        };

        let message: serde_json::Value = match serde_json::from_slice(&body_bytes) {
            Ok(v) => v,
            Err(e) => {
                return Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::BAD_REQUEST)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(format!(r#"{{"error":"Invalid JSON: {}"}}"#, e))))
                    .unwrap());
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
                    .unwrap());
            }
        };

        let item_data = match message.get("data") {
            Some(data) => data.clone(),
            None => {
                return Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::BAD_REQUEST)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(r#"{"error":"Missing 'data' field"}"#)))
                    .unwrap());
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
                    log::debug!("📥 Replication UPDATE applied for {} in model {}", id, model.name);
                    Ok(hyper::Response::builder()
                        .status(hyper::StatusCode::OK)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(r#"{"status":"ok"}"#)))
                        .unwrap())
                }
                Err(e) => {
                    log::error!("❌ Replication UPDATE failed: {}", e);
                    Ok(hyper::Response::builder()
                        .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(format!(r#"{{"error":"{}"}}"#, e))))
                        .unwrap())
                }
            }
        } else {
            Ok(hyper::Response::builder()
                .status(hyper::StatusCode::NOT_FOUND)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(r#"{"error":"No model handler found"}"#)))
                .unwrap())
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
                    .unwrap());
            }
        };

        let message: serde_json::Value = match serde_json::from_slice(&body_bytes) {
            Ok(v) => v,
            Err(e) => {
                return Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::BAD_REQUEST)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(format!(r#"{{"error":"Invalid JSON: {}"}}"#, e))))
                    .unwrap());
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
                    .unwrap());
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
                        "📥 Replication DELETE applied for {} in model {} (deleted: {})",
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
                        .unwrap())
                }
                Err(e) => {
                    log::error!("❌ Replication DELETE failed: {}", e);
                    Ok(hyper::Response::builder()
                        .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(format!(r#"{{"error":"{}"}}"#, e))))
                        .unwrap())
                }
            }
        } else {
            Ok(hyper::Response::builder()
                .status(hyper::StatusCode::NOT_FOUND)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(r#"{"error":"No model handler found"}"#)))
                .unwrap())
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
                        .unwrap());
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
                    .unwrap());
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
                .unwrap());
        }

        // Update heartbeat (if we have raft_state)
        if let Some(ref raft_state) = self.raft_state {
            raft_state.update_heartbeat();
        }

        // Append entries to local log (can happen concurrently)
        let entries_count = request.entries.len();
        consensus_log
            .append_entries(request.entries.clone(), request.leader_commit)
            .await;

        log::debug!(
            "📥 Received {} entries from leader {}, commit_index={}",
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
            log::debug!("📥 FOLLOWER: Applying {} entry index={}", op_type, entry.log_id.index);
            match self.apply_crud_operation(&entry.operation).await {
                Ok(_) => {
                    consensus_log.mark_applied(entry.log_id.index);
                    log::debug!("✅ Applied entry index={}", entry.log_id.index);
                }
                Err(e) => {
                    // CRITICAL: Stop processing here! If we continue, we'd skip this entry
                    // because mark_applied on later entries would advance applied_index past it.
                    // Mark as NOT all successful so leader knows to retry
                    log::error!(
                        "❌ Failed to apply entry index={}: {} - stopping to prevent skip",
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
            .unwrap())
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
                    .unwrap());
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
                    .unwrap());
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
                    .unwrap())
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
                    .unwrap())
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
                    .unwrap());
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
                    .unwrap());
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
                log::info!("📸 Snapshot installed: index={}, term={}", last_included_index, term);

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
                    .unwrap())
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
                    .unwrap())
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
            .unwrap())
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
            .unwrap())
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
                .unwrap());
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
                let lag = if commit_index > f.last_replicated_index {
                    commit_index - f.last_replicated_index
                } else {
                    0
                };

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
            .unwrap())
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
                .unwrap());
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
                    .unwrap());
            }
        };

        log::info!("🔄 Manual resync requested for follower: {}", target);

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
                .unwrap());
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
                log::info!("✅ Manual resync to {} completed successfully", target);
                (
                    hyper::StatusCode::OK,
                    format!("Snapshot resync to {} completed successfully", target),
                )
            }
            Err(e) => {
                self.resync_stats.record_send_failure();
                log::error!("❌ Manual resync to {} failed: {}", target, e);
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
            .unwrap())
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
                .unwrap());
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
                    .unwrap());
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
                .unwrap());
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
                            .unwrap()),
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
                            .unwrap()),
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
                    .unwrap()),
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
                    .unwrap()),
                Err(e) => Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(
                        serde_json::json!({
                            "error": format!("Migration failed: {}", e)
                        })
                        .to_string(),
                    )))
                    .unwrap()),
            }
        }
    }

    /// Handle metrics request
    async fn handle_metrics_request(
        &self,
        _req: hyper::Request<hyper::body::Incoming>,
    ) -> Result<hyper::Response<http_body_util::Full<bytes::Bytes>>> {
        use http_body_util::Full;

        // TODO: Implement Prometheus metrics
        Ok(hyper::Response::builder()
            .status(200)
            .header("Content-Type", "text/plain")
            .body(Full::new(Bytes::from("# Metrics endpoint\n")))
            .unwrap())
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
                    .body(Full::new(Bytes::from(serde_json::to_string_pretty(&response).unwrap())))
                    .unwrap())
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
                            serde_json::to_string_pretty(&response).unwrap(),
                        )))
                        .unwrap())
                } else {
                    Ok(hyper::Response::builder()
                        .status(404)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(format!(
                            r#"{{"error":"Model '{}' not found"}}"#,
                            name
                        ))))
                        .unwrap())
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
                            serde_json::to_string_pretty(&export).unwrap(),
                        )))
                        .unwrap())
                } else {
                    Ok(hyper::Response::builder()
                        .status(404)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(format!(
                            r#"{{"error":"Model '{}' not found"}}"#,
                            name
                        ))))
                        .unwrap())
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
                            serde_json::to_string_pretty(&history).unwrap(),
                        )))
                        .unwrap())
                } else {
                    Ok(hyper::Response::builder()
                        .status(404)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(format!(
                            r#"{{"error":"Model '{}' not found"}}"#,
                            name
                        ))))
                        .unwrap())
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
                                .unwrap());
                        }
                    };

                    let changes: serde_json::Value = match serde_json::from_slice(&body_bytes) {
                        Ok(v) => v,
                        Err(_) => {
                            return Ok(hyper::Response::builder()
                                .status(400)
                                .header("Content-Type", "application/json")
                                .body(Full::new(Bytes::from(r#"{"error":"Invalid JSON"}"#)))
                                .unwrap());
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
                                    serde_json::to_string_pretty(&response).unwrap(),
                                )))
                                .unwrap())
                        }
                        Err(e) => Ok(hyper::Response::builder()
                            .status(400)
                            .header("Content-Type", "application/json")
                            .body(Full::new(Bytes::from(format!(r#"{{"error":"{}"}}"#, e))))
                            .unwrap()),
                    }
                } else {
                    Ok(hyper::Response::builder()
                        .status(404)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(format!(
                            r#"{{"error":"Model '{}' not found"}}"#,
                            name
                        ))))
                        .unwrap())
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
                    .body(Full::new(Bytes::from(serde_json::to_string_pretty(&response).unwrap())))
                    .unwrap())
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
                    .body(Full::new(Bytes::from(serde_json::to_string_pretty(&backup).unwrap())))
                    .unwrap())
            }

            // 404 for unknown data admin paths
            _ => Ok(hyper::Response::builder()
                .status(404)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(r#"{"error":"Unknown data admin endpoint"}"#)))
                .unwrap()),
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
            .unwrap())
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
        if is_write && self.consensus_log.is_some() && !self.cluster_peers.is_empty() {
            log::debug!(
                "🔄 CLUSTER MODE: {} {} (create={}, update={}, delete={})",
                method,
                path,
                is_create,
                is_update,
                is_delete
            );

            // Check if we are the leader
            let is_leader = self.raft_state.as_ref().map(|s| s.is_leader()).unwrap_or(false);

            if !is_leader {
                // Redirect to leader
                if let Some(ref raft_state) = self.raft_state {
                    let leader_port = raft_state.get_leader_port();
                    return Ok(hyper::Response::builder()
                        .status(307) // Temporary Redirect
                        .header("Location", format!("http://127.0.0.1:{}{}", leader_port, path))
                        .header("X-Raft-Leader", format!("{}", leader_port))
                        .body(Full::new(Bytes::from(format!(r#"{{"error":"Not leader","leader_port":{}}}"#, leader_port))))
                        .unwrap());
                }
            }

            // We are the leader - process through consensus log
            let consensus_log = self.consensus_log.as_ref().unwrap();

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
                if data.get("id").is_none() || data.get("id") == Some(&serde_json::Value::Null) {
                    data["id"] = serde_json::Value::String(uuid::Uuid::new_v4().to_string());
                }
                // Add timestamps for consistency across all nodes
                if data.get("created_at").is_none() {
                    data["created_at"] = serde_json::Value::String(now.clone());
                }
                if data.get("updated_at").is_none() {
                    data["updated_at"] = serde_json::Value::String(now.clone());
                }
                crate::cluster::CrudOperation::Create { model_path: model.base_path.clone(), data }
            } else if is_update {
                let id = resource_id.clone().unwrap_or_default();
                log::info!("📝 CLUSTER: Creating UPDATE operation for id={}", id);
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
                log::info!("🗑️ CLUSTER: Creating DELETE operation for id={}", id);
                crate::cluster::CrudOperation::Delete { model_path: model.base_path.clone(), id }
            } else {
                // Bulk create - for now handle as single operation
                // TODO: Handle bulk properly with BatchOperation
                let mut data = body_json.clone();
                if data.get("id").is_none() || data.get("id") == Some(&serde_json::Value::Null) {
                    data["id"] = serde_json::Value::String(uuid::Uuid::new_v4().to_string());
                }
                // Add timestamps for consistency across all nodes
                if data.get("created_at").is_none() {
                    data["created_at"] = serde_json::Value::String(now.clone());
                }
                if data.get("updated_at").is_none() {
                    data["updated_at"] = serde_json::Value::String(now);
                }
                crate::cluster::CrudOperation::Create { model_path: model.base_path.clone(), data }
            };

            // Step 1: Append to local consensus log (in-memory, fast)
            let log_entry = consensus_log.append(operation.clone()).await;
            let entry_index = log_entry.log_id.index;
            let term = consensus_log.current_term();
            let node_id = self.node_id.unwrap_or(0);
            let _current_commit = consensus_log.commit_index(); // For debugging (window-based replication doesn't need this)
            log::debug!("📝 Appended to log: index={}, term={}", entry_index, term);

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
            // Send ALL entries from beginning to ensure lagging followers can always catch up.
            // This is critical: if we use a window, followers stuck on entry N will never receive
            // entries N+1 to window_start, causing permanent divergence.
            let consensus_log_clone = consensus_log.clone();
            let replication_future = async move {
                // Always send ALL entries from index 1 to ensure no gaps
                let entries_to_send = consensus_log_clone.get_entries_from(1).await;

                if entries_to_send.is_empty() {
                    return Ok(entry_index);
                }

                log::debug!(
                    "📤 Replicating {} entries (window {} to {}), target_commit={}",
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
                    batcher_clone,
                )
                .await
            };

            // Run WAL and replication in parallel
            let (wal_result, replication_result) = tokio::join!(wal_future, replication_future);

            // Check WAL result first (must succeed for durability)
            if let Err(e) = wal_result {
                log::error!("❌ WAL write failed: {}", e);
                return Ok(hyper::Response::builder()
                    .status(503)
                    .body(Full::new(Bytes::from(format!(
                        r#"{{"error":"WAL write failed: {}"}}"#,
                        e
                    ))))
                    .unwrap());
            }
            log::debug!("💾 WAL entry durable: index={}", entry_index);

            // Check replication result
            match replication_result {
                Ok(new_commit_index) => {
                    // Step 4: Commit the entry (majority achieved)
                    consensus_log.commit(new_commit_index);
                    log::debug!("✅ Committed index: {}", new_commit_index);

                    // Step 4.5: Send commit notification to followers IN PARALLEL (fire-and-forget)
                    // Include the window of entries so followers get both data and commit in one shot
                    let peers_for_notify = self.cluster_peers.clone();
                    let commit_index_to_notify = new_commit_index;
                    let term_for_notify = term;
                    let node_id_for_notify = node_id;
                    let consensus_log_for_notify = consensus_log.clone();
                    tokio::spawn(async move {
                        // Send ALL entries from index 1 to ensure followers can always catch up
                        // This is critical: if we use a window, followers stuck on entry N will never
                        // receive entries N+1 to window_start, causing permanent divergence
                        let entries_for_notify = consensus_log_for_notify.get_entries_from(1).await;

                        let client = reqwest::Client::builder()
                            .timeout(std::time::Duration::from_secs(1))
                            .build()
                            .ok();
                        if let Some(client) = client {
                            let request = crate::cluster::consensus_log::AppendEntriesRequest {
                                term: term_for_notify,
                                leader_id: node_id_for_notify,
                                prev_log_index: 0,
                                prev_log_term: 0,
                                entries: entries_for_notify, // Include entries for catch-up
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
                                        let _ = client.post(&endpoint).json(&request).send().await;
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
                                "❌ Waited 5s for earlier entry {} to commit (current commit={})",
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
                                .unwrap());
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
                                "⚠️ Slow apply: entry {} waiting for {} (commit={}, applied={})",
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
                                .unwrap());
                        }
                        Err(e) => {
                            log::error!("Failed to apply operation: {}", e);
                            return Ok(hyper::Response::builder()
                                .status(500)
                                .body(Full::new(Bytes::from(format!(
                                    r#"{{"error":"Apply failed: {}"}}"#,
                                    e
                                ))))
                                .unwrap());
                        }
                    }
                }
                Err(e) => {
                    log::error!("Failed to replicate: {}", e);
                    return Ok(hyper::Response::builder()
                        .status(503) // Service Unavailable
                        .body(Full::new(Bytes::from(format!(r#"{{"error":"Replication failed: {}"}}"#, e))))
                        .unwrap());
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
                .unwrap()),
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
                    "🔄 MIGRATION_BEGIN: {} -> {} (id: {})",
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
                            log::info!("✅ Migration {} registered successfully", migration_id);
                            Ok(serde_json::json!({
                                "status": "started",
                                "migration_id": migration_id.to_string(),
                                "from": from_version.to_string(),
                                "to": to_version.to_string(),
                            }))
                        }
                        Err(e) => {
                            log::error!("❌ Failed to begin migration {}: {}", migration_id, e);
                            Err(e)
                        }
                    }
                } else {
                    // No migration manager - acknowledge but warn
                    log::warn!("⚠️ Migration manager not available, migration {} acknowledged but not tracked", migration_id);
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
                    "🔧 MIGRATION_STEP: migration={}, step={}, operation={:?}",
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
                                log::warn!("⚠️ Failed to record migration step: {}", e);
                            }
                        }
                        Ok(serde_json::json!({
                            "status": "applied",
                            "migration_id": migration_id.to_string(),
                            "step_index": step_index,
                        }))
                    }
                    Err(e) => {
                        log::error!("❌ Migration step {} failed: {}", step_index, e);
                        Err(format!("Migration step {} failed: {}", step_index, e))
                    }
                }
            }
            CrudOperation::MigrationCommit { migration_id, checksum } => {
                log::info!(
                    "✅ MIGRATION_COMMIT: migration={}, checksum={}",
                    migration_id,
                    checksum
                );

                if let Some(ref manager) = self.migration_manager {
                    // Get migration context to get the target version
                    if let Some(ctx) = manager.get_migration(migration_id).await {
                        let new_version = ctx.to_version.clone();
                        match manager.commit_migration(migration_id, new_version).await {
                            Ok(()) => {
                                log::info!(
                                    "✅ Migration {} committed, checksum verified: {}",
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
                                log::error!(
                                    "❌ Failed to commit migration {}: {}",
                                    migration_id,
                                    e
                                );
                                Err(e)
                            }
                        }
                    } else {
                        let msg = format!("Migration {} not found", migration_id);
                        log::error!("❌ {}", msg);
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
                    "⚠️ MIGRATION_ROLLBACK: migration={}, failed_step={}, reason={}",
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
                                log::info!("🔄 Rolling back step {}", op.step_index);
                                if let Err(e) = self.apply_schema_change(&op.operation).await {
                                    log::error!("❌ Rollback step {} failed: {}", op.step_index, e);
                                    rollback_errors.push(format!("Step {}: {}", op.step_index, e));
                                }
                            }

                            if rollback_errors.is_empty() {
                                log::info!("✅ Migration {} fully rolled back", migration_id);
                                Ok(serde_json::json!({
                                    "status": "rolled_back",
                                    "migration_id": migration_id.to_string(),
                                    "failed_step": failed_step,
                                    "reason": reason,
                                }))
                            } else {
                                log::error!(
                                    "⚠️ Migration {} partially rolled back with errors",
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
                            log::error!("❌ Failed to rollback migration {}: {}", migration_id, e);
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
                log::info!("📦 Applying AddModel: {}", name);
                // TODO: Phase 3 - Register model in runtime schema registry
                // For now, log and return the inverse operation
                Ok(SchemaChange::RemoveModel { name: name.clone(), backup_path: None })
            }
            SchemaChange::RemoveModel { name, backup_path } => {
                log::info!("🗑️ Applying RemoveModel: {} (backup: {:?})", name, backup_path);
                // TODO: Phase 3 - Remove model from registry, backup data if path provided
                // For rollback, we need the original schema - this would be stored in migration context
                Ok(SchemaChange::Custom {
                    description: format!("Restore model: {}", name),
                    forward: format!("restore_model:{}", name),
                    backward: format!("remove_model:{}", name),
                })
            }
            SchemaChange::AddField { model, field, default_value: _ } => {
                log::info!("➕ Applying AddField: {}.{}", model, field.name);
                // TODO: Phase 3 - Add field to model schema
                Ok(SchemaChange::RemoveField { model: model.clone(), field: field.name.clone() })
            }
            SchemaChange::RemoveField { model, field } => {
                log::info!("➖ Applying RemoveField: {}.{}", model, field);
                // TODO: Phase 3 - Remove field from model, backup data
                // For rollback, we need the original field definition
                Ok(SchemaChange::Custom {
                    description: format!("Restore field: {}.{}", model, field),
                    forward: format!("restore_field:{}:{}", model, field),
                    backward: format!("remove_field:{}:{}", model, field),
                })
            }
            SchemaChange::RenameField { model, old_name, new_name } => {
                log::info!("✏️ Applying RenameField: {}.{} -> {}", model, old_name, new_name);
                // TODO: Phase 3 - Update field name in schema and all data
                // Inverse is simply swapping old and new names
                Ok(SchemaChange::RenameField {
                    model: model.clone(),
                    old_name: new_name.clone(),
                    new_name: old_name.clone(),
                })
            }
            SchemaChange::ChangeFieldType { model, field, new_type, transform: _ } => {
                log::info!("🔄 Applying ChangeFieldType: {}.{} -> {:?}", model, field, new_type);
                // TODO: Phase 3 - Transform field data to new type
                // For rollback, we need the original type - stored in migration context
                Ok(SchemaChange::Custom {
                    description: format!("Restore field type: {}.{}", model, field),
                    forward: format!("restore_type:{}:{}", model, field),
                    backward: format!("change_type:{}:{}:{:?}", model, field, new_type),
                })
            }
            SchemaChange::AddIndex { model, fields, unique } => {
                log::info!("📇 Applying AddIndex: {}.{:?} (unique: {})", model, fields, unique);
                // TODO: Phase 3 - Create index on model fields
                // Inverse: remove the index
                Ok(SchemaChange::Custom {
                    description: format!("Remove index on {}.{:?}", model, fields),
                    forward: format!("drop_index:{}:{}", model, fields.join(",")),
                    backward: format!("create_index:{}:{}:{}", model, fields.join(","), unique),
                })
            }
            SchemaChange::Custom { description, forward, backward } => {
                log::info!("⚙️ Applying Custom: {}", description);
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
                "📸 Snapshot sent successfully to {} (index={}, {}KB)",
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
                "📸 Snapshot sent successfully to {} (index={}, {}KB, timeout={}s)",
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
            log::debug!(
                "⏭️ Skipping {} desynced followers (will use snapshot resync)",
                skipped_count
            );
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
                                        "📤 Log replicated to {} (applied_index={}, target={})",
                                        peer_name,
                                        response.applied_index,
                                        target_commit
                                    );
                                } else {
                                    log::debug!(
                                        "📤 Log replicated AND applied on {} (applied_index={})",
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
                            "⚠️ Replicate log to {} failed: status {}",
                            peer_name,
                            resp.status()
                        );
                        (peer_name.clone(), false, 0)
                    }
                    Err(e) => {
                        log::warn!("⚠️ Replicate log to {} error: {}", peer_name, e);
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
                    log::debug!("✓ {}/{} followers acknowledged", success_count, peers.len());

                    // Track when majority is reached, but DON'T return early
                    if success_count >= needed_from_followers && !majority_reached {
                        majority_reached = true;
                        commit_index =
                            entries.last().map(|e| e.log_id.index).unwrap_or(leader_commit);
                        log::debug!(
                            "✅ Majority reached ({}/{}), will commit index {}",
                            success_count + 1,
                            total_nodes,
                            commit_index
                        );
                        // Continue waiting for remaining followers instead of returning
                    }
                } else {
                    log::debug!("✗ Follower {} failed", peer);
                }
            }

            // Continue until all followers complete or timeout
            // Don't break early - we want to give all followers a chance
        }

        // Check if majority was reached (even if some followers timed out)
        if majority_reached {
            log::debug!(
                "✅ Replication complete: {}/{} followers succeeded, committing {}",
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
            route_guards: Vec::new(),
            model_infos: Vec::new(),
            models: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            frontend_configs: Vec::new(),
            frontend_engines: std::collections::HashMap::new(),
            logging_config: None,
            readiness_config: None,
            observe_config: None,
            perf_config: None,
            gzip_config: None,
            route_policies: std::collections::HashMap::new(),
            firewall_config: None,
            anti_ddos_config: None,
            legacy_endpoints: false,
            deprecation_warnings: false,
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
}
