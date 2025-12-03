//! RBAC with Session Management Demo
//!
//! This example demonstrates:
//! - Password authentication with RBAC
//! - Session management (Cookie + Bearer token support)
//! - Login once → get session token → use for all requests
//! - Role-based permissions (Customer, Employee, Administrator)

use anyhow::Result;
use bytes::Bytes;
use chrono::Duration;
use clap::Parser;
use http::{Method, Request, Response, StatusCode};
use http_body_util::Full;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use lithair_core::session::{MemorySessionStore, Session, SessionConfig, SessionMiddleware, SessionStore, SessionManager};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::net::TcpListener;
use uuid::Uuid;

/// Product model
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Product {
    id: String,
    name: String,
    price: f64,
}

/// User roles with RBAC permissions
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Role {
    Customer,      // Can read products
    Employee,      // Can read + create products
    Administrator, // Can do everything
}

/// Login request
#[derive(Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

/// Login response
#[derive(Serialize)]
struct LoginResponse {
    session_token: String,
    role: String,
    expires_in: i64,
}

/// CLI arguments
#[derive(Parser)]
struct Args {
    #[arg(short, long, default_value = "3000")]
    port: u16,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let args = Args::parse();
    
    // Create session manager with automatic cleanup
    let session_manager = SessionManager::new(MemorySessionStore::new());
    let session_store = session_manager.store();
    
    // Create session config (hybrid: Cookie + Bearer)
    let session_config = SessionConfig::hybrid()
        .with_max_age(std::time::Duration::from_secs(3600)); // 1 hour
    
    // Create session middleware
    let session_middleware = Arc::new(SessionMiddleware::new(
        session_store.clone(),
        session_config,
    ));
    
    println!("🔐 Lithair RBAC + Sessions Demo");
    println!("=================================");
    println!();
    println!("🌐 Server: http://localhost:{}", args.port);
    println!();
    println!("📚 Endpoints:");
    println!("   POST /auth/login          - Login with username/password");
    println!("   POST /auth/logout         - Logout and destroy session");
    println!("   GET  /api/products        - List products (requires session)");
    println!("   POST /api/products        - Create product (Employee+)");
    println!("   DELETE /api/products/{{id}} - Delete product (Admin only)");
    println!();
    println!("👥 Demo Users:");
    println!("   alice/password123  → Customer");
    println!("   bob/password123    → Employee");
    println!("   admin/password123  → Administrator");
    println!();
    println!("💡 Try:");
    println!("   # 1. Login");
    println!("   curl -X POST http://localhost:{}/auth/login \\", args.port);
    println!("     -H 'Content-Type: application/json' \\");
    println!("     -d '{{\"username\":\"alice\",\"password\":\"password123\"}}'");
    println!();
    println!("   # 2. Use the session_token from response");
    println!("   curl http://localhost:{}/api/products \\", args.port);
    println!("     -H 'Authorization: Bearer <session_token>'");
    println!();
    println!("🧹 Automatic session cleanup: enabled (every 5 minutes)");
    println!();
    
    // Start server
    let addr = format!("127.0.0.1:{}", args.port);
    let listener = TcpListener::bind(&addr).await?;
    
    log::info!("🚀 Server listening on {}", addr);
    
    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
        
        let session_middleware = session_middleware.clone();
        let session_store = session_store.clone();
        
        tokio::task::spawn(async move {
            let service = service_fn(move |req| {
                handle_request(req, session_middleware.clone(), session_store.clone())
            });
            
            if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                eprintln!("Error serving connection: {:?}", err);
            }
        });
    }
}

async fn handle_request(
    req: Request<hyper::body::Incoming>,
    session_middleware: Arc<SessionMiddleware<MemorySessionStore>>,
    session_store: Arc<MemorySessionStore>,
) -> Result<Response<Full<Bytes>>> {
    let path = req.uri().path();
    let method = req.method();
    
    log::info!("{} {}", method, path);
    
    match (method, path) {
        (&Method::POST, "/auth/login") => login(req, session_middleware, session_store).await,
        (&Method::POST, "/auth/logout") => logout(req, session_middleware).await,
        (&Method::GET, "/api/products") => list_products(req, session_middleware).await,
        (&Method::POST, "/api/products") => create_product(req, session_middleware).await,
        (method, path) if path.starts_with("/api/products/") && method == Method::DELETE => {
            delete_product(req, session_middleware).await
        }
        _ => Ok(json_response(StatusCode::NOT_FOUND, serde_json::json!({
            "error": "Not found"
        }))),
    }
}

/// Login endpoint - creates a session
async fn login(
    mut req: Request<hyper::body::Incoming>,
    _session_middleware: Arc<SessionMiddleware<MemorySessionStore>>,
    session_store: Arc<MemorySessionStore>,
) -> Result<Response<Full<Bytes>>> {
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
            return Ok(json_response(StatusCode::UNAUTHORIZED, serde_json::json!({
                "error": "Invalid credentials"
            })));
        }
    };
    
    // Create session
    let session_id = Uuid::new_v4().to_string();
    let expires_at = chrono::Utc::now() + Duration::hours(1);
    
    let mut session = Session::new(session_id.clone(), expires_at);
    session.set("user_id", &login_req.username)?;
    session.set("role", format!("{:?}", role))?;
    
    // Store session (Arc<MemorySessionStore> implements SessionStore)
    session_store.set(session).await?;
    
    log::info!("✅ User logged in: {} as {:?}", login_req.username, role);
    
    // Return session token
    Ok(json_response(StatusCode::OK, serde_json::json!({
        "session_token": session_id,
        "role": format!("{:?}", role),
        "expires_in": 3600
    })))
}

/// Logout endpoint - destroys the session
async fn logout(
    req: Request<hyper::body::Incoming>,
    session_middleware: Arc<SessionMiddleware<MemorySessionStore>>,
) -> Result<Response<Full<Bytes>>> {
    // Extract session
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
    
    // Delete session
    session_middleware.store().delete(&session_id).await?;
    
    log::info!("👋 User logged out: {} (session: {})", user_id, session_id);
    
    Ok(json_response(StatusCode::OK, serde_json::json!({
        "message": "Logged out successfully"
    })))
}

/// List products - requires valid session
async fn list_products(
    req: Request<hyper::body::Incoming>,
    session_middleware: Arc<SessionMiddleware<MemorySessionStore>>,
) -> Result<Response<Full<Bytes>>> {
    // Extract session
    let session = match session_middleware.extract_session(&req).await? {
        Some(s) => s,
        None => {
            return Ok(json_response(StatusCode::UNAUTHORIZED, serde_json::json!({
                "error": "Authentication required - please login first"
            })));
        }
    };
    
    let user_id: String = session.get("user_id").unwrap_or_default();
    let role: String = session.get("role").unwrap_or_default();
    
    log::info!("📋 User {} ({}) listing products", user_id, role);
    
    // Return mock products
    let products = vec![
        Product { id: "1".to_string(), name: "Laptop".to_string(), price: 999.99 },
        Product { id: "2".to_string(), name: "Mouse".to_string(), price: 29.99 },
    ];
    
    Ok(json_response(StatusCode::OK, serde_json::json!(products)))
}

/// Create product - requires Employee or Administrator role
async fn create_product(
    mut req: Request<hyper::body::Incoming>,
    session_middleware: Arc<SessionMiddleware<MemorySessionStore>>,
) -> Result<Response<Full<Bytes>>> {
    use http_body_util::BodyExt;
    
    // Extract session
    let session = match session_middleware.extract_session(&req).await? {
        Some(s) => s,
        None => {
            return Ok(json_response(StatusCode::UNAUTHORIZED, serde_json::json!({
                "error": "Authentication required"
            })));
        }
    };
    
    let role: String = session.get("role").unwrap_or_default();
    
    // Check permissions
    if role != "Employee" && role != "Administrator" {
        return Ok(json_response(StatusCode::FORBIDDEN, serde_json::json!({
            "error": "Insufficient permissions - Employee or Administrator required"
        })));
    }
    
    // Parse product
    let body = req.body_mut().collect().await?.to_bytes();
    let mut product: Product = serde_json::from_slice(&body)?;
    product.id = Uuid::new_v4().to_string();
    
    log::info!("✅ Product created: {} (by {})", product.name, role);
    
    Ok(json_response(StatusCode::CREATED, serde_json::json!(product)))
}

/// Delete product - requires Administrator role
async fn delete_product(
    req: Request<hyper::body::Incoming>,
    session_middleware: Arc<SessionMiddleware<MemorySessionStore>>,
) -> Result<Response<Full<Bytes>>> {
    // Extract session
    let session = match session_middleware.extract_session(&req).await? {
        Some(s) => s,
        None => {
            return Ok(json_response(StatusCode::UNAUTHORIZED, serde_json::json!({
                "error": "Authentication required"
            })));
        }
    };
    
    let role: String = session.get("role").unwrap_or_default();
    
    // Check permissions
    if role != "Administrator" {
        return Ok(json_response(StatusCode::FORBIDDEN, serde_json::json!({
            "error": "Insufficient permissions - Administrator required"
        })));
    }
    
    let product_id = req.uri().path().strip_prefix("/api/products/").unwrap_or("");
    
    log::info!("🗑️  Product deleted: {} (by Administrator)", product_id);
    
    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(Full::new(Bytes::new()))
        .unwrap())
}

/// Helper to create JSON response
fn json_response(status: StatusCode, body: serde_json::Value) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body.to_string())))
        .unwrap()
}
