//! Automatic authentication handlers for RBAC
//!
//! This module provides automatically generated /auth/login and /auth/logout handlers

use super::{RbacUser};
use crate::session::{PersistentSessionStore, Session, SessionStore};
use anyhow::Result;
use bytes::Bytes;
use chrono::Duration;
use http_body_util::Full;
use hyper::{Request, Response, StatusCode};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    pub totp_code: Option<String>,
}

#[derive(Debug, Serialize)]
#[allow(dead_code)]
pub struct LoginResponse {
    pub session_token: String,
    pub role: String,
    pub expires_in: i64,
}

/// Generate login handler
pub async fn handle_rbac_login(
    mut req: Request<hyper::body::Incoming>,
    session_store: Arc<PersistentSessionStore>,
    users: &[RbacUser],
    session_duration: u64,
    mfa_storage: Option<Arc<crate::mfa::MfaStorage>>,
) -> Result<Response<Full<Bytes>>> {
    use http_body_util::BodyExt;
    
    // Parse request body
    let body = req.body_mut().collect().await?.to_bytes();
    let login_req: LoginRequest = match serde_json::from_slice(&body) {
        Ok(req) => req,
        Err(_) => {
            return Ok(json_response(StatusCode::BAD_REQUEST, serde_json::json!({
                "error": "Invalid JSON"
            })));
        }
    };
    
    // Find user from in-memory list
    let user = users.iter().find(|u| {
        u.username == login_req.username && u.verify_password(&login_req.password)
    });
    
    let user = match user {
        Some(u) if u.active => u,
        _ => {
            return Ok(json_response(StatusCode::UNAUTHORIZED, serde_json::json!({
                "error": "Invalid credentials"
            })));
        }
    };
    
    // Check if MFA is enabled for this user
    if let Some(mfa_store) = mfa_storage {
        if let Ok(Some(mfa_data)) = mfa_store.get(&user.username).await {
            if mfa_data.status.enabled {
                // MFA is enabled - verify TOTP code
                match &login_req.totp_code {
                    Some(code) => {
                        // Validate TOTP code
                        use crate::mfa::TotpValidator;
                        let valid = TotpValidator::validate(&mfa_data.secret, code)?;
                        
                        if !valid {
                            return Ok(json_response(StatusCode::UNAUTHORIZED, serde_json::json!({
                                "error": "Invalid TOTP code"
                            })));
                        }
                        // Code is valid, proceed with login
                    }
                    None => {
                        // MFA required but no code provided
                        return Ok(json_response(StatusCode::UNAUTHORIZED, serde_json::json!({
                            "error": "MFA required",
                            "mfa_required": true
                        })));
                    }
                }
            }
        }
    }
    
    // Create session
    let session_id = Uuid::new_v4().to_string();
    let expires_at = chrono::Utc::now() + Duration::seconds(session_duration as i64);
    
    let mut session = Session::new(session_id.clone(), expires_at);
    session.set("user_id", &user.username)?;
    session.set("username", &user.username)?;
    session.set("role", &user.role)?;
    
    // Store session
    session_store.set(session).await?;

    log::info!("✅ User logged in: {} as {}", user.username, user.role);
    
    // Return session token
    Ok(json_response(StatusCode::OK, serde_json::json!({
        "session_token": session_id,
        "role": user.role,
        "expires_in": session_duration
    })))
}

/// Generate logout handler
pub async fn handle_rbac_logout(
    req: Request<hyper::body::Incoming>,
    session_store: Arc<PersistentSessionStore>,
) -> Result<Response<Full<Bytes>>> {
    // Extract session token from Authorization header
    let auth_header = req.headers().get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));
    
    let session_token = match auth_header {
        Some(token) => token,
        None => {
            return Ok(json_response(StatusCode::UNAUTHORIZED, serde_json::json!({
                "error": "No session token provided"
            })));
        }
    };
    
    // Get session to log username
    let session = match session_store.get(session_token).await? {
        Some(s) => s,
        None => {
            return Ok(json_response(StatusCode::UNAUTHORIZED, serde_json::json!({
                "error": "Invalid session"
            })));
        }
    };
    
    let username: String = session.get("username").unwrap_or_default();
    
    // Delete session
    session_store.delete(session_token).await?;
    
    log::info!("👋 User logged out: {} (session: {})", username, session_token);
    
    Ok(json_response(StatusCode::OK, serde_json::json!({
        "message": "Logged out successfully"
    })))
}

/// Helper to create JSON response
fn json_response(status: StatusCode, body: serde_json::Value) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(body.to_string())))
        .unwrap()
}
