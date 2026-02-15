//! Lithair Blog - Fully Declarative Example
//!
//! This example demonstrates:
//! - DeclarativeModel for automatic CRUD on Article entity
//! - LithairServer with unified routing
//! - Persistent sessions with SessionMiddleware
//! - RBAC with roles: Admin, Reporter, Contributor, Anonymous
//! - Frontend assets served from SCC2 memory (FrontendEngine)
//! - Single binary, single port (3000)

use anyhow::Result;
use bytes::Bytes;
use chrono::Duration;
use clap::Parser;
use http::{Method, Request, Response, StatusCode};
use http_body_util::Full;
use lithair_core::app::LithairServer;
use lithair_core::frontend::{FrontendEngine, FrontendServer};
use lithair_core::session::{
    PersistentSessionStore, Session, SessionConfig, SessionMiddleware, SessionStore,
};
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

/// Article status workflow
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum ArticleStatus {
    Draft,
    UnderReview,
    Published,
}

/// Article model with automatic CRUD via DeclarativeModel
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
struct Article {
    #[http(expose)]
    #[permission(read = "ArticleRead", write = "ArticleWrite")]
    id: String,

    #[http(expose)]
    #[permission(read = "ArticleRead", write = "ArticleWrite")]
    title: String,

    #[http(expose)]
    #[permission(read = "ArticleRead", write = "ArticleWrite")]
    content: String,

    #[http(expose)]
    #[permission(read = "ArticleRead", write = "ArticleWrite")]
    author_id: String,

    #[http(expose)]
    #[permission(read = "ArticleRead", write = "ArticleWrite")]
    status: ArticleStatus,
}

/// Blog user roles with RBAC permissions
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Role {
    #[allow(dead_code)]
    Anonymous, // Can read published articles only
    Contributor, // Can create + update own articles
    Reporter,    // Can publish articles
    Admin,       // Full access
}

/// Permission checker implementing role-based permissions for blog
struct BlogPermissionChecker;

impl lithair_core::rbac::PermissionChecker for BlogPermissionChecker {
    fn has_permission(&self, role: &str, permission: &str) -> bool {
        match (role, permission) {
            // Anonymous: read published articles only
            ("Anonymous", "ArticleRead") => true,

            // Contributor: read + create + update own
            ("Contributor", "ArticleRead") => true,
            ("Contributor", "ArticleWrite") => true, // Will check ownership

            // Reporter: read + create + update + publish
            ("Reporter", "ArticleRead") => true,
            ("Reporter", "ArticleWrite") => true,
            ("Reporter", "ArticlePublish") => true,

            // Admin: all permissions
            ("Admin", "ArticleRead") => true,
            ("Admin", "ArticleWrite") => true,
            ("Admin", "ArticlePublish") => true,
            ("Admin", "ArticleDelete") => true,

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

    #[arg(long, default_value = "./data/blog/sessions")]
    sessions_dir: PathBuf,

    #[arg(long, default_value = "./data/blog/articles")]
    articles_dir: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Create Lithair event-sourced session store
    let session_store = Arc::new(PersistentSessionStore::new(args.sessions_dir.clone())?);

    // Create session middleware
    let session_config = SessionConfig::hybrid().with_max_age(std::time::Duration::from_secs(3600));
    let session_middleware =
        Arc::new(SessionMiddleware::new(session_store.clone(), session_config));

    // Clone for handlers
    let sm_login = session_middleware.clone();
    let ss_login = session_store.clone();
    let sm_logout = session_middleware.clone();

    println!("📰 Lithair Blog - Fully Declarative");
    println!("======================================");
    println!("💾 Sessions: Event Sourcing ({})", args.sessions_dir.display());
    println!("📦 Articles: DeclarativeModel with automatic CRUD");
    println!();
    println!("🌐 Server: http://localhost:{}", args.port);
    println!();
    println!("📚 Endpoints:");
    println!("   POST   /auth/login           - Login with username/password");
    println!("   POST   /auth/logout          - Logout and destroy session");
    println!("   GET    /api/articles         - List articles (auto-generated)");
    println!("   POST   /api/articles         - Create article (auto-generated)");
    println!("   GET    /api/articles/:id     - Get article by ID (auto-generated)");
    println!("   PUT    /api/articles/:id     - Update article (auto-generated)");
    println!("   DELETE /api/articles/:id     - Delete article (auto-generated)");
    println!();
    println!("👥 Demo Users:");
    println!("   admin/password123       → Admin");
    println!("   reporter/password123    → Reporter");
    println!("   contributor/password123 → Contributor");
    println!();
    println!("🔐 RBAC Roles & Permissions:");
    println!("   Anonymous    → ArticleRead (published only)");
    println!("   Contributor  → ArticleRead + ArticleWrite (own articles)");
    println!("   Reporter     → ArticleRead + ArticleWrite + ArticlePublish");
    println!("   Admin        → All permissions including ArticleDelete");
    println!();
    println!("✅ RBAC enforcement: ENABLED");
    println!("   Permissions are checked on every CREATE, UPDATE, DELETE operation");
    println!();

    // Create permission checker
    let permission_checker = Arc::new(BlogPermissionChecker);

    // ✨ Lithair Frontend (SCC2): Load static assets with event sourcing
    let frontend_engine = Arc::new(
        FrontendEngine::new("blog_demo", "./data/blog/frontend")
            .await
            .expect("Failed to create frontend engine"),
    );

    match frontend_engine.load_directory("examples/04-blog/frontend").await {
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

        // 🚀 Automatic CRUD for Article via DeclarativeModel with RBAC
        .with_model_full::<Article>(
            args.articles_dir.to_string_lossy().to_string(),
            "/api/articles",
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

                Ok(Response::from_parts(parts, Full::new(bytes)))
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
) -> Result<Response<Full<Bytes>>> {
    use http_body_util::BodyExt;

    // Parse request body
    let body = req.body_mut().collect().await?.to_bytes();
    let login_req: LoginRequest = serde_json::from_slice(&body)?;

    // Authenticate (simple demo - in production use proper password hashing)
    let role = match (login_req.username.as_str(), login_req.password.as_str()) {
        ("admin", "password123") => Role::Admin,
        ("reporter", "password123") => Role::Reporter,
        ("contributor", "password123") => Role::Contributor,
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
) -> Result<Response<Full<Bytes>>> {
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
fn json_response(status: StatusCode, body: serde_json::Value) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(body.to_string())))
        .unwrap()
}
