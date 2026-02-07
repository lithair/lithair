//! Lithair Framework - Core
//!
//! A high-performance, zero-dependency framework for building distributed applications in Rust.
//!
//! # Overview
//!
//! Lithair is a "shell" framework - you provide the business logic, we handle the infrastructure.
//! It fuses backend and database into a single binary, eliminating the complexity of traditional
//! 3-tier architectures while delivering ultra-high performance through in-memory state management
//! and event sourcing.
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use lithair_core::{Lithair, RaftstoneApplication};
//!
//! #[derive(Default)]
//! struct MyApp {
//!     counter: u64,
//! }
//!
//! impl RaftstoneApplication for MyApp {
//!     type State = Self;
//!     fn initial_state() -> Self::State { Self::default() }
//!     fn routes() -> Vec<lithair_core::http::Route<Self::State>> { vec![] }
//! }
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let app = MyApp::default();
//!     let framework = Lithair::new(app);
//!     framework.run("127.0.0.1:8080")?;
//!     Ok(())
//! }
//! ```
//!
//! # Architecture
//!
//! Lithair is built on four core modules:
//!
//! - [`http`] - Custom HTTP server with zero external dependencies
//! - [`engine`] - Event sourcing and state management
//! - [`serialization`] - JSON and binary serialization
//! - [`macros`] - Helper types for procedural macros
//!
//! # Features
//!
//! - **Zero Dependencies**: Everything built from scratch for maximum control
//! - **Ultra Performance**: Sub-millisecond reads, microsecond writes
//! - **Event Sourcing**: Built-in immutable event log with CQRS
//! - **Type Safety**: Rust's type system prevents common errors
//! - **Single Binary**: Deploy anywhere, no external services required

// Public modules - Core framework only
pub mod cluster;
pub mod config; // Configuration system with TOML support
pub mod consensus; // Distributed replication for DeclarativeModels
pub mod engine;
use crate::engine::events::Event; // Import Event trait for aggregate_id usage
pub mod frontend; // Revolutionary memory-first asset serving
pub mod http;
pub mod lifecycle;
pub mod logging; // Declarative logging system with standard log crate integration
pub mod mfa; // Multi-Factor Authentication (TOTP)
pub mod model; // Declarative model specifications
pub mod model_inspect; // Internal field inspection and optimization
pub mod raft; // Page-centric development support
pub mod rbac; // Role-Based Access Control system
pub mod schema;
pub mod security; // Core RBAC security - non-optional
pub mod serialization; // Declarative cluster management - PURE Lithair experience
pub mod session; // Session management with event sourcing

// Proxy and gateway functionality
pub mod cache;
pub mod integrations; // External source integration (blacklists, configs, etc.)
pub mod patterns; // Pattern matching utilities (wildcards, CIDR, domains)
pub mod proxy; // Generic proxy primitives (forward, reverse, transparent) // Caching strategies (LRU, etc.)

// Application server (unified multi-model server)
pub mod app;

// Admin UI (optional, feature-gated)
#[cfg(feature = "admin-ui")]
pub mod admin_ui;

// No internal examples - keep framework API clean

#[cfg(test)]
pub mod testing;

// Internal modules (not in public API)
mod macros;

// Re-exports of main types and traits
pub use engine::{RaftstoneApplication, StateEngine};
pub use http::{HttpServer, Route};
pub use model_inspect::Inspectable;
pub use security::{
    AuthContext, Permission, RBACMiddleware, Role, SecurityError, SecurityEvent, SecurityState,
    User,
};

// Re-exports from lithair-macros crate (when available)
// Note: Planned macro integration - uncomment when lithair-macros is ready
// pub use lithair_macros::{RaftstoneModel, RaftstoneApi};

// Main result type for the framework
pub type Result<T> = std::result::Result<T, Error>;

/// Main error type for Lithair framework
#[derive(Debug)]
pub enum Error {
    /// HTTP-related errors (parsing, server issues, etc.)
    HttpError(String),
    /// Serialization/deserialization errors
    SerializationError(String),
    /// File I/O and persistence errors
    PersistenceError(String),
    /// State management and engine errors
    EngineError(String),
    /// Generic framework errors
    FrameworkError(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::HttpError(msg) => write!(f, "HTTP Error: {}", msg),
            Error::SerializationError(msg) => write!(f, "Serialization Error: {}", msg),
            Error::PersistenceError(msg) => write!(f, "Persistence Error: {}", msg),
            Error::EngineError(msg) => write!(f, "Engine Error: {}", msg),
            Error::FrameworkError(msg) => write!(f, "Framework Error: {}", msg),
        }
    }
}

impl std::error::Error for Error {}

/// Main entry point for Lithair applications
///
/// This struct wraps your application and provides the framework infrastructure.
/// It supports both single-node (local) and multi-node (distributed) modes.
///
/// # Example
///
/// ```rust,ignore
/// use lithair_core::{Lithair, RaftstoneApplication};
///
/// #[derive(Default)]
/// struct MyApp;
///
/// impl RaftstoneApplication for MyApp {
///     type State = Self;
///     fn initial_state() -> Self::State { Self::default() }
///     fn routes() -> Vec<lithair_core::http::Route<Self::State>> { vec![] }
/// }
///
/// // Single node mode
/// let framework = Lithair::new(MyApp::default());
///
/// // Distributed mode
/// let cluster_config = lithair_core::raft::ClusterConfig {
///     node_id: 1,
///     listen_addr: "127.0.0.1:8080".parse().unwrap(),
///     peers: vec![],
///     data_dir: "./data/node1".to_string(),
/// };
/// let distributed_framework = Lithair::new_distributed(MyApp::default(), cluster_config);
/// ```
pub struct Lithair<A: RaftstoneApplication> {
    #[allow(dead_code)]
    application: A,
    database_path: Option<String>,
    // Mode selection - either local engine or distributed engine
    mode: LithairMode<A>,
}

/// Lithair execution mode
enum LithairMode<A: RaftstoneApplication> {
    /// Single-node mode with local engine
    Local(std::marker::PhantomData<A>),
    // Distributed mode not yet available (requires OpenRaft consensus integration)
    // Distributed { cluster_config: raft::ClusterConfig, _phantom: std::marker::PhantomData<A> },
}

impl<A: RaftstoneApplication + 'static> Lithair<A> {
    /// Create a new Lithair framework instance in single-node mode
    ///
    /// # Arguments
    ///
    /// * `app` - Your application instance that implements `RaftstoneApplication`
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// # use lithair_core::{Lithair, RaftstoneApplication};
    /// # #[derive(Default)]
    /// # struct MyApp;
    /// # impl RaftstoneApplication for MyApp {
    /// #     type State = Self;
    /// #     fn initial_state() -> Self::State { Self::default() }
    /// #     fn routes() -> Vec<lithair_core::http::Route<Self::State>> { vec![] }
    /// # }
    /// let app = MyApp::default();
    /// let framework = Lithair::new(app);
    /// ```
    pub fn new(app: A) -> Self {
        Self {
            application: app,
            database_path: None,
            mode: LithairMode::<A>::Local(std::marker::PhantomData),
        }
    }

    // Create a new Lithair framework instance in distributed mode
    //
    // # Arguments
    //
    // * `app` - Your application instance that implements `RaftstoneApplication`
    // * `cluster_config` - Configuration for the distributed cluster
    //
    // # Example
    //
    // ```rust
    // # use lithair_core::{Lithair, RaftstoneApplication};
    // # #[derive(Default)]
    // # struct MyApp;
    // # impl RaftstoneApplication for MyApp {
    // #     type State = Self;
    // #     fn initial_state() -> Self::State { Self::default() }
    // #     fn routes() -> Vec<lithair_core::http::Route<Self::State>> { vec![] }
    // # }
    // let app = MyApp::default();
    // let cluster_config = lithair_core::raft::ClusterConfig {
    //     node_id: 1,
    //     listen_addr: "127.0.0.1:8080".parse().unwrap(),
    //     peers: vec![],
    //     data_dir: "./data/node1".to_string(),
    // };
    // let framework = Lithair::new_distributed(app, cluster_config);
    // ```
    // Distributed mode not yet available - awaiting raft module stabilization
    /*pub fn new_distributed(app: A, cluster_config: raft::ClusterConfig) -> Self {
        Self {
            application: app,
            database_path: None,
            mode: LithairMode::<A>::Distributed {
                cluster_config,
                _phantom: std::marker::PhantomData,
            },
        }
    }*/

    /// Create a new Lithair framework instance with custom database path (single-node mode)
    ///
    /// # Arguments
    ///
    /// * `app` - Your application instance that implements `RaftstoneApplication`
    /// * `database_path` - Custom path for the database directory
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// # use lithair_core::{Lithair, RaftstoneApplication};
    /// # #[derive(Default)]
    /// # struct MyApp;
    /// # impl RaftstoneApplication for MyApp {
    /// #     type State = Self;
    /// #     fn initial_state() -> Self::State { Self::default() }
    /// #     fn routes() -> Vec<lithair_core::http::Route<Self::State>> { vec![] }
    /// # }
    /// let app = MyApp::default();
    /// let framework = Lithair::with_database_path(app, "my_app/data");
    /// ```
    pub fn with_database_path(app: A, database_path: &str) -> Self {
        Self {
            application: app,
            database_path: Some(database_path.to_string()),
            mode: LithairMode::<A>::Local(std::marker::PhantomData),
        }
    }

    /// Start the Lithair server
    ///
    /// This will:
    /// 1. Initialize the state engine with your application's initial state
    /// 2. Set up the HTTP server with your application's routes
    /// 3. Start listening for incoming connections
    ///
    /// # Arguments
    ///
    /// * `addr` - The address to bind to (e.g., "127.0.0.1:8080")
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The address is invalid or already in use
    /// - The server fails to start
    /// - The state engine fails to initialize
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// # use lithair_core::{Lithair, RaftstoneApplication};
    /// # #[derive(Default)]
    /// # struct MyApp;
    /// # impl RaftstoneApplication for MyApp {
    /// #     type State = Self;
    /// #     fn initial_state() -> Self::State { Self::default() }
    /// #     fn routes() -> Vec<lithair_core::http::Route<Self::State>> { vec![] }
    /// # }
    /// let framework = Lithair::new(MyApp::default());
    /// framework.run("127.0.0.1:8080")?;
    /// # Ok::<(), lithair_core::Error>(())
    /// ```
    pub fn run(self, addr: &str) -> Result<()> {
        match self.mode {
            LithairMode::Local(_) => self.run_local(addr),
            // Distributed mode not yet available
            // LithairMode::Distributed { ref cluster_config, .. } => {
            //     let cluster_config_clone = cluster_config.clone();
            //     self.run_distributed(addr, cluster_config_clone)
            // }
        }
    }

    /// Run in single-node local mode
    fn run_local(self, addr: &str) -> Result<()> {
        log::info!("Lithair framework starting on {} (Local Mode)", addr);
        log::info!("Application: {}", std::any::type_name::<A>());
        log::info!("State type: {}", std::any::type_name::<A::State>());

        // 1. Initialize the engine with event sourcing and persistence
        let engine = {
            // Get event deserializers from the application
            let deserializers = A::event_deserializers();

            if let Some(ref db_path) = self.database_path {
                // Use custom database path if provided
                // Allow runtime tuning via env vars for benchmarks
                let mut config = crate::engine::EngineConfig {
                    event_log_path: db_path.clone(),
                    ..crate::engine::EngineConfig::default()
                };
                if let Ok(v) = std::env::var("LITHAIR_FLUSH_EVERY") {
                    if let Ok(n) = v.parse::<usize>() {
                        config.flush_every = n.max(1);
                    }
                }
                if let Ok(v) = std::env::var("LITHAIR_FSYNC_ON_APPEND") {
                    let v = v.to_ascii_lowercase();
                    if v == "0" || v == "false" {
                        config.fsync_on_append = false;
                    }
                    if v == "1" || v == "true" {
                        config.fsync_on_append = true;
                    }
                }
                if let Ok(v) = std::env::var("LITHAIR_SNAPSHOT_EVERY") {
                    if let Ok(n) = v.parse::<u64>() {
                        config.snapshot_every = n.max(1);
                    }
                }
                crate::engine::Engine::<A>::new_with_deserializers(config, deserializers)
            } else {
                // Use default configuration with deserializers
                crate::engine::Engine::<A>::new_with_deserializers(
                    crate::engine::EngineConfig::default(),
                    deserializers,
                )
            }
        }
        .map_err(|e| crate::Error::EngineError(e.to_string()))?;

        // 2. Call application startup hook
        {
            // Use write_state to support both RwLock and Scc2 modes
            engine
                .write_state("global", |state| {
                    A::on_startup(state).map_err(|e| crate::Error::EngineError(e.to_string()))
                })
                .map_err(|e| crate::Error::EngineError(e.to_string()))??;
        }

        // 3. Create HTTP router with application routes
        let router = self.create_router(&engine)?;

        // 4. Start HTTP server with shared state architecture (InfluxDB pattern)
        // Create command channel for async event processing
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<crate::http::CommandMessage<A>>();
        let command_sender = std::sync::Arc::new(std::sync::Mutex::new(cmd_tx));

        // Share the engine between worker thread and read routes with RwLock for true async
        let engine_arc = std::sync::Arc::new(std::sync::RwLock::new(engine));
        let engine_for_worker = engine_arc.clone();
        let engine_for_routes = engine_arc.clone();

        // Start engine worker thread
        let worker_handle =
            std::thread::spawn(move || engine_worker_thread_shared(engine_for_worker, cmd_rx));

        let stateless_router = create_stateless_router_with_shared_engine::<A>(
            router,
            engine_for_routes,
            command_sender.clone(),
        );
        let server = crate::http::HttpServer::new().with_router(stateless_router);

        log::info!("Lithair framework initialized successfully");
        server.serve(addr).map_err(|e| crate::Error::HttpError(e.to_string()))?;

        // 5. Cleanup on shutdown - signal worker thread to stop
        drop(command_sender); // This will close the channel and stop the worker
        let _ = worker_handle
            .join()
            .map_err(|_| crate::Error::EngineError("Worker thread join error".to_string()));

        Ok(())
    }

    // Distributed mode not yet available - full implementation pending
    /*fn run_distributed(self, addr: &str, cluster_config: raft::ClusterConfig) -> Result<()> {
        println!("Lithair framework starting on {} (Distributed Mode - not yet implemented)", addr);
        println!("Application: {}", std::any::type_name::<A>());
        println!("State type: {}", std::any::type_name::<A::State>());
        println!("Node ID: {} | Data Dir: {}", cluster_config.node_id, cluster_config.data_dir);

        // For now, fall back to local mode with a warning
        println!("Distributed mode not yet fully implemented - running in single-node mode");
        println!("OpenRaft integration pending");

        // Run local mode but with different data directory
        let mut local_copy = Lithair {
            application: self.application,
            database_path: Some(cluster_config.data_dir),
            mode: LithairMode::<A>::Local(std::marker::PhantomData),
        };

        local_copy.run_local(addr)
    }*/

    /// Create HTTP router with application routes
    fn create_router(
        &self,
        _engine: &crate::engine::Engine<A>,
    ) -> Result<crate::http::EnhancedRouter<A>> {
        let mut router = crate::http::EnhancedRouter::new();

        // Get application-defined routes
        let app_routes = A::routes();
        let command_routes = A::command_routes();

        // Add application routes to the router
        for route in app_routes {
            router = router.route(route);
        }

        // Add command routes to the router
        for command_route in command_routes {
            router = router.command_route(command_route);
        }

        // Add default fallback routes if no custom routes are defined
        if router.route_count() == 0 {
            log::warn!("No custom routes defined, adding default routes");
            router = router
                .route(crate::http::Route::new(
                    crate::http::HttpMethod::GET,
                    "/",
                    |_req, _params, _state| crate::http::HttpResponse::ok().text("hello world"),
                ))
                .route(crate::http::Route::new(
                    crate::http::HttpMethod::GET,
                    "/hello",
                    |_req, _params, _state| crate::http::HttpResponse::ok().text("hello world"),
                ));
        }

        log::info!(
            "Registered {} routes ({} regular + {} command)",
            router.route_count(),
            A::routes().len(),
            A::command_routes().len()
        );

        Ok(router)
    }
}

/// Generate a beautiful HTML homepage for the Lithair application
#[allow(dead_code)]
fn create_home_page_html<State>(_state: &State) -> crate::http::HttpResponse
where
    State: std::any::Any,
{
    // Generate a beautiful homepage showcasing Lithair's capabilities
    let html = r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Lithair E-commerce - Single Binary Full Stack</title>
    <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: linear-gradient(135deg, #667eea 0%, #764ba2 100%); color: #333; min-height: 100vh; }
        .container { max-width: 1200px; margin: 0 auto; padding: 2rem; }
        .hero { background: white; border-radius: 20px; padding: 3rem; margin-bottom: 2rem; box-shadow: 0 20px 40px rgba(0,0,0,0.1); text-align: center; }
        .hero h1 { font-size: 3rem; margin-bottom: 1rem; background: linear-gradient(135deg, #667eea, #764ba2); -webkit-background-clip: text; -webkit-text-fill-color: transparent; }
        .hero p { font-size: 1.2rem; color: #666; margin-bottom: 2rem; }
        .stats { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 1rem; margin-bottom: 2rem; }
        .stat-card { background: white; padding: 2rem; border-radius: 15px; text-align: center; box-shadow: 0 10px 20px rgba(0,0,0,0.1); }
        .stat-number { font-size: 2.5rem; font-weight: bold; color: #667eea; }
        .stat-label { color: #666; margin-top: 0.5rem; }
        .api-section { background: white; border-radius: 20px; padding: 2rem; box-shadow: 0 20px 40px rgba(0,0,0,0.1); }
        .api-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(300px, 1fr)); gap: 1rem; margin-top: 1rem; }
        .api-card { background: #f8f9fa; border: 1px solid #e9ecef; border-radius: 10px; padding: 1rem; }
        .api-endpoint { font-family: 'Monaco', 'Menlo', monospace; background: #667eea; color: white; padding: 0.5rem 1rem; border-radius: 5px; display: inline-block; margin-bottom: 0.5rem; }
        .performance { background: linear-gradient(135deg, #11998e, #38ef7d); color: white; padding: 2rem; border-radius: 15px; margin-top: 1rem; text-align: center; }
        .btn { display: inline-block; padding: 1rem 2rem; background: #667eea; color: white; text-decoration: none; border-radius: 10px; margin: 0.5rem; transition: transform 0.2s; }
        .btn:hover { transform: translateY(-2px); }
    </style>
</head>
<body>
    <div class="container">
        <div class="hero">
            <h1>🚀 Lithair E-commerce</h1>
            <p>Single Binary • Full Stack • Zero Latency Database</p>
            <p><strong>"Nous SOMMES la base de données"</strong> - Embedded event-sourced architecture</p>
        </div>

        <div class="stats">
            <div class="stat-card">
                <div class="stat-number" id="products-count">8</div>
                <div class="stat-label">Products</div>
            </div>
            <div class="stat-card">
                <div class="stat-number" id="users-count">3</div>
                <div class="stat-label">Users</div>
            </div>
            <div class="stat-card">
                <div class="stat-number" id="orders-count">3</div>
                <div class="stat-label">Orders</div>
            </div>
            <div class="stat-card">
                <div class="stat-number" id="revenue-count">$3779.95</div>
                <div class="stat-label">Revenue</div>
            </div>
        </div>

        <div class="performance">
            <h2>⚡ Performance Highlights</h2>
            <p>• <strong>Sub-millisecond reads</strong> (in-memory state)</p>
            <p>• <strong>Zero network latency</strong> (embedded database)</p>
            <p>• <strong>Event sourcing</strong> with automatic snapshots</p>
            <p>• <strong>Single binary deployment</strong> with Kubernetes scalability</p>
        </div>

        <div class="api-section">
            <h2>🔗 REST API Endpoints</h2>
            <p>All endpoints return JSON and operate on in-memory state for maximum performance.</p>

            <div class="api-grid">
                <div class="api-card">
                    <div class="api-endpoint">GET /api/products</div>
                    <p>List all products with real-time inventory</p>
                </div>
                <div class="api-card">
                    <div class="api-endpoint">GET /api/products/1</div>
                    <p>Get specific product details</p>
                </div>
                <div class="api-card">
                    <div class="api-endpoint">GET /api/users</div>
                    <p>List all registered users</p>
                </div>
                <div class="api-card">
                    <div class="api-endpoint">GET /api/orders</div>
                    <p>View all orders and transactions</p>
                </div>
                <div class="api-card">
                    <div class="api-endpoint">GET /api/analytics/revenue</div>
                    <p>Real-time revenue analytics</p>
                </div>
                <div class="api-card">
                    <div class="api-endpoint">GET /api/health</div>
                    <p>System health and database stats</p>
                </div>
            </div>

            <div style="text-align: center; margin-top: 2rem;">
                <a href="/api/products" class="btn">🛍️ View Products</a>
                <a href="/api/analytics/revenue" class="btn">📊 Analytics</a>
                <a href="/api/health" class="btn">💚 Health Check</a>
            </div>
        </div>

        <script>
            // Fetch real-time data and update stats
            fetch('/api/health')
                .then(response => response.json())
                .then(data => {
                    if (data.database) {
                        document.getElementById('products-count').textContent = data.database.products;
                        document.getElementById('users-count').textContent = data.database.users;
                        document.getElementById('orders-count').textContent = data.database.orders;
                        if (data.performance && data.performance.total_revenue) {
                            document.getElementById('revenue-count').textContent = '$' + data.performance.total_revenue.toFixed(2);
                        }
                    }
                })
                .catch(err => console.log('Stats will show default values'));
        </script>
    </div>
</body>
</html>
"#;

    crate::http::HttpResponse::ok()
        .header("Content-Type", "text/html; charset=utf-8")
        .body(html.as_bytes().to_vec())
}

/// Generate the admin products management page
#[allow(dead_code)]
fn create_admin_products_page() -> crate::http::HttpResponse {
    let html = r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Lithair Admin - Product Management</title>
    <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: linear-gradient(135deg, #667eea 0%, #764ba2 100%); color: #333; min-height: 100vh; }
        .container { max-width: 1400px; margin: 0 auto; padding: 2rem; }
        .header { background: white; border-radius: 20px; padding: 2rem; margin-bottom: 2rem; box-shadow: 0 20px 40px rgba(0,0,0,0.1); }
        .header h1 { font-size: 2.5rem; margin-bottom: 0.5rem; background: linear-gradient(135deg, #667eea, #764ba2); -webkit-background-clip: text; -webkit-text-fill-color: transparent; }
        .header p { color: #666; font-size: 1.1rem; }
        .nav { margin-bottom: 2rem; }
        .nav a { display: inline-block; padding: 0.8rem 1.5rem; background: white; color: #667eea; text-decoration: none; border-radius: 10px; margin-right: 1rem; box-shadow: 0 5px 15px rgba(0,0,0,0.1); transition: transform 0.2s; }
        .nav a:hover { transform: translateY(-2px); }
        .main-content { display: grid; grid-template-columns: 1fr 400px; gap: 2rem; }
        .products-section { background: white; border-radius: 20px; padding: 2rem; box-shadow: 0 20px 40px rgba(0,0,0,0.1); }
        .form-section { background: white; border-radius: 20px; padding: 2rem; box-shadow: 0 20px 40px rgba(0,0,0,0.1); }
        .section-title { font-size: 1.5rem; margin-bottom: 1.5rem; color: #333; }
        .btn { display: inline-block; padding: 0.8rem 1.5rem; background: #667eea; color: white; text-decoration: none; border: none; border-radius: 8px; cursor: pointer; font-size: 1rem; transition: all 0.2s; }
        .btn:hover { background: #5a6fd8; transform: translateY(-1px); }
        .btn-danger { background: #e74c3c; }
        .btn-danger:hover { background: #c0392b; }
        .btn-success { background: #27ae60; }
        .btn-success:hover { background: #229954; }
        .btn-warning { background: #f39c12; }
        .btn-warning:hover { background: #e67e22; }
        .products-table { width: 100%; border-collapse: collapse; margin-top: 1rem; }
        .products-table th, .products-table td { padding: 1rem; text-align: left; border-bottom: 1px solid #eee; }
        .products-table th { background: #f8f9fa; font-weight: 600; }
        .products-table tr:hover { background: #f8f9fa; }
        .form-group { margin-bottom: 1.5rem; }
        .form-group label { display: block; margin-bottom: 0.5rem; font-weight: 600; color: #333; }
        .form-group input, .form-group textarea, .form-group select { width: 100%; padding: 0.8rem; border: 2px solid #e9ecef; border-radius: 8px; font-size: 1rem; transition: border-color 0.2s; }
        .form-group input:focus, .form-group textarea:focus, .form-group select:focus { outline: none; border-color: #667eea; }
        .form-group textarea { resize: vertical; min-height: 100px; }
        .actions { display: flex; gap: 0.5rem; }
        .loading { display: none; text-align: center; padding: 2rem; color: #666; }
        .success-message, .error-message { padding: 1rem; border-radius: 8px; margin-bottom: 1rem; display: none; }
        .success-message { background: #d4edda; color: #155724; border: 1px solid #c3e6cb; }
        .error-message { background: #f8d7da; color: #721c24; border: 1px solid #f5c6cb; }
        .stats { display: grid; grid-template-columns: repeat(auto-fit, minmax(150px, 1fr)); gap: 1rem; margin-bottom: 2rem; }
        .stat-card { background: white; padding: 1.5rem; border-radius: 15px; text-align: center; box-shadow: 0 10px 20px rgba(0,0,0,0.1); }
        .stat-number { font-size: 2rem; font-weight: bold; color: #667eea; }
        .stat-label { color: #666; margin-top: 0.5rem; font-size: 0.9rem; }
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>🛍️ Product Management</h1>
            <p>Manage your e-commerce products with real-time updates</p>
        </div>

        <div class="nav">
            <a href="/">🏠 Home</a>
            <a href="/admin/products">📦 Products</a>
            <a href="/admin/users">👥 Users</a>
            <a href="/admin/orders">🛒 Orders</a>
            <a href="/api/health">📊 API Health</a>
        </div>

        <div class="stats">
            <div class="stat-card">
                <div class="stat-number" id="total-products">-</div>
                <div class="stat-label">Total Products</div>
            </div>
            <div class="stat-card">
                <div class="stat-number" id="total-categories">-</div>
                <div class="stat-label">Categories</div>
            </div>
            <div class="stat-card">
                <div class="stat-number" id="avg-price">-</div>
                <div class="stat-label">Avg Price</div>
            </div>
        </div>

        <div class="main-content">
            <div class="products-section">
                <h2 class="section-title">Products List</h2>

                <div class="success-message" id="success-message"></div>
                <div class="error-message" id="error-message"></div>

                <div class="loading" id="loading">Loading products...</div>

                <table class="products-table" id="products-table">
                    <thead>
                        <tr>
                            <th>ID</th>
                            <th>Name</th>
                            <th>Price</th>
                            <th>Category</th>
                            <th>Actions</th>
                        </tr>
                    </thead>
                    <tbody id="products-tbody">
                        <!-- Products will be loaded here -->
                    </tbody>
                </table>
            </div>

            <div class="form-section">
                <h2 class="section-title" id="form-title">Add New Product</h2>

                <form id="product-form">
                    <input type="hidden" id="product-id" value="">

                    <div class="form-group">
                        <label for="product-name">Product Name</label>
                        <input type="text" id="product-name" required placeholder="Enter product name">
                    </div>

                    <div class="form-group">
                        <label for="product-price">Price ($)</label>
                        <input type="number" id="product-price" step="0.01" required placeholder="0.00">
                    </div>

                    <div class="form-group">
                        <label for="product-category">Category</label>
                        <select id="product-category" required>
                            <option value="">Select category</option>
                            <option value="Electronics">Electronics</option>
                            <option value="Fashion">Fashion</option>
                            <option value="Home">Home</option>
                            <option value="Sports">Sports</option>
                            <option value="Books">Books</option>
                            <option value="Other">Other</option>
                        </select>
                    </div>

                    <div class="form-group">
                        <label for="product-description">Description</label>
                        <textarea id="product-description" placeholder="Enter product description"></textarea>
                    </div>

                    <div class="actions">
                        <button type="submit" class="btn btn-success" id="submit-btn">➕ Add Product</button>
                        <button type="button" class="btn" id="cancel-btn" onclick="cancelEdit()" style="display: none;">❌ Cancel</button>
                    </div>
                </form>
            </div>
        </div>
    </div>

    <script>
        let products = [];
        let editingId = null;

        // Load products on page load
        document.addEventListener('DOMContentLoaded', function() {
            loadProducts();
        });

        // Load products from API
        async function loadProducts() {
            try {
                document.getElementById('loading').style.display = 'block';
                const response = await fetch('/api/products');
                products = await response.json();
                renderProducts();
                updateStats();
            } catch (error) {
                showError('Failed to load products: ' + error.message);
            } finally {
                document.getElementById('loading').style.display = 'none';
            }
        }

        // Render products table
        function renderProducts() {
            const tbody = document.getElementById('products-tbody');
            tbody.innerHTML = '';

            products.forEach(product => {
                const row = document.createElement('tr');
                row.innerHTML = `
                    <td>${product.id}</td>
                    <td>${product.name}</td>
                    <td>$${product.price.toFixed(2)}</td>
                    <td>${product.category}</td>
                    <td class="actions">
                        <button class="btn btn-warning" onclick="editProduct(${product.id})" title="Edit">✏️</button>
                        <button class="btn btn-danger" onclick="deleteProduct(${product.id})" title="Delete">🗑️</button>
                    </td>
                `;
                tbody.appendChild(row);
            });
        }

        // Update statistics
        function updateStats() {
            document.getElementById('total-products').textContent = products.length;

            const categories = [...new Set(products.map(p => p.category))];
            document.getElementById('total-categories').textContent = categories.length;

            const avgPrice = products.length > 0
                ? (products.reduce((sum, p) => sum + p.price, 0) / products.length).toFixed(2)
                : '0.00';
            document.getElementById('avg-price').textContent = '$' + avgPrice;
        }

        // Handle form submission
        document.getElementById('product-form').addEventListener('submit', async function(e) {
            e.preventDefault();

            const formData = {
                name: document.getElementById('product-name').value,
                price: parseFloat(document.getElementById('product-price').value),
                category: document.getElementById('product-category').value,
                description: document.getElementById('product-description').value
            };

            try {
                let response;
                if (editingId) {
                    // Update existing product
                    response = await fetch(`/api/products/${editingId}`, {
                        method: 'PUT',
                        headers: { 'Content-Type': 'application/json' },
                        body: JSON.stringify(formData)
                    });
                } else {
                    // Create new product
                    response = await fetch('/api/products', {
                        method: 'POST',
                        headers: { 'Content-Type': 'application/json' },
                        body: JSON.stringify(formData)
                    });
                }

                if (response.ok) {
                    showSuccess(editingId ? 'Product updated successfully!' : 'Product created successfully!');
                    resetForm();
                    loadProducts(); // Reload products
                } else {
                    throw new Error('Failed to save product');
                }
            } catch (error) {
                showError('Error saving product: ' + error.message);
            }
        });

        // Edit product
        function editProduct(id) {
            const product = products.find(p => p.id === id);
            if (!product) return;

            editingId = id;
            document.getElementById('product-id').value = id;
            document.getElementById('product-name').value = product.name;
            document.getElementById('product-price').value = product.price;
            document.getElementById('product-category').value = product.category;
            document.getElementById('product-description').value = product.description || '';

            document.getElementById('form-title').textContent = '✏️ Edit Product';
            document.getElementById('submit-btn').textContent = '💾 Update Product';
            document.getElementById('submit-btn').className = 'btn btn-warning';
            document.getElementById('cancel-btn').style.display = 'inline-block';
        }

        // Delete product
        async function deleteProduct(id) {
            if (!confirm('Are you sure you want to delete this product?')) return;

            try {
                const response = await fetch(`/api/products/${id}`, {
                    method: 'DELETE'
                });

                if (response.ok) {
                    showSuccess('Product deleted successfully!');
                    loadProducts(); // Reload products
                } else {
                    throw new Error('Failed to delete product');
                }
            } catch (error) {
                showError('Error deleting product: ' + error.message);
            }
        }

        // Cancel edit
        function cancelEdit() {
            resetForm();
        }

        // Reset form
        function resetForm() {
            editingId = null;
            document.getElementById('product-form').reset();
            document.getElementById('product-id').value = '';
            document.getElementById('form-title').textContent = '➕ Add New Product';
            document.getElementById('submit-btn').textContent = '➕ Add Product';
            document.getElementById('submit-btn').className = 'btn btn-success';
            document.getElementById('cancel-btn').style.display = 'none';
        }

        // Show success message
        function showSuccess(message) {
            const element = document.getElementById('success-message');
            element.textContent = message;
            element.style.display = 'block';
            setTimeout(() => element.style.display = 'none', 3000);
        }

        // Show error message
        function showError(message) {
            const element = document.getElementById('error-message');
            element.textContent = message;
            element.style.display = 'block';
            setTimeout(() => element.style.display = 'none', 5000);
        }
    </script>
</body>
</html>
"#;

    crate::http::HttpResponse::ok()
        .header("Content-Type", "text/html; charset=utf-8")
        .body(html.as_bytes().to_vec())
}

// Lithair API integration - using real state engine instead of global state

/// Engine worker thread - processes commands sequentially like InfluxDB/TiKV
#[allow(dead_code)]
fn engine_worker_thread<A: crate::engine::RaftstoneApplication>(
    engine: crate::engine::Engine<A>,
    cmd_rx: std::sync::mpsc::Receiver<crate::http::CommandMessage<A>>,
) -> Result<()> {
    log::info!("Lithair command worker thread started (InfluxDB pattern)");

    // Process commands sequentially - no race conditions!
    while let Ok(cmd_msg) = cmd_rx.recv() {
        // Apply the event to the engine
        let key = cmd_msg.event.aggregate_id().unwrap_or_else(|| "global".to_string());
        let result = match engine.apply_event(key, cmd_msg.event) {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Engine error: {}", e)),
        };

        // Send response back to the HTTP handler
        if cmd_msg.response_sender.send(result).is_err() {
            // HTTP handler dropped, continue processing other commands
            continue;
        }
    }

    // Channel closed, shutdown engine
    log::debug!("Command worker shutting down...");
    engine.shutdown().map_err(|e| crate::Error::EngineError(e.to_string()))?;
    log::info!("Engine worker thread stopped");
    Ok(())
}

/// Shared engine worker thread - processes commands with shared state access using RwLock
fn engine_worker_thread_shared<A: crate::engine::RaftstoneApplication>(
    engine_arc: std::sync::Arc<std::sync::RwLock<crate::engine::Engine<A>>>,
    cmd_rx: std::sync::mpsc::Receiver<crate::http::CommandMessage<A>>,
) -> Result<()> {
    log::info!("Lithair SHARED command worker thread started (async RwLock pattern)");

    // Process commands sequentially - shared state with read routes!
    while let Ok(cmd_msg) = cmd_rx.recv() {
        log::debug!("Worker thread received command");
        let result = {
            log::debug!("Attempting to write-lock engine...");
            let engine_guard = match engine_arc.write() {
                Ok(guard) => {
                    log::debug!("Engine write-locked successfully");
                    guard
                }
                Err(_) => {
                    log::error!("Engine write-lock failed");
                    let _ = cmd_msg.response_sender.send(Err("Engine lock error".to_string()));
                    continue;
                }
            };

            // Apply the event to the shared engine - this updates the SAME state that read routes access!
            log::debug!("Applying event to engine...");
            let key = cmd_msg.event.aggregate_id().unwrap_or_else(|| "global".to_string());
            match engine_guard.apply_event(key, cmd_msg.event) {
                Ok(_) => {
                    log::debug!("Event applied to SHARED engine state successfully");
                    Ok(())
                }
                Err(e) => {
                    log::error!("Failed to apply event to shared state: {}", e);
                    Err(format!("Engine error: {}", e))
                }
            }
        };

        // Send response back to the HTTP handler
        log::debug!("Sending response back to HTTP handler...");
        if cmd_msg.response_sender.send(result).is_err() {
            log::warn!("HTTP handler dropped, continuing with other commands");
            // HTTP handler dropped, continue processing other commands
            continue;
        }
        log::debug!("Response sent successfully");
    }

    // Channel closed, shutdown
    log::debug!("Shared command worker shutting down...");
    if let Ok(_engine_guard) = engine_arc.read() {
        // Note: We can't move out of the Arc<RwLock<>> for shutdown
        log::info!("Shared engine worker thread stopped (engine still accessible for reads)");
    }
    Ok(())
}

/// Convert enhanced router to stateless router using async channels (InfluxDB pattern)
#[allow(dead_code)]
fn create_stateless_router_with_channels<A: crate::engine::RaftstoneApplication + 'static>(
    enhanced_router: crate::http::EnhancedRouter<A>,
    state_engine: std::sync::Arc<crate::engine::StateEngine<A::State>>,
    command_sender: crate::http::CommandSender<A>,
) -> crate::http::Router<()> {
    let mut router = crate::http::Router::new();
    let enhanced_router = std::sync::Arc::new(enhanced_router);

    // Create a custom handler that delegates to the enhanced router
    let state_for_handler = state_engine.clone();
    let router_for_handler = enhanced_router.clone();
    let cmd_sender_for_handler = command_sender.clone();

    // Route all requests through enhanced router with command sender
    router = router.not_found(move |req, _params, _state| {
        state_for_handler
            .with_state(|app_state| {
                router_for_handler.handle_request(req, app_state, &cmd_sender_for_handler)
            })
            .unwrap_or_else(|_| {
                crate::http::HttpResponse::internal_server_error().text("State engine error")
            })
    });

    router
}

/// Convert enhanced router to stateless router with SHARED engine access using RwLock
fn create_stateless_router_with_shared_engine<A: crate::engine::RaftstoneApplication + 'static>(
    enhanced_router: crate::http::EnhancedRouter<A>,
    shared_engine: std::sync::Arc<std::sync::RwLock<crate::engine::Engine<A>>>,
    command_sender: crate::http::CommandSender<A>,
) -> crate::http::Router<()> {
    let mut router = crate::http::Router::new();
    let enhanced_router = std::sync::Arc::new(enhanced_router);

    // Create a custom handler that delegates to the enhanced router with SHARED engine access
    let engine_for_handler = shared_engine.clone();
    let router_for_handler = enhanced_router.clone();
    let cmd_sender_for_handler = command_sender.clone();

    // Route all requests through enhanced router with SHARED engine access using read-lock
    router = router.not_found(move |req, _params, _state| {
        log::debug!("HTTP Handler: Processing request: {} {}", req.method(), req.path());

        // Use a two-phase approach to avoid deadlock:
        // 1. Check if it's a command route and handle WITHOUT read-lock
        // 2. For read routes, acquire read-lock and get state

        // Phase 1: Try to handle as command route (no read-lock needed)
        let enhanced_router_clone = router_for_handler.clone();
        let cmd_sender_clone = cmd_sender_for_handler.clone();

        // Check if any command route matches this request (method + path)
        if enhanced_router_clone.has_matching_command_route(req) {
            // Create a dummy state for command route execution (commands don't use state)
            let dummy_state = A::initial_state();
            let response =
                enhanced_router_clone.handle_request(req, &dummy_state, &cmd_sender_clone);
            log::debug!("HTTP Handler: Command route matched - executed WITHOUT read-lock");
            return response;
        }

        // Phase 2: Handle as read route with read-lock
        log::debug!("HTTP Handler: Attempting to acquire read-lock for read-only request");
        let response = {
            let engine_guard = match engine_for_handler.read() {
                Ok(guard) => {
                    log::debug!("HTTP Handler: Read-lock acquired successfully");
                    guard
                }
                Err(_) => {
                    log::error!("HTTP Handler: Read-lock failed");
                    return crate::http::HttpResponse::internal_server_error()
                        .text("Engine read-lock error");
                }
            };

            // Get fresh state from the shared engine - SCOPE THE LOCK!
            // Use read_state to support both RwLock and Scc2 modes
            // For RwLock, aggregate_id is ignored. For Scc2, we default to "global".
            let result = engine_guard.read_state("global", |fresh_app_state| {
                // Use the fresh state that includes all applied events!
                enhanced_router_clone.handle_request(req, fresh_app_state, &cmd_sender_clone)
            });
            log::debug!("HTTP Handler: About to release read-lock");
            // Map EngineResult to what the router expects (HttpResponse)
            result.unwrap_or_else(|| {
                crate::http::HttpResponse::internal_server_error().text("State access error")
            })
        }; // READ LOCK IS RELEASED HERE!
        log::debug!("HTTP Handler: Read-lock released");

        response
    });

    router
}

/// Initialize a new Lithair application and run it
///
/// This is a convenience function that combines `Lithair::new()` and `run()`.
///
/// # Example
///
/// ```rust,ignore
/// use lithair_core::{run_app, RaftstoneApplication};
///
/// #[derive(Default)]
/// struct MyApp;
///
/// impl RaftstoneApplication for MyApp {
///     type State = Self;
///     fn initial_state() -> Self::State { Self::default() }
///     fn routes() -> Vec<lithair_core::http::Route<Self::State>> { vec![] }
/// }
///
/// run_app(MyApp::default(), "127.0.0.1:8080")?;
/// # Ok::<(), lithair_core::Error>(())
/// ```
pub fn run_app<A: RaftstoneApplication + 'static>(app: A, addr: &str) -> Result<()> {
    Lithair::new(app).run(addr)
}
