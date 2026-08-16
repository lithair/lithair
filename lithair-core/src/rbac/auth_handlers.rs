//! Automatic authentication handlers for RBAC
//!
//! This module provides automatically generated /auth/login and /auth/logout handlers

use super::RbacUser;
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

    // The token goes back two ways on purpose. The body is what a script or a
    // headless client reads and replays as `Authorization: Bearer`. The cookie
    // is what a browser stores and sends by itself — without it, a browser
    // login means JavaScript capturing the token out of the JSON and attaching
    // it to every later request by hand, which is both awkward and a good way
    // to leak it into somewhere scriptable. The session gate and the route
    // guards already accept either form (`http::declarative`), so this only
    // closes the half that was missing: the framework accepted cookie sessions
    // and never issued one.
    //
    // Existing Bearer clients are unaffected — they ignore Set-Cookie.
    Ok(json_response_with_session_cookie(
        StatusCode::OK,
        serde_json::json!({
            "session_token": session_id,
            "role": user.role,
            "expires_in": session_duration
        }),
        Some((&session_id, session_duration)),
    ))
}

/// Generate logout handler
pub async fn handle_rbac_logout(
    req: Request<hyper::body::Incoming>,
    session_store: Arc<PersistentSessionStore>,
) -> Result<Response<Full<Bytes>>> {
    // Bearer header or session cookie: a browser that logged in through this
    // same handler has the cookie and nothing else, so reading only the header
    // made logout unreachable for it.
    let token_owned = extract_login_session_token(&req);
    let session_token = match token_owned.as_deref() {
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
    let session = match session_store.get(session_token).await? {
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
    session_store.delete(session_token).await?;

    log::info!("User logged out: {} (session: {})", username, session_token);

    // Max-Age=0 tells the browser to drop it. Without this the session is gone
    // server-side but the browser keeps presenting a dead token on every
    // request, which reads as "still logged in" until something 401s.
    Ok(json_response_with_session_cookie(
        StatusCode::OK,
        serde_json::json!({
            "message": "Logged out successfully"
        }),
        None,
    ))
}

/// Name of the session cookie.
///
/// The same name the session extractor in `http::declarative` looks for — the
/// two have to agree or a cookie issued here would never be recognised.
pub(crate) const SESSION_COOKIE: &str = "session_token";

/// Read a session token from the `Authorization: Bearer` header, falling back
/// to the session cookie.
fn extract_login_session_token(req: &Request<hyper::body::Incoming>) -> Option<String> {
    if let Some(token) = req
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        return Some(token.to_string());
    }

    req.headers()
        .get(http::header::COOKIE)
        .and_then(|h| h.to_str().ok())?
        .split(';')
        .find_map(|part| part.trim().strip_prefix(&format!("{SESSION_COOKIE}=")))
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

/// Build the `Set-Cookie` value for a session.
///
/// `HttpOnly` keeps it away from page scripts, `SameSite=Strict` stops another
/// site driving an API with the user's session, and `Secure` means it never
/// travels in clear. `Secure` is unconditional because browsers treat
/// `localhost` as a secure context, so development still works and everything
/// else belongs behind TLS.
pub(crate) fn session_cookie_header(session: Option<(&str, u64)>) -> String {
    match session {
        Some((token, max_age)) => format!(
            "{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Secure; Max-Age={max_age}"
        ),
        None => format!("{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Secure; Max-Age=0"),
    }
}

/// JSON response carrying a session cookie. `None` clears it.
fn json_response_with_session_cookie(
    status: StatusCode,
    body: serde_json::Value,
    session: Option<(&str, u64)>,
) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .header("Set-Cookie", session_cookie_header(session))
        .body(Full::new(Bytes::from(body.to_string())))
        .unwrap()
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
mod session_cookie_tests {
    use super::*;

    #[test]
    fn a_session_cookie_carries_the_token_and_the_protections() {
        let header = session_cookie_header(Some(("abc123", 28800)));

        assert!(header.starts_with("session_token=abc123;"), "{header}");
        assert!(header.contains("HttpOnly"), "must be out of reach of page scripts: {header}");
        assert!(
            header.contains("SameSite=Strict"),
            "must not ride cross-site requests: {header}"
        );
        assert!(header.contains("Secure"), "must not travel in clear: {header}");
        assert!(header.contains("Max-Age=28800"), "must expire with the session: {header}");
    }

    #[test]
    fn clearing_uses_max_age_zero_and_keeps_the_same_attributes() {
        // A browser only drops a cookie when the attributes match the ones it
        // was set with, so this is not cosmetic.
        let header = session_cookie_header(None);

        assert!(header.starts_with("session_token=;"), "{header}");
        assert!(header.contains("Max-Age=0"), "{header}");
        assert!(header.contains("HttpOnly"));
        assert!(header.contains("SameSite=Strict"));
        assert!(header.contains("Secure"));
    }

    #[test]
    fn the_cookie_name_matches_what_the_session_extractor_looks_for() {
        // If these two ever drift, a login would issue a cookie the gate
        // ignores, and every authenticated request from a browser would 401.
        assert_eq!(SESSION_COOKIE, "session_token");
    }
}
