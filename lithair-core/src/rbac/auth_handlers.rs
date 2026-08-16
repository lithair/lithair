//! Automatic authentication handlers for RBAC
//!
//! This module provides automatically generated /auth/login and /auth/logout handlers

use super::RbacUser;
use crate::session::{PersistentSessionStore, Session, SessionStore, SESSION_COOKIE_NAME};
use anyhow::Result;
use bytes::Bytes;
use chrono::Duration;
use http_body_util::Full;
use hyper::{Request, Response, StatusCode};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
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

/// Opaque MFA storage type used when the `mfa` feature is disabled.
#[cfg(not(feature = "mfa"))]
pub type MfaStorageOption = Option<()>;
/// MFA storage type used when the `mfa` feature is enabled.
#[cfg(feature = "mfa")]
pub type MfaStorageOption = Option<Arc<crate::mfa::MfaStorage>>;

/// Generate login handler
pub async fn handle_rbac_login(
    mut req: Request<hyper::body::Incoming>,
    session_store: Arc<PersistentSessionStore>,
    users: &[RbacUser],
    session_duration: u64,
    mfa_storage: MfaStorageOption,
) -> Result<Response<Full<Bytes>>> {
    use http_body_util::BodyExt;

    // Parse request body
    let body = req.body_mut().collect().await?.to_bytes();
    let login_req: LoginRequest = match serde_json::from_slice(&body) {
        Ok(req) => req,
        Err(_) => {
            return Ok(json_response(
                StatusCode::BAD_REQUEST,
                serde_json::json!({
                    "error": "Invalid JSON"
                }),
            ));
        }
    };

    // Find user from in-memory list
    let user = users
        .iter()
        .find(|u| u.username == login_req.username && u.verify_password(&login_req.password));

    let user = match user {
        Some(u) if u.active => u,
        _ => {
            return Ok(json_response(
                StatusCode::UNAUTHORIZED,
                serde_json::json!({
                    "error": "Invalid credentials"
                }),
            ));
        }
    };

    // Check if MFA is enabled for this user
    #[cfg(feature = "mfa")]
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
                            return Ok(json_response(
                                StatusCode::UNAUTHORIZED,
                                serde_json::json!({
                                    "error": "Invalid TOTP code"
                                }),
                            ));
                        }
                        // Code is valid, proceed with login
                    }
                    None => {
                        // MFA required but no code provided
                        return Ok(json_response(
                            StatusCode::UNAUTHORIZED,
                            serde_json::json!({
                                "error": "MFA required",
                                "mfa_required": true
                            }),
                        ));
                    }
                }
            }
        }
    }
    #[cfg(not(feature = "mfa"))]
    let _ = mfa_storage; // suppress unused warning

    // Create session
    let session_id = Uuid::new_v4().to_string();
    let expires_at = chrono::Utc::now() + Duration::seconds(session_duration as i64);

    let mut session = Session::new(session_id.clone(), expires_at);
    session.set("user_id", &user.username)?;
    session.set("username", &user.username)?;
    session.set("role", &user.role)?;

    // Store session
    session_store.set(session).await?;

    log::info!("User logged in: {} as {}", user.username, user.role);

    // Return session token in the body (Bearer clients) and as a cookie
    // (browsers) — see issue #219.
    let mut resp = json_response(
        StatusCode::OK,
        serde_json::json!({
            "session_token": session_id,
            "role": user.role,
            "expires_in": session_duration
        }),
    );
    resp.headers_mut().insert(
        hyper::header::SET_COOKIE,
        session_cookie(&session_id, session_duration).parse()?,
    );
    Ok(resp)
}

/// Build the `Set-Cookie` value for the session cookie.
///
/// `Secure` is unconditional: browsers treat `localhost` as a secure context,
/// anything else belongs behind TLS. Clearing (`session_cookie("", 0)`) keeps
/// the identical attributes on purpose — a browser only drops a cookie when
/// they match the ones it was set with.
pub(crate) fn session_cookie(value: &str, max_age: u64) -> String {
    format!(
        "{SESSION_COOKIE_NAME}={value}; Path=/; HttpOnly; SameSite=Strict; Secure; Max-Age={max_age}"
    )
}

/// Generate logout handler
pub async fn handle_rbac_logout(
    req: Request<hyper::body::Incoming>,
    session_store: Arc<PersistentSessionStore>,
) -> Result<Response<Full<Bytes>>> {
    // Bearer header or session cookie — same extractor as the session gate.
    let session_token = match crate::http::declarative::extract_session_token(&req) {
        Some(token) => token,
        None => {
            return Ok(json_response(
                StatusCode::UNAUTHORIZED,
                serde_json::json!({
                    "error": "No session token provided"
                }),
            ));
        }
    };

    // Get session to log username
    let session = match session_store.get(&session_token).await? {
        Some(s) => s,
        None => {
            return Ok(json_response(
                StatusCode::UNAUTHORIZED,
                serde_json::json!({
                    "error": "Invalid session"
                }),
            ));
        }
    };

    let username: String = session.get("username").unwrap_or_default();

    // Delete session
    session_store.delete(&session_token).await?;

    log::info!("User logged out: {} (session: {})", username, session_token);

    let mut resp = json_response(
        StatusCode::OK,
        serde_json::json!({
            "message": "Logged out successfully"
        }),
    );
    resp.headers_mut()
        .insert(hyper::header::SET_COOKIE, session_cookie("", 0).parse()?);
    Ok(resp)
}

/// Helper to create JSON response
fn json_response(status: StatusCode, body: serde_json::Value) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(body.to_string())))
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_cookie_carries_the_hardening_attributes_and_the_session_duration() {
        let cookie = session_cookie("abc123", 3600);
        assert_eq!(
            cookie,
            "session_token=abc123; Path=/; HttpOnly; SameSite=Strict; Secure; Max-Age=3600"
        );
    }

    #[test]
    fn clear_cookie_keeps_the_same_attributes_with_max_age_zero() {
        let set = session_cookie("abc123", 3600);
        let clear = session_cookie("", 0);
        assert_eq!(clear, "session_token=; Path=/; HttpOnly; SameSite=Strict; Secure; Max-Age=0");
        // Identical attribute set — a browser only drops the cookie on a match.
        let attrs = |c: &str| {
            c.split("; ")
                .skip(1)
                .filter(|a| !a.starts_with("Max-Age="))
                .map(str::to_string)
                .collect::<Vec<_>>()
        };
        assert_eq!(attrs(&set), attrs(&clear));
    }

    /// The cookie the login emits must be the one the gate reads back. If the
    /// two names drift, browsers get a cookie the gate ignores and every
    /// request 401s with nothing obviously wrong (issue #219).
    #[test]
    fn issued_cookie_name_matches_what_the_session_gate_extracts() {
        let cookie = session_cookie("tok-42", 60);
        let cookie_pair = cookie.split(';').next().expect("name=value pair");
        assert_eq!(cookie_pair, format!("{SESSION_COOKIE_NAME}=tok-42"));
        assert_eq!(cookie_pair.split('=').next(), Some(SESSION_COOKIE_NAME));

        let req = Request::builder()
            .header(hyper::header::COOKIE, format!("other=1; {cookie_pair}"))
            .body(())
            .expect("request");
        assert_eq!(
            crate::http::declarative::extract_session_token(&req).as_deref(),
            Some("tok-42")
        );
    }
}
