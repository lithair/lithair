//! Lithair Framework - Core
//!
//! A declarative, memory-first web framework for building applications in Rust.
//!
//! # Overview
//!
//! Lithair is a "shell" framework - you define your data models with annotations,
//! and Lithair generates the complete backend: REST endpoints, event sourcing,
//! sessions, RBAC, and distributed consensus. It fuses backend and database into
//! a single binary, eliminating the complexity of traditional 3-tier architectures.
//!
//! # Quick Start
//!
//! Add `lithair-core` to your `Cargo.toml` (includes derive macros by default):
//!
//! ```toml,ignore
//! [dependencies]
//! lithair-core = "0.1"
//! ```
//!
//! Then define your model and start the server:
//!
//! ```rust,ignore
//! use lithair_core::prelude::*;
//! use serde::{Serialize, Deserialize};
//!
//! #[derive(DeclarativeModel, Serialize, Deserialize, Clone, Debug)]
//! struct Product {
//!     id: String,
//!     name: String,
//!     price: f64,
//! }
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     LithairServer::new()
//!         .with_port(3000)
//!         .with_model::<Product>("./data/products", "/api/products")
//!         .serve()
//!         .await
//! }
//! ```
//!
//! # Architecture
//!
//! Lithair is built on these core modules:
//!
//! - [`app`] - Declarative server builder (`LithairServer`)
//! - [`http`] - HTTP server built on hyper
//! - [`engine`] - Event sourcing and state management
//! - [`serialization`] - JSON (simd-json) and binary (rkyv) serialization
//! - [`rbac`] - Role-based access control
//! - [`session`] - Session management with event sourcing
//!
//! # Features
//!
//! - **Declarative**: Define models, get full REST APIs automatically
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
pub mod frontend; // Memory-first static file serving
pub mod http;
pub mod lifecycle;
pub mod logging; // Declarative logging system with standard log crate integration
pub mod mfa; // Multi-Factor Authentication (TOTP)
pub mod model; // Declarative model specifications
pub mod model_inspect; // Internal field inspection and optimization
pub mod raft; // OpenRaft consensus integration
pub mod rbac; // Role-Based Access Control system
pub mod schema;
pub mod security; // Core RBAC security - non-optional
pub mod serialization; // JSON and binary serialization (simd-json, rkyv, bincode)
pub mod session; // Session management with event sourcing

// Proxy and gateway functionality
pub mod cache;
pub mod integrations; // External source integration (blacklists, configs, etc.)
pub mod patterns; // Pattern matching utilities (wildcards, CIDR, domains)
pub mod proxy; // Generic proxy primitives (forward, reverse, transparent)

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

// Re-export derive macros from lithair-macros so users only need one crate
#[cfg(feature = "macros")]
pub use lithair_macros::{
    lithair_api, lithair_model, DeclarativeModel, LifecycleAware, Page, RaftstoneModel, RbacRole,
    SchemaEvolution,
};

// Prelude module for convenient imports
pub mod prelude;

// Re-exports of main types and traits
pub use app::LithairServer;
pub use engine::{RaftstoneApplication, StateEngine};
pub use http::{HttpServer, Route};
pub use model_inspect::Inspectable;
pub use security::{
    AuthContext, Permission, RBACMiddleware, Role, SecurityError, SecurityEvent, SecurityState,
    User,
};

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

/// Low-level entry point for Lithair applications using the `RaftstoneApplication` trait.
///
/// For most use cases, prefer [`LithairServer`] which provides a simpler builder API.
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
/// let framework = Lithair::new(MyApp::default());
/// framework.run("127.0.0.1:8080")?;
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

// Lithair API integration - using real state engine instead of global state

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
