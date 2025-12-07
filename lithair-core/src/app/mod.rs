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
pub mod router;
pub mod model_handler;

pub use builder::LithairServerBuilder;
pub use model_handler::{ModelHandler, DeclarativeModelHandler};

/// Model registration with handler
pub struct ModelRegistration {
    pub name: String,
    pub base_path: String,
    pub data_path: String,
    pub handler: Arc<dyn ModelHandler>,
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
}

/// A CRUD operation to be submitted through Raft consensus
#[derive(Debug)]
pub struct RaftCrudOperation {
    pub operation: crate::cluster::CrudOperation,
    pub response_tx: tokio::sync::oneshot::Sender<Result<serde_json::Value, String>>,
}

/// Type alias for async route handlers
pub type RouteHandler = Arc<
    dyn Fn(hyper::Request<hyper::body::Incoming>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<hyper::Response<http_body_util::Full<bytes::Bytes>>>> + Send>>
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
    dyn Fn(String) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Arc<dyn ModelHandler>>> + Send>
    > + Send + Sync
>;

/// Model registration info with factory
pub struct ModelRegistrationInfo {
    pub name: String,
    pub base_path: String,
    pub data_path: String,
    pub factory: ModelFactory,
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

    /// Start the server
    pub async fn serve(mut self) -> Result<()> {
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
        log::info!("   Sessions: {}", if self.config.sessions.enabled { "enabled" } else { "disabled" });
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
            log::info!("   Raft auth: {}", if self.config.raft.auth_required { "enabled" } else { "disabled" });

            let raft_state = Arc::new(RaftLeadershipState::new(node_id, port, self.cluster_peers.clone()));

            if raft_state.is_leader() {
                log::info!("👑 THIS NODE IS THE LEADER");
            } else {
                log::info!("👥 This node is a FOLLOWER (leader port: {})", raft_state.get_leader_port());
            }

            self.raft_state = Some(raft_state);

            // Initialize replication batcher with peers
            if let Some(ref batcher) = self.replication_batcher {
                batcher.initialize(&self.cluster_peers).await;
                log::info!("📊 Replication batcher initialized with {} peers", self.cluster_peers.len());
            }

            // Log WAL and snapshot status
            if self.wal.is_some() {
                log::info!("💾 WAL enabled for durability");
            }
            if self.snapshot_manager.is_some() {
                log::info!("📸 Snapshot manager enabled for resync");
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

                    let heartbeat_interval = Duration::from_secs(raft_config.heartbeat_interval_secs);

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
                    async move {
                        server.handle_request(req).await
                    }
                });

                if let Err(err) = hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, service)
                    .await
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
        use http_body_util::Full;
        use bytes::Bytes;

        let method = req.method().clone();
        let path = req.uri().path().to_string();

        log::debug!("{} {}", method, path);

        // 🗳️ Raft Cluster: Check for write redirection and Raft endpoints
        if let Some(ref raft_state) = self.raft_state {
            let heartbeat_path = self.config.raft.heartbeat_path();
            let leader_path = self.config.raft.leader_path();

            // Raft heartbeat endpoint
            if path == heartbeat_path && method == hyper::Method::POST {
                let provided_token = req.headers()
                    .get("X-Raft-Token")
                    .and_then(|v| v.to_str().ok());

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
                let body_bytes = req.into_body().collect().await.map(|c| c.to_bytes()).unwrap_or_default();
                if let Ok(heartbeat) = serde_json::from_slice::<serde_json::Value>(&body_bytes) {
                    let leader_id = heartbeat.get("leader_id").and_then(|v| v.as_u64()).unwrap_or(0);
                    let leader_port = heartbeat.get("leader_port").and_then(|v| v.as_u64()).unwrap_or(0) as u16;

                    if !raft_state.is_leader() && leader_id != raft_state.current_leader_id.load(std::sync::atomic::Ordering::Relaxed) {
                        log::info!("💓 Heartbeat: updating leader to node {} (port {})", leader_id, leader_port);
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
                let provided_token = req.headers()
                    .get("X-Raft-Token")
                    .and_then(|v| v.to_str().ok());

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
            let is_write = matches!(method, hyper::Method::POST | hyper::Method::PUT | hyper::Method::DELETE);
            let is_internal = path.starts_with("/internal/");

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
                            let frontend_server = crate::frontend::FrontendServer::new_scc2(engine.clone());

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
                                    let full_response = hyper::Response::from_parts(parts, Full::new(bytes));
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
        let base_path = message.get("base_path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Get the item data
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

        // Try to match by base_path if provided
        let handler = if let Some(ref path) = base_path {
            models.iter().find(|m| m.base_path == *path || m.base_path == format!("/api/{}", path))
        } else {
            // Fallback: use first model (typical single-model clusters)
            models.first()
        };

        if let Some(model) = handler {
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
        let base_path = message.get("base_path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

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

        let batch_id = message.get("batch_id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        // Find the matching model handler
        let models = self.models.read().await;

        let handler = if let Some(ref path) = base_path {
            models.iter().find(|m| m.base_path == *path || m.base_path == format!("/api/{}", path))
        } else {
            models.first()
        };

        if let Some(model) = handler {
            match model.handler.apply_replicated_items_json(items).await {
                Ok(count) => {
                    log::debug!("📥 Bulk replication applied: {} items for model {} (batch: {})", count, model.name, batch_id);
                    Ok(hyper::Response::builder()
                        .status(hyper::StatusCode::OK)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(format!(r#"{{"status":"ok","count":{}}}"#, count))))
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
        let base_path = message.get("base_path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

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
            models.iter().find(|m| m.base_path == *path || m.base_path == format!("/api/{}", path))
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
        let base_path = message.get("base_path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

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
            models.iter().find(|m| m.base_path == *path || m.base_path == format!("/api/{}", path))
        } else {
            models.first()
        };

        if let Some(model) = handler {
            match model.handler.apply_replicated_delete_json(&id).await {
                Ok(deleted) => {
                    log::debug!("📥 Replication DELETE applied for {} in model {} (deleted: {})", id, model.name, deleted);
                    Ok(hyper::Response::builder()
                        .status(hyper::StatusCode::OK)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(format!(r#"{{"status":"ok","deleted":{}}}"#, deleted))))
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
        use http_body_util::Full;
        use http_body_util::BodyExt;

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
                        .body(Full::new(Bytes::from(format!(r#"{{"error":"Invalid request: {}"}}"#, e))))
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

        // Append entries to local log
        let entries_count = request.entries.len();
        consensus_log.append_entries(request.entries.clone(), request.leader_commit).await;

        log::debug!("📥 Received {} entries from leader {}, commit_index={}",
            entries_count, request.leader_id, request.leader_commit);

        // Apply committed entries that we haven't applied yet
        let unapplied = consensus_log.get_unapplied_entries().await;
        for entry in unapplied {
            log::debug!("🔄 Applying entry index={}", entry.log_id.index);
            match self.apply_crud_operation(&entry.operation).await {
                Ok(_) => {
                    consensus_log.mark_applied(entry.log_id.index);
                    log::debug!("✅ Applied entry index={}", entry.log_id.index);
                }
                Err(e) => {
                    log::error!("❌ Failed to apply entry index={}: {}", entry.log_id.index, e);
                    // Continue anyway - don't block replication
                }
            }
        }

        // Send success response
        let response = crate::cluster::consensus_log::AppendEntriesResponse {
            term: consensus_log.current_term(),
            success: true,
            last_log_index: consensus_log.last_index().await,
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
                    .body(Full::new(Bytes::from(format!(r#"{{"error":"Failed to read snapshot: {}"}}"#, e))))
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
        let body_bytes = match req.into_body().collect().await.map(|c| c.to_bytes()) {
            Ok(bytes) => bytes,
            Err(e) => {
                return Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::BAD_REQUEST)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(format!(r#"{{"error":"Failed to read body: {}"}}"#, e))))
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

        // Install the snapshot
        let mut mgr = snapshot_manager.write().await;
        match mgr.install_snapshot(meta.clone(), &body_bytes) {
            Ok(snapshot_data) => {
                log::info!("📸 Snapshot installed: index={}, term={}", last_included_index, term);

                // Apply snapshot data to models
                let models = self.models.read().await;
                for (model_path, json_data) in &snapshot_data.models {
                    if let Some(model) = models.iter().find(|m| m.base_path == *model_path) {
                        let items: Vec<serde_json::Value> = serde_json::from_str(json_data).unwrap_or_default();
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
                    follower_info["last_replicated_index"] = serde_json::json!(stats.last_replicated_index);
                    follower_info["last_latency_ms"] = serde_json::json!(stats.last_latency_ms);
                    follower_info["pending_count"] = serde_json::json!(stats.pending_count);
                    follower_info["consecutive_failures"] = serde_json::json!(stats.consecutive_failures);
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
            .body(Full::new(Bytes::from(serde_json::to_string_pretty(&health_data).unwrap_or_default())))
            .unwrap())
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
        use http_body_util::Full;
        use bytes::Bytes;

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
                        .body(Full::new(Bytes::from(serde_json::to_string_pretty(&response).unwrap())))
                        .unwrap())
                } else {
                    Ok(hyper::Response::builder()
                        .status(404)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(format!(r#"{{"error":"Model '{}' not found"}}"#, name))))
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
                        .header("Content-Disposition", format!("attachment; filename=\"{}_export.json\"", name))
                        .body(Full::new(Bytes::from(serde_json::to_string_pretty(&export).unwrap())))
                        .unwrap())
                } else {
                    Ok(hyper::Response::builder()
                        .status(404)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(format!(r#"{{"error":"Model '{}' not found"}}"#, name))))
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
                        .body(Full::new(Bytes::from(serde_json::to_string_pretty(&history).unwrap())))
                        .unwrap())
                } else {
                    Ok(hyper::Response::builder()
                        .status(404)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(format!(r#"{{"error":"Model '{}' not found"}}"#, name))))
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
                                .body(Full::new(Bytes::from(serde_json::to_string_pretty(&response).unwrap())))
                                .unwrap())
                        }
                        Err(e) => {
                            Ok(hyper::Response::builder()
                                .status(400)
                                .header("Content-Type", "application/json")
                                .body(Full::new(Bytes::from(format!(r#"{{"error":"{}"}}"#, e))))
                                .unwrap())
                        }
                    }
                } else {
                    Ok(hyper::Response::builder()
                        .status(404)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(format!(r#"{{"error":"Model '{}' not found"}}"#, name))))
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
            _ => {
                Ok(hyper::Response::builder()
                    .status(404)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(r#"{"error":"Unknown data admin endpoint"}"#)))
                    .unwrap())
            }
        }
    }

    /// Handle embedded data admin UI request (serves the dashboard HTML)
    /// Only available when the `admin-ui` feature is enabled
    #[cfg(feature = "admin-ui")]
    async fn handle_data_admin_ui_request(
        &self,
    ) -> Result<hyper::Response<http_body_util::Full<bytes::Bytes>>> {
        use http_body_util::Full;
        use bytes::Bytes;

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
        let is_update = (method == hyper::Method::PUT || method == hyper::Method::PATCH) && !segments.is_empty();
        let is_delete = method == hyper::Method::DELETE && !segments.is_empty();
        let is_write = is_create || is_bulk_create || is_update || is_delete;

        // Extract the resource ID for UPDATE and DELETE operations
        let resource_id = if is_update || is_delete {
            segments.first().map(|s| s.to_string())
        } else {
            None
        };

        // ==================== CLUSTER MODE WITH CONSENSUS LOG ====================
        // If we have a consensus log (cluster mode), write operations go through Raft
        if is_write && self.consensus_log.is_some() && !self.cluster_peers.is_empty() {
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
            let body_json: serde_json::Value = serde_json::from_slice(&body_bytes)
                .unwrap_or(serde_json::Value::Null);

            // Create the CRUD operation
            let operation = if is_create {
                crate::cluster::CrudOperation::Create {
                    model_path: model.base_path.clone(),
                    data: body_json.clone(),
                }
            } else if is_update {
                let id = resource_id.clone().unwrap_or_default();
                crate::cluster::CrudOperation::Update {
                    model_path: model.base_path.clone(),
                    id,
                    data: body_json.clone(),
                }
            } else if is_delete {
                let id = resource_id.clone().unwrap_or_default();
                crate::cluster::CrudOperation::Delete {
                    model_path: model.base_path.clone(),
                    id,
                }
            } else {
                // Bulk create - for now handle as single operation
                // TODO: Handle bulk properly with BatchOperation
                crate::cluster::CrudOperation::Create {
                    model_path: model.base_path.clone(),
                    data: body_json.clone(),
                }
            };

            // Step 1: Append to local consensus log
            let log_entry = consensus_log.append(operation.clone()).await;
            let entry_index = log_entry.log_id.index;
            log::debug!("📝 Appended to log: index={}, term={}", entry_index, log_entry.log_id.term);

            // Step 2: Write to WAL for durability (before replication)
            if let Some(ref wal) = self.wal {
                if let Err(e) = wal.append(&log_entry).await {
                    log::error!("❌ WAL write failed: {}", e);
                    return Ok(hyper::Response::builder()
                        .status(503)
                        .body(Full::new(Bytes::from(format!(r#"{{"error":"WAL write failed: {}"}}"#, e))))
                        .unwrap());
                }
                log::debug!("💾 WAL entry written: index={}", entry_index);
            }

            // Step 3: Queue for batcher (healthy followers get immediate, lagging get batched)
            if let Some(ref batcher) = self.replication_batcher {
                batcher.queue_entry(log_entry.clone()).await;
            }

            // Step 4: Replicate to followers (SYNCHRONOUS - waits for majority)
            let term = consensus_log.current_term();
            let node_id = self.node_id.unwrap_or(0);
            let current_commit = consensus_log.commit_index();

            match Self::replicate_log_entries_to_followers(
                &self.cluster_peers,
                vec![log_entry],
                current_commit,
                term,
                node_id,
                self.replication_batcher.clone(),
            ).await {
                Ok(new_commit_index) => {
                    // Step 3: Commit the entry
                    consensus_log.commit(new_commit_index);
                    log::debug!("✅ Committed index: {}", new_commit_index);

                    // Step 4: Apply to local state machine
                    match self.apply_crud_operation(&operation).await {
                        Ok(result) => {
                            consensus_log.mark_applied(entry_index);
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
                                .body(Full::new(Bytes::from(format!(r#"{{"error":"Apply failed: {}"}}"#, e))))
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
            Err(_) => {
                Ok(hyper::Response::builder()
                    .status(500)
                    .body(Full::new(Bytes::from(r#"{"error":"Internal error"}"#)))
                    .unwrap())
            }
        }
    }

    // NOTE: The old fire-and-forget replication methods (replicate_to_followers,
    // replicate_update_to_followers, replicate_delete_to_followers) have been removed.
    // They were replaced by the Raft consensus log approach which guarantees ordering.
    // See: replicate_log_entries_to_followers() and handle_raft_append_entries()

    /// Apply a CRUD operation from the consensus log to the appropriate model
    /// This is called when a log entry is committed and needs to be applied to the state machine
    pub async fn apply_crud_operation(&self, operation: &crate::cluster::CrudOperation) -> Result<serde_json::Value, String> {
        use crate::cluster::CrudOperation;

        let models = self.models.read().await;

        match operation {
            CrudOperation::Create { model_path, data } => {
                // Find the model by base_path
                let model = models.iter()
                    .find(|m| model_path.starts_with(&m.base_path))
                    .ok_or_else(|| format!("Model not found for path: {}", model_path))?;

                model.handler.apply_replicated_item_json(data.clone()).await?;
                Ok(data.clone())
            }
            CrudOperation::Update { model_path, id, data } => {
                let model = models.iter()
                    .find(|m| model_path.starts_with(&m.base_path))
                    .ok_or_else(|| format!("Model not found for path: {}", model_path))?;

                model.handler.apply_replicated_update_json(id, data.clone()).await?;
                Ok(data.clone())
            }
            CrudOperation::Delete { model_path, id } => {
                let model = models.iter()
                    .find(|m| model_path.starts_with(&m.base_path))
                    .ok_or_else(|| format!("Model not found for path: {}", model_path))?;

                model.handler.apply_replicated_delete_json(id).await?;
                Ok(serde_json::json!({"deleted": id}))
            }
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
            .timeout(std::time::Duration::from_secs(2)) // Short timeout - don't wait for slow nodes
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

        // Spawn parallel requests to all followers
        let mut handles = Vec::with_capacity(peers.len());
        for peer in peers {
            let endpoint = format!("http://{}/_raft/append", peer);
            let client = client.clone();
            let request = request.clone();
            let peer_name = peer.clone();
            let batcher_clone = batcher.clone();

            handles.push(tokio::spawn(async move {
                let start = std::time::Instant::now();
                let result = match client.post(&endpoint).json(&request).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        if let Ok(response) = resp.json::<crate::cluster::consensus_log::AppendEntriesResponse>().await {
                            if response.success {
                                log::debug!("📤 Log entry replicated to {} successfully", peer_name);
                                (peer_name.clone(), true, response.last_log_index)
                            } else {
                                (peer_name.clone(), false, 0)
                            }
                        } else {
                            (peer_name.clone(), false, 0)
                        }
                    }
                    Ok(resp) => {
                        log::warn!("⚠️ Replicate log to {} failed: status {}", peer_name, resp.status());
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

        // Collect results as they complete, return early when majority reached
        // Uses FuturesUnordered for efficient polling - first response wins!
        use futures::stream::{FuturesUnordered, StreamExt};
        let mut futures: FuturesUnordered<_> = handles.into_iter().collect();

        let mut success_count = 0usize;
        let mut completed = 0usize;

        // Set a global timeout for the entire operation
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);

        while let Ok(Some(result)) = tokio::time::timeout_at(deadline, futures.next()).await {
            completed += 1;
            if let Ok((peer, success)) = result {
                if success {
                    success_count += 1;
                    log::debug!("✓ {}/{} followers acknowledged (need {})", success_count, peers.len(), needed_from_followers);

                    // Early return: majority reached! Don't wait for slow nodes.
                    if success_count >= needed_from_followers {
                        let new_commit = entries.last().map(|e| e.log_id.index).unwrap_or(leader_commit);
                        log::debug!("✅ Majority reached ({}/{}), committing index {}",
                            success_count + 1, total_nodes, new_commit);
                        return Ok(new_commit);
                    }
                } else {
                    log::debug!("✗ Follower {} failed", peer);
                }
            }

            // Early failure check: if we can't reach majority even if all remaining succeed
            let remaining = peers.len() - completed;
            if success_count + remaining < needed_from_followers {
                break;
            }
        }

        // Didn't reach majority (either timeout or not enough successes)
        Err(format!(
            "Failed to reach majority: {} of {} nodes responded successfully (need {})",
            success_count + 1, // +1 for leader
            total_nodes,
            majority
        ))
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
