//! Lithair Blog - Replicated Cluster Node
//!
//! Combines the blog functionality with Raft replication:
//! - Article model with RBAC (Admin, Reporter, Contributor, Anonymous)
//! - User model for authentication
//! - Persistent sessions with event sourcing
//! - Full Raft replication across cluster nodes
//!
//! ## Running a 3-node cluster
//!
//! ```bash
//! # Use the cluster script
//! ./run_cluster.sh start
//!
//! # Or manually:
//! # Terminal 1 - Leader (node 0)
//! cargo run --bin blog_replicated_node -- --node-id 0 --port 8080 --peers 8081,8082
//!
//! # Terminal 2 - Follower 1
//! cargo run --bin blog_replicated_node -- --node-id 1 --port 8081 --peers 8080,8082
//!
//! # Terminal 3 - Follower 2
//! cargo run --bin blog_replicated_node -- --node-id 2 --port 8082 --peers 8080,8081
//! ```
//!
//! ## Test replication
//!
//! ```bash
//! # Login as admin on leader
//! curl -X POST http://localhost:8080/auth/login \
//!   -H "Content-Type: application/json" \
//!   -d '{"username":"admin","password":"password123"}'
//!
//! # Create article on leader (use session token from login)
//! curl -X POST http://localhost:8080/api/articles \
//!   -H "Content-Type: application/json" \
//!   -H "Authorization: Bearer <session_token>" \
//!   -d '{"title":"Hello World","content":"First replicated article!","author_id":"admin"}'
//!
//! # Read from any follower (should be replicated)
//! curl http://localhost:8081/api/articles
//! curl http://localhost:8082/api/articles
//!
//! # Check cluster health
//! curl http://localhost:8080/_raft/health | jq
//! ```

use anyhow::Result;
use bytes::Bytes;
use chrono::{DateTime, Duration, Utc};
use clap::Parser;
use http::{Method, Request, Response, StatusCode};
use http_body_util::Full;
use lithair_core::app::LithairServer;
use lithair_core::cluster::ClusterArgs;
use lithair_core::frontend::{FrontendEngine, FrontendServer};
use lithair_core::session::{PersistentSessionStore, Session, SessionConfig, SessionMiddleware, SessionStore};
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

// ============================================================================
// MODELS
// ============================================================================

/// Article status workflow
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ArticleStatus {
    #[default]
    Draft,
    UnderReview,
    Published,
}

/// Article model - blog posts with RBAC
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
pub struct Article {
    #[db(primary_key, indexed)]
    #[lifecycle(immutable)]
    #[http(expose)]
    #[persistence(replicate, track_history)]
    #[permission(read = "ArticleRead")]
    #[serde(default = "generate_uuid")]
    pub id: Uuid,

    #[db(indexed)]
    #[http(expose, validate = "non_empty")]
    #[persistence(replicate, track_history)]
    #[permission(read = "ArticleRead", write = "ArticleWrite")]
    pub title: String,

    #[http(expose)]
    #[persistence(replicate, track_history)]
    #[permission(read = "ArticleRead", write = "ArticleWrite")]
    pub content: String,

    #[db(indexed)]
    #[http(expose)]
    #[persistence(replicate, track_history)]
    #[permission(read = "ArticleRead", write = "ArticleWrite")]
    pub author_id: String,

    #[http(expose)]
    #[persistence(replicate, track_history)]
    #[permission(read = "ArticleRead", write = "ArticleWrite")]
    #[serde(default)]
    pub status: ArticleStatus,

    #[lifecycle(immutable)]
    #[http(expose)]
    #[persistence(track_history)]
    #[serde(default = "chrono::Utc::now")]
    pub created_at: DateTime<Utc>,

    #[http(expose)]
    #[persistence(replicate, track_history)]
    #[serde(default = "chrono::Utc::now")]
    pub updated_at: DateTime<Utc>,
}

/// User model - for authentication (replicated across cluster)
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
pub struct User {
    #[db(primary_key, indexed)]
    #[lifecycle(immutable)]
    #[http(expose)]
    #[persistence(replicate, track_history)]
    #[serde(default = "generate_uuid")]
    pub id: Uuid,

    #[db(indexed, unique)]
    #[http(expose, validate = "non_empty")]
    #[persistence(replicate, track_history)]
    pub username: String,

    #[http(expose)]
    #[persistence(replicate)]
    pub role: String,

    #[lifecycle(immutable)]
    #[http(expose)]
    #[persistence(track_history)]
    #[serde(default = "chrono::Utc::now")]
    pub created_at: DateTime<Utc>,
}

fn generate_uuid() -> Uuid {
    Uuid::new_v4()
}

// ============================================================================
// RBAC
// ============================================================================

/// Permission checker implementing role-based permissions for blog
struct BlogPermissionChecker;

impl lithair_core::rbac::PermissionChecker for BlogPermissionChecker {
    fn has_permission(&self, role: &str, permission: &str) -> bool {
        match (role, permission) {
            // Anonymous: read published articles only
            ("Anonymous", "ArticleRead") => true,

            // Contributor: read + create + update own
            ("Contributor", "ArticleRead") => true,
            ("Contributor", "ArticleWrite") => true,

            // Reporter: read + create + update + publish
            ("Reporter", "ArticleRead") => true,
            ("Reporter", "ArticleWrite") => true,
            ("Reporter", "ArticlePublish") => true,

            // Admin: all permissions
            ("Admin", "ArticleRead") => true,
            ("Admin", "ArticleWrite") => true,
            ("Admin", "ArticlePublish") => true,
            ("Admin", "ArticleDelete") => true,
            ("Admin", "UserRead") => true,
            ("Admin", "UserWrite") => true,

            _ => false,
        }
    }
}

// ============================================================================
// AUTH HANDLERS
// ============================================================================

#[derive(Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

/// Login endpoint - creates a session
async fn login(
    mut req: Request<hyper::body::Incoming>,
    session_store: Arc<PersistentSessionStore>,
) -> Result<Response<Full<Bytes>>> {
    use http_body_util::BodyExt;

    let body = req.body_mut().collect().await?.to_bytes();
    let login_req: LoginRequest = serde_json::from_slice(&body)?;

    // Demo users (in production, check against User model with hashed passwords)
    let role = match (login_req.username.as_str(), login_req.password.as_str()) {
        ("admin", "password123") => "Admin",
        ("reporter", "password123") => "Reporter",
        ("contributor", "password123") => "Contributor",
        _ => {
            return Ok(json_response(StatusCode::UNAUTHORIZED, serde_json::json!({
                "error": "Invalid credentials"
            })));
        }
    };

    // Create session
    let session_id = Uuid::new_v4().to_string();
    let expires_at = Utc::now() + Duration::hours(1);

    let mut session = Session::new(session_id.clone(), expires_at);
    session.set("user_id", &login_req.username)?;
    session.set("role", role)?;

    session_store.set(session).await?;

    log::info!("User logged in: {} as {}", login_req.username, role);

    Ok(json_response(StatusCode::OK, serde_json::json!({
        "session_token": session_id,
        "role": role,
        "expires_in": 3600
    })))
}

/// Logout endpoint - destroys the session
async fn logout(
    req: Request<hyper::body::Incoming>,
    session_middleware: Arc<SessionMiddleware<PersistentSessionStore>>,
) -> Result<Response<Full<Bytes>>> {
    let session = match session_middleware.extract_session(&req).await? {
        Some(s) => s,
        None => {
            return Ok(json_response(StatusCode::UNAUTHORIZED, serde_json::json!({
                "error": "No active session"
            })));
        }
    };

    let user_id: String = session.get("user_id").unwrap_or_default();
    let session_id = session.id.clone();

    session_middleware.store().delete(&session_id).await?;

    log::info!("User logged out: {} (session: {})", user_id, session_id);

    Ok(json_response(StatusCode::OK, serde_json::json!({
        "message": "Logged out successfully"
    })))
}

fn json_response(status: StatusCode, body: serde_json::Value) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(body.to_string())))
        .unwrap()
}

// ============================================================================
// MAIN
// ============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    let args = ClusterArgs::parse();
    let peer_ports = args.peers.clone().unwrap_or_default();
    let peers: Vec<String> = peer_ports
        .iter()
        .map(|p| format!("127.0.0.1:{}", p))
        .collect();

    // Data directories
    let base_dir = std::env::var("EXPERIMENT_DATA_BASE").unwrap_or_else(|_| "data".to_string());
    let data_dir = format!("{}/blog_node_{}", base_dir, args.node_id);
    std::fs::create_dir_all(&data_dir)?;

    let articles_path = format!("{}/articles_events", data_dir);
    let users_path = format!("{}/users_events", data_dir);
    let sessions_path = format!("{}/sessions", data_dir);

    // Session store
    let session_store = Arc::new(PersistentSessionStore::new(PathBuf::from(&sessions_path))?);
    let session_config = SessionConfig::hybrid()
        .with_max_age(std::time::Duration::from_secs(3600));
    let session_middleware = Arc::new(SessionMiddleware::new(
        session_store.clone(),
        session_config,
    ));

    // Clones for handlers
    let ss_login = session_store.clone();
    let sm_logout = session_middleware.clone();

    // Permission checker
    let permission_checker = Arc::new(BlogPermissionChecker);

    // Frontend engine (SCC2-based, memory-first)
    let frontend_engine = Arc::new(
        FrontendEngine::new("blog_replicated", &format!("{}/frontend", data_dir)).await
            .expect("Failed to create frontend engine")
    );

    // Load frontend assets - try multiple paths
    let frontend_paths = [
        "frontend",                                    // Same directory as binary
        "examples/blog_replicated_demo/frontend",     // From workspace root
        "../blog_replicated_demo/frontend",           // Relative from examples
    ];

    let mut loaded = false;
    for path in &frontend_paths {
        if std::path::Path::new(path).exists() {
            match frontend_engine.load_directory(path).await {
                Ok(count) => {
                    log::info!("Loaded {} frontend assets from {}", count, path);
                    loaded = true;
                    break;
                }
                Err(e) => log::debug!("Could not load from {}: {}", path, e),
            }
        }
    }
    if !loaded {
        log::warn!("Could not load frontend assets from any path");
    }

    let frontend_server = Arc::new(FrontendServer::new_scc2(frontend_engine.clone()));

    log::info!("═══════════════════════════════════════════════════════════");
    log::info!("  LITHAIR BLOG - Replicated Cluster Node");
    log::info!("═══════════════════════════════════════════════════════════");
    log::info!("  Node ID:    {}", args.node_id);
    log::info!("  Port:       {}", args.port);
    log::info!("  Peers:      {:?}", peers);
    log::info!("  Data dir:   {}", data_dir);
    log::info!("═══════════════════════════════════════════════════════════");
    log::info!("  Endpoints:");
    log::info!("    POST   /auth/login        - Login");
    log::info!("    POST   /auth/logout       - Logout");
    log::info!("    GET    /api/articles      - List articles");
    log::info!("    POST   /api/articles      - Create article (replicated)");
    log::info!("    GET    /api/articles/:id  - Get article");
    log::info!("    PUT    /api/articles/:id  - Update article (replicated)");
    log::info!("    DELETE /api/articles/:id  - Delete article (replicated)");
    log::info!("    GET    /_raft/health      - Cluster health");
    log::info!("═══════════════════════════════════════════════════════════");
    log::info!("  Demo Users:");
    log::info!("    admin/password123       -> Admin");
    log::info!("    reporter/password123    -> Reporter");
    log::info!("    contributor/password123 -> Contributor");
    log::info!("═══════════════════════════════════════════════════════════");

    let mut server = LithairServer::new()
        .with_port(args.port)
        // Articles with RBAC
        .with_model_full::<Article>(
            articles_path,
            "/api/articles",
            Some(permission_checker.clone()),
            Some(session_store.clone() as Arc<dyn std::any::Any + Send + Sync>),
        )
        // Users (admin only)
        .with_model::<User>(&users_path, "/api/users")
        // Admin dashboard to visualize data
        .with_admin_panel(true)
        .with_data_admin()
        .with_data_admin_ui("/_admin")
        // Auth routes
        .with_route(Method::POST, "/auth/login".to_string(), move |req| {
            let ss = ss_login.clone();
            Box::pin(async move { login(req, ss).await })
        })
        .with_route(Method::POST, "/auth/logout".to_string(), move |req| {
            let sm = sm_logout.clone();
            Box::pin(async move { logout(req, sm).await })
        })
        // Frontend: serve static assets from SCC2 memory
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
        });

    // Enable Raft cluster if peers are provided
    if !peers.is_empty() {
        server = server.with_raft_cluster(args.node_id, peers);
        log::info!("Cluster mode enabled with {} peers", peer_ports.len());
    } else {
        log::info!("Single-node mode (no peers)");
    }

    server.build()?.serve().await
}
