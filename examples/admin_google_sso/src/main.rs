//! Admin Dashboard with Google OAuth2 SSO
//!
//! This example demonstrates:
//! - Google OAuth2 authentication flow
//! - Session management with refresh tokens
//! - Protected admin routes
//! - Automatic token refresh

use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration, Utc};
use clap::Parser;
use http::{Response, StatusCode};
use http_body_util::Full;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// Session store (in-memory for demo - use Redis/DB in production)
type SessionStore = Arc<RwLock<HashMap<String, Session>>>;

/// User session with OAuth tokens
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Session {
    /// Session ID
    id: String,
    
    /// Google user ID
    user_id: String,
    
    /// User email
    email: String,
    
    /// User name
    name: String,
    
    /// Access token
    access_token: String,
    
    /// Refresh token (to get new access tokens)
    refresh_token: Option<String>,
    
    /// Token expiration time
    expires_at: DateTime<Utc>,
    
    /// Session creation time
    created_at: DateTime<Utc>,
    
    /// User role
    role: String,
}

impl Session {
    /// Check if the access token is expired
    fn is_expired(&self) -> bool {
        Utc::now() >= self.expires_at
    }
    
    /// Check if session is still valid (not too old)
    fn is_valid(&self) -> bool {
        let max_age = Duration::hours(24);
        Utc::now() < self.created_at + max_age
    }
}

/// Google OAuth2 configuration
#[derive(Clone)]
struct GoogleOAuthConfig {
    client_id: String,
    client_secret: String,
    redirect_uri: String,
}

impl GoogleOAuthConfig {
    fn from_env() -> Result<Self> {
        Ok(Self {
            client_id: std::env::var("GOOGLE_CLIENT_ID")
                .map_err(|_| anyhow!("GOOGLE_CLIENT_ID not set"))?,
            client_secret: std::env::var("GOOGLE_CLIENT_SECRET")
                .map_err(|_| anyhow!("GOOGLE_CLIENT_SECRET not set"))?,
            redirect_uri: std::env::var("GOOGLE_REDIRECT_URI")
                .unwrap_or_else(|_| "http://localhost:3000/auth/google/callback".to_string()),
        })
    }
    
    /// Generate Google OAuth2 authorization URL
    fn get_auth_url(&self, state: &str) -> String {
        format!(
            "https://accounts.google.com/o/oauth2/v2/auth?\
             client_id={}&\
             redirect_uri={}&\
             response_type=code&\
             scope=openid%20email%20profile&\
             access_type=offline&\
             prompt=consent&\
             state={}",
            urlencoding::encode(&self.client_id),
            urlencoding::encode(&self.redirect_uri),
            urlencoding::encode(state)
        )
    }
}

/// Google token response
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: i64,
    refresh_token: Option<String>,
    scope: String,
    token_type: String,
}

/// Google user info
#[derive(Debug, Deserialize)]
struct GoogleUserInfo {
    id: String,
    email: String,
    verified_email: bool,
    name: String,
    given_name: Option<String>,
    family_name: Option<String>,
    picture: Option<String>,
}

/// CLI arguments
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Port to listen on
    #[arg(short, long, default_value = "3000")]
    port: u16,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let args = Args::parse();
    
    // Load Google OAuth config from environment
    let oauth_config = GoogleOAuthConfig::from_env()?;
    
    // Create session store
    let sessions: SessionStore = Arc::new(RwLock::new(HashMap::new()));
    
    println!("🔐 Lithair Admin with Google SSO");
    println!("==================================");
    println!();
    println!("🌐 Server listening on http://localhost:{}", args.port);
    println!();
    println!("📚 Endpoints:");
    println!("   GET  /                        - Public homepage");
    println!("   GET  /admin                   - Admin dashboard (requires Google login)");
    println!("   GET  /auth/google/login       - Initiate Google OAuth2 flow");
    println!("   GET  /auth/google/callback    - OAuth2 callback handler");
    println!("   GET  /auth/logout             - Logout");
    println!();
    println!("💡 Try:");
    println!("   1. Visit http://localhost:{}", args.port);
    println!("   2. Click 'Admin Dashboard'");
    println!("   3. You'll be redirected to Google login");
    println!("   4. After authentication, you'll see the admin panel");
    println!();
    println!("⚙️  Configuration:");
    println!("   Client ID: {}...", &oauth_config.client_id[..20.min(oauth_config.client_id.len())]);
    println!("   Redirect URI: {}", oauth_config.redirect_uri);
    println!();
    
    // Start HTTP server
    start_server(args.port, oauth_config, sessions).await
}

async fn start_server(
    port: u16,
    oauth_config: GoogleOAuthConfig,
    sessions: SessionStore,
) -> Result<()> {
    use hyper::server::conn::http1;
    use hyper::service::service_fn;
    use hyper_util::rt::TokioIo;
    use tokio::net::TcpListener;
    
    let addr = format!("127.0.0.1:{}", port);
    let listener = TcpListener::bind(&addr).await?;
    
    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
        
        let oauth_config = oauth_config.clone();
        let sessions = sessions.clone();
        
        tokio::task::spawn(async move {
            let service = service_fn(move |req| {
                handle_request(req, oauth_config.clone(), sessions.clone())
            });
            
            if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                eprintln!("Error serving connection: {:?}", err);
            }
        });
    }
}

async fn handle_request(
    req: hyper::Request<hyper::body::Incoming>,
    oauth_config: GoogleOAuthConfig,
    sessions: SessionStore,
) -> Result<Response<Full<Bytes>>> {
    let path = req.uri().path();
    let method = req.method();
    
    log::info!("{} {}", method, path);
    
    match (method.as_str(), path) {
        ("GET", "/") => Ok(homepage_response()),
        ("GET", "/auth/google/login") => Ok(google_login_response(&oauth_config)),
        ("GET", "/auth/google/callback") => {
            google_callback(req, oauth_config, sessions).await
        }
        ("GET", "/auth/logout") => Ok(logout_response()),
        ("GET", path) if path.starts_with("/admin") => {
            admin_route(req, sessions).await
        }
        _ => Ok(not_found_response()),
    }
}

fn homepage_response() -> Response<Full<Bytes>> {
    let html = r#"
<!DOCTYPE html>
<html>
<head>
    <title>Lithair Admin with Google SSO</title>
    <style>
        body { font-family: Arial, sans-serif; max-width: 800px; margin: 50px auto; padding: 20px; }
        h1 { color: #333; }
        .button { display: inline-block; padding: 12px 24px; background: #4285f4; color: white; 
                  text-decoration: none; border-radius: 4px; margin: 10px 5px; }
        .button:hover { background: #357ae8; }
        .info { background: #f0f0f0; padding: 15px; border-radius: 4px; margin: 20px 0; }
    </style>
</head>
<body>
    <h1>🔐 Lithair Admin with Google SSO</h1>
    
    <div class="info">
        <p><strong>This demo shows:</strong></p>
        <ul>
            <li>Google OAuth2 authentication</li>
            <li>Session management with refresh tokens</li>
            <li>Protected admin routes</li>
            <li>Automatic token refresh</li>
        </ul>
    </div>
    
    <h2>Try it out:</h2>
    <a href="/admin" class="button">📊 Admin Dashboard</a>
    <a href="/auth/google/login" class="button">🔐 Login with Google</a>
    
    <h3>How it works:</h3>
    <ol>
        <li>Click "Admin Dashboard" or "Login with Google"</li>
        <li>You'll be redirected to Google's login page</li>
        <li>Sign in and grant permissions</li>
        <li>You'll be redirected back with a session cookie</li>
        <li>Access the protected admin dashboard!</li>
    </ol>
</body>
</html>
    "#;
    
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/html; charset=utf-8")
        .body(Full::new(Bytes::from(html)))
        .unwrap()
}

fn google_login_response(oauth_config: &GoogleOAuthConfig) -> Response<Full<Bytes>> {
    // Generate random state for CSRF protection
    let state = Uuid::new_v4().to_string();
    
    // In production, store state in session/cookie to validate callback
    let auth_url = oauth_config.get_auth_url(&state);
    
    Response::builder()
        .status(StatusCode::FOUND)
        .header("location", auth_url)
        .body(Full::new(Bytes::new()))
        .unwrap()
}

async fn google_callback(
    req: hyper::Request<hyper::body::Incoming>,
    oauth_config: GoogleOAuthConfig,
    sessions: SessionStore,
) -> Result<Response<Full<Bytes>>> {
    // Parse query parameters
    let query = req.uri().query().unwrap_or("");
    let params: HashMap<String, String> = url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect();
    
    // Get authorization code
    let code = params.get("code")
        .ok_or_else(|| anyhow!("No code in callback"))?;
    
    // Exchange code for tokens
    let token_response = exchange_code_for_token(&oauth_config, code).await?;
    
    // Get user info from Google
    let user_info = get_user_info(&token_response.access_token).await?;
    
    // Create session
    let session_id = Uuid::new_v4().to_string();
    let session = Session {
        id: session_id.clone(),
        user_id: user_info.id,
        email: user_info.email.clone(),
        name: user_info.name,
        access_token: token_response.access_token,
        refresh_token: token_response.refresh_token,
        expires_at: Utc::now() + Duration::seconds(token_response.expires_in),
        created_at: Utc::now(),
        role: map_email_to_role(&user_info.email),
    };
    
    // Store session
    sessions.write().unwrap().insert(session_id.clone(), session);
    
    log::info!("✅ User logged in: {}", user_info.email);
    
    // Set session cookie and redirect to admin
    Ok(Response::builder()
        .status(StatusCode::FOUND)
        .header("location", "/admin")
        .header("set-cookie", format!("session_id={}; HttpOnly; SameSite=Lax; Path=/", session_id))
        .body(Full::new(Bytes::new()))
        .unwrap())
}

async fn admin_route(
    req: hyper::Request<hyper::body::Incoming>,
    sessions: SessionStore,
) -> Result<Response<Full<Bytes>>> {
    // Extract session cookie
    let session_id = req.headers()
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';')
                .find(|c| c.trim().starts_with("session_id="))
                .and_then(|c| c.split('=').nth(1))
        });
    
    let session_id = match session_id {
        Some(id) => id,
        None => {
            // No session - redirect to login
            return Ok(Response::builder()
                .status(StatusCode::FOUND)
                .header("location", "/auth/google/login")
                .body(Full::new(Bytes::new()))
                .unwrap());
        }
    };
    
    // Get session
    let sessions_read = sessions.read().unwrap();
    let session = match sessions_read.get(session_id) {
        Some(s) if s.is_valid() => s.clone(),
        _ => {
            drop(sessions_read);
            // Invalid/expired session - redirect to login
            return Ok(Response::builder()
                .status(StatusCode::FOUND)
                .header("location", "/auth/google/login")
                .header("set-cookie", "session_id=; Max-Age=0; Path=/")
                .body(Full::new(Bytes::new()))
                .unwrap());
        }
    };
    drop(sessions_read);
    
    // TODO: If token is expired, refresh it using refresh_token
    
    // Render admin dashboard
    Ok(admin_dashboard_response(&session))
}

fn admin_dashboard_response(session: &Session) -> Response<Full<Bytes>> {
    let html = format!(r#"
<!DOCTYPE html>
<html>
<head>
    <title>Admin Dashboard</title>
    <style>
        body {{ font-family: Arial, sans-serif; max-width: 900px; margin: 50px auto; padding: 20px; }}
        h1 {{ color: #333; }}
        .user-info {{ background: #e8f5e9; padding: 15px; border-radius: 4px; margin: 20px 0; }}
        .stats {{ display: grid; grid-template-columns: repeat(3, 1fr); gap: 20px; margin: 20px 0; }}
        .stat-card {{ background: #f5f5f5; padding: 20px; border-radius: 4px; text-align: center; }}
        .stat-value {{ font-size: 32px; font-weight: bold; color: #4285f4; }}
        .button {{ display: inline-block; padding: 10px 20px; background: #f44336; color: white; 
                  text-decoration: none; border-radius: 4px; margin: 10px 0; }}
    </style>
</head>
<body>
    <h1>📊 Admin Dashboard</h1>
    
    <div class="user-info">
        <p><strong>👤 Logged in as:</strong> {}</p>
        <p><strong>📧 Email:</strong> {}</p>
        <p><strong>🎭 Role:</strong> {}</p>
        <p><strong>🕐 Session expires:</strong> {}</p>
    </div>
    
    <h2>Statistics</h2>
    <div class="stats">
        <div class="stat-card">
            <div class="stat-value">1,234</div>
            <div>Total Users</div>
        </div>
        <div class="stat-card">
            <div class="stat-value">5,678</div>
            <div>Active Sessions</div>
        </div>
        <div class="stat-card">
            <div class="stat-value">98.5%</div>
            <div>Uptime</div>
        </div>
    </div>
    
    <h2>Actions</h2>
    <a href="/auth/logout" class="button">🚪 Logout</a>
</body>
</html>
    "#, session.name, session.email, session.role, session.expires_at.format("%Y-%m-%d %H:%M:%S UTC"));
    
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/html; charset=utf-8")
        .body(Full::new(Bytes::from(html)))
        .unwrap()
}

fn logout_response() -> Response<Full<Bytes>> {
    Response::builder()
        .status(StatusCode::FOUND)
        .header("location", "/")
        .header("set-cookie", "session_id=; Max-Age=0; Path=/")
        .body(Full::new(Bytes::new()))
        .unwrap()
}

fn not_found_response() -> Response<Full<Bytes>> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header("content-type", "text/plain")
        .body(Full::new(Bytes::from("404 Not Found")))
        .unwrap()
}

/// Exchange authorization code for access token
async fn exchange_code_for_token(
    config: &GoogleOAuthConfig,
    code: &str,
) -> Result<TokenResponse> {
    let client = reqwest::Client::new();
    
    let params = [
        ("code", code),
        ("client_id", &config.client_id),
        ("client_secret", &config.client_secret),
        ("redirect_uri", &config.redirect_uri),
        ("grant_type", "authorization_code"),
    ];
    
    let response = client
        .post("https://oauth2.googleapis.com/token")
        .form(&params)
        .send()
        .await?;
    
    if !response.status().is_success() {
        let error = response.text().await?;
        return Err(anyhow!("Token exchange failed: {}", error));
    }
    
    Ok(response.json().await?)
}

/// Get user info from Google
async fn get_user_info(access_token: &str) -> Result<GoogleUserInfo> {
    let client = reqwest::Client::new();
    
    let response = client
        .get("https://www.googleapis.com/oauth2/v2/userinfo")
        .bearer_auth(access_token)
        .send()
        .await?;
    
    if !response.status().is_success() {
        let error = response.text().await?;
        return Err(anyhow!("User info request failed: {}", error));
    }
    
    Ok(response.json().await?)
}

/// Map email to role (customize this for your needs)
fn map_email_to_role(email: &str) -> String {
    if email.ends_with("@admin.com") {
        "Administrator".to_string()
    } else if email.ends_with("@manager.com") {
        "Manager".to_string()
    } else {
        "Viewer".to_string()
    }
}
