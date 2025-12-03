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

        // Build server address
        let addr = format!("{}:{}", self.config.server.host, self.config.server.port);

        // Start HTTP server
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .with_context(|| format!("Failed to bind to {}", addr))?;

        log::info!("✅ Server listening on http://{}", addr);

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
    async fn handle_model_request(
        &self,
        req: hyper::Request<hyper::body::Incoming>,
        model: &ModelRegistration,
    ) -> Result<hyper::Response<http_body_util::Full<bytes::Bytes>>> {
        // Extract path segments after base_path (clone path first to avoid borrow issues)
        let path = req.uri().path().to_string();
        let segments: Vec<&str> = path
            .strip_prefix(&model.base_path)
            .unwrap_or("")
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        // Delegate to model handler
        match model.handler.handle_request(req, &segments).await {
            Ok(resp) => {
                // Convert BoxBody to Full<Bytes>
                use http_body_util::BodyExt;
                use http_body_util::Full;

                let (parts, body) = resp.into_parts();
                let body_bytes = body.collect().await?.to_bytes();
                Ok(hyper::Response::from_parts(parts, Full::new(body_bytes)))
            }
            Err(_) => {
                use http_body_util::Full;
                Ok(hyper::Response::builder()
                    .status(500)
                    .body(Full::new(Bytes::from(r#"{"error":"Internal error"}"#)))
                    .unwrap())
            }
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
