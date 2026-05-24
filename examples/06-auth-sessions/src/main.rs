//! RBAC with Session Management Demo - V2 (LithairServer)
//!
//! This is the refactored version using LithairServer instead of manual Hyper.
//!
//! **BEFORE**: 340 lines of manual Hyper code
//! **AFTER**: ~150 lines with LithairServer + persistent sessions
//!
//! This example demonstrates:
//! - Password authentication with RBAC
//! - Session management (Cookie + Bearer token support)
//! - **Persistent sessions** stored with event sourcing
//! - **DeclarativeModel for automatic CRUD** (products)
//! - Login once → get session token → use for all requests
//! - Role-based permissions (Customer, Employee, Administrator)

use anyhow::Result;
use bytes::Bytes;
use chrono::Duration;
use clap::Parser;
use http::{Method, Request, Response, StatusCode};
use http_body_util::{BodyExt, Full};
use lithair_core::app::LithairServer;
use lithair_core::frontend::{FrontendEngine, FrontendServer};
use lithair_core::session::{
    PersistentSessionStore, Session, SessionConfig, SessionManager, SessionMiddleware, SessionStore,
};
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

/// Product model with automatic CRUD via DeclarativeModel
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
struct Product {
    #[http(expose)]
    #[permission(read = "ProductRead", write = "ProductWrite")]
    id: String,

    #[http(expose)]
    #[permission(read = "ProductRead", write = "ProductWrite")]
    name: String,

    #[http(expose)]
    #[permission(read = "ProductRead", write = "ProductWrite")]
    price: f64,
}

/// User roles with RBAC permissions
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Role {
    Customer,      // Can read products
    Employee,      // Can read + create products
    Administrator, // Can do everything
}

/// Permission checker implementing role-based permissions
struct RolePermissionChecker;

impl lithair_core::rbac::PermissionChecker for RolePermissionChecker {
    fn has_permission(&self, role: &str, permission: &str) -> bool {
        match (role, permission) {
            // Customer: read only
            ("Customer", "ProductRead") => true,

            // Employee: read + write (no delete)
            ("Employee", "ProductRead") => true,
            ("Employee", "ProductWrite") => true,

            // Administrator: all permissions
            ("Administrator", "ProductRead") => true,
            ("Administrator", "ProductWrite") => true,
            ("Administrator", "ProductDelete") => true,

            // Deny everything else
            _ => false,
        }
    }
}

/// Login request
#[derive(Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

/// CLI arguments
#[derive(Parser)]
struct Args {
    #[arg(short, long, default_value = "3000")]
    port: u16,

    #[arg(long, default_value = "./data/sessions")]
    sessions_dir: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Create Lithair event-sourced session store
    let session_store = Arc::new(PersistentSessionStore::new(args.sessions_dir.clone())?);
    // `from_arc` (not `new`) — we already have an `Arc<PersistentSessionStore>`
    // because the login/logout handlers below need to share it. Calling
    // `SessionManager::new(session_store.clone())` here would produce a
    // double-`Arc` shape that the require-session gate cannot recognize
    // (issue #80).
    let _session_manager = SessionManager::from_arc(session_store.clone());

    // Create session middleware
    let session_config = SessionConfig::hybrid().with_max_age(std::time::Duration::from_secs(3600));
    let session_middleware =
        Arc::new(SessionMiddleware::new(session_store.clone(), session_config));

    // Clone for handlers
    let sm_login = session_middleware.clone();
    let ss_login = session_store.clone();
    let sm_logout = session_middleware.clone();

    println!("🔐 Lithair RBAC + Sessions Demo (V2 - Declarative)");
    println!("=====================================================");
    println!("💾 Sessions: Event Sourcing ({})", args.sessions_dir.display());
    println!("📦 Products: DeclarativeModel with automatic CRUD");
    println!();
    println!("🌐 Server: http://localhost:{}", args.port);
    println!();
    println!("📚 Endpoints:");
    println!("   POST /auth/login           - Login with username/password");
    println!("   POST /auth/logout          - Logout and destroy session");
    println!("   GET  /api/products         - List products (auto-generated)");
    println!("   POST /api/products         - Create product (auto-generated)");
    println!("   GET  /api/products/:id     - Get product by ID (auto-generated)");
    println!("   PUT  /api/products/:id     - Update product (auto-generated)");
    println!("   DELETE /api/products/:id   - Delete product (auto-generated)");
    println!();
    println!("👥 Demo Users:");
    println!("   alice/password123  → Customer");
    println!("   bob/password123    → Employee");
    println!("   admin/password123  → Administrator");
    println!();
    println!("🔐 RBAC Roles & Permissions:");
    println!("   Customer       → ProductRead (GET only)");
    println!("   Employee       → ProductRead + ProductWrite (GET, POST, PUT)");
    println!("   Administrator  → ProductRead + ProductWrite + ProductDelete (all operations)");
    println!();
    println!("✅ RBAC enforcement: ENABLED");
    println!("   Permissions are checked on every CREATE, UPDATE, DELETE operation");
    println!();

    // Create permission checker
    let permission_checker = Arc::new(RolePermissionChecker);

    // ✨ Lithair Frontend (SCC2): Load static assets with event sourcing
    let frontend_engine = Arc::new(
        FrontendEngine::new("rbac_demo", "./data")
            .await
            .expect("Failed to create frontend engine"),
    );

    match frontend_engine.load_directory("examples/06-auth-sessions/frontend").await {
        Ok(count) => println!(
            "✅ Loaded {} frontend assets into SCC2 memory (lock-free + event sourcing)\n",
            count
        ),
        Err(e) => println!("⚠️  Warning: Could not load frontend assets: {}\n", e),
    }

    // Create frontend server (SCC2-based, 40M+ ops/sec)
    let frontend_server = Arc::new(FrontendServer::new_scc2(frontend_engine.clone()));

    // Start server with LithairServer + DeclarativeModel
    LithairServer::new()
        .with_port(args.port)
        .with_host("127.0.0.1")

        // 🚀 Automatic CRUD for Product via DeclarativeModel with RBAC
        .with_model_full::<Product>(
            "./data/products",
            "/api/products",
            Some(permission_checker),
            Some(session_store.clone() as Arc<dyn std::any::Any + Send + Sync>),
        )

        // Session routes (custom logic)
        .with_route(Method::POST, "/auth/login".to_string(), move |req| {
            let ss = ss_login.clone();
            let sm = sm_login.clone();
            Box::pin(async move { login(req, sm, ss).await })
        })
        .with_route(Method::POST, "/auth/logout".to_string(), move |req| {
            let sm = sm_logout.clone();
            Box::pin(async move { logout(req, sm).await })
        })

        // Admin panel
        .with_admin_panel(true)

        // 🎨 Lithair Frontend: Serve all static assets from SCC2 memory
        .with_route(Method::GET, "/*".to_string(), move |req| {
            let server = frontend_server.clone();
            Box::pin(async move {
                use http_body_util::BodyExt;
                let response = server.handle_request(req).await
                    .map_err(|e| anyhow::anyhow!("{:?}", e))?;

                // Convert BoxBody to Full<Bytes>
                let (parts, body) = response.into_parts();
                let bytes = body.collect().await
                    .map_err(|_| anyhow::anyhow!("Failed to collect body"))?
                    .to_bytes();

                Ok(Response::from_parts(parts, Full::new(bytes).boxed()))
            })
        })

        .serve()
        .await?;

    Ok(())
}

/// Login endpoint - creates a session
async fn login(
    mut req: Request<hyper::body::Incoming>,
    _session_middleware: Arc<SessionMiddleware<PersistentSessionStore>>,
    session_store: Arc<PersistentSessionStore>,
) -> Result<Response<http_body_util::combinators::BoxBody<Bytes, std::convert::Infallible>>> {
    use http_body_util::BodyExt;

    // Parse request body
    let body = req.body_mut().collect().await?.to_bytes();
    let login_req: LoginRequest = serde_json::from_slice(&body)?;

    // Authenticate (simple demo - in production use proper password hashing)
    let role = match (login_req.username.as_str(), login_req.password.as_str()) {
        ("alice", "password123") => Role::Customer,
        ("bob", "password123") => Role::Employee,
        ("admin", "password123") => Role::Administrator,
        _ => {
            return Ok(json_response(
                StatusCode::UNAUTHORIZED,
                serde_json::json!({
                    "error": "Invalid credentials"
                }),
            ));
        }
    };

    // Create session
    let session_id = Uuid::new_v4().to_string();
    let expires_at = chrono::Utc::now() + Duration::hours(1);

    let mut session = Session::new(session_id.clone(), expires_at);
    session.set("user_id", &login_req.username)?;
    session.set("role", format!("{:?}", role))?;

    // Store session
    session_store.set(session).await?;

    log::info!("✅ User logged in: {} as {:?}", login_req.username, role);

    // Return session token
    Ok(json_response(
        StatusCode::OK,
        serde_json::json!({
            "session_token": session_id,
            "role": format!("{:?}", role),
            "expires_in": 3600
        }),
    ))
}

/// Logout endpoint - destroys the session
async fn logout(
    req: Request<hyper::body::Incoming>,
    session_middleware: Arc<SessionMiddleware<PersistentSessionStore>>,
) -> Result<Response<http_body_util::combinators::BoxBody<Bytes, std::convert::Infallible>>> {
    // Extract session
    let session = match session_middleware.extract_session(&req).await? {
        Some(s) => s,
        None => {
            return Ok(json_response(
                StatusCode::UNAUTHORIZED,
                serde_json::json!({
                    "error": "No active session"
                }),
            ));
        }
    };

    let user_id: String = session.get("user_id").unwrap_or_default();
    let session_id = session.id.clone();

    // Delete session
    let store = session_middleware.store();
    store.delete(&session_id).await?;

    log::info!("👋 User logged out: {} (session: {})", user_id, session_id);

    Ok(json_response(
        StatusCode::OK,
        serde_json::json!({
            "message": "Logged out successfully"
        }),
    ))
}

/// Helper to create JSON response
fn json_response(
    status: StatusCode,
    body: serde_json::Value,
) -> Response<http_body_util::combinators::BoxBody<Bytes, std::convert::Infallible>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(body.to_string())).boxed())
        .unwrap()
}
