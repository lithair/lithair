//! Automatic authentication handlers for RBAC
//!
//! This module provides automatically generated /auth/login and /auth/logout handlers

use super::RbacUser;
use crate::session::{
    effective_cookie_config, PersistentSessionStore, Session, SessionCookie, SessionStore,
};
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
    // (browsers) — see issue #219. The cookie's attributes come from the
    // effective `CookieConfig` (TOML/env/`with_session_cookie`); its Max-Age
    // is the session's own lifetime so the two expire together. With
    // `enabled: false` (Bearer-only mode) only the body carries the token.
    let mut resp = json_response(
        StatusCode::OK,
        serde_json::json!({
            "session_token": session_id,
            "role": user.role,
            "expires_in": session_duration
        }),
    );
    if let Some(cookie) = session_set_cookie(&req, &session_id, session_duration) {
        resp.headers_mut().insert(hyper::header::SET_COOKIE, cookie.parse()?);
    }
    Ok(resp)
}

/// `Set-Cookie` value the login emits: the request's effective cookie config
/// with `Max-Age` = the session duration. `None` when the cookie is disabled.
fn session_set_cookie<B>(
    req: &Request<B>,
    session_id: &str,
    session_duration: u64,
) -> Option<String> {
    let mut config = (*effective_cookie_config(req)).clone();
    if !config.enabled {
        return None;
    }
    config.max_age = Some(session_duration as i64);
    Some(SessionCookie::new(config).build_set_cookie(session_id, None))
}

/// Generate logout handler.
///
/// Idempotent and expiry-aware (issue #219 follow-up):
/// - every session the request names — `Authorization: Bearer` AND the
///   session cookie, when both are present with different values — is
///   deleted from the store, expired entries included (opportunistic
///   cleanup);
/// - the response is 200 when at least one of them was live, 401 otherwise
///   (no token, or only unknown/expired ones);
/// - the clearing `Set-Cookie` (`Max-Age=0`, same attributes as the login's)
///   is sent on EVERY path, 401 included, so a browser holding a dead cookie
///   leaves clean. Omitted only when the cookie is disabled (Bearer-only).
pub async fn handle_rbac_logout(
    req: Request<hyper::body::Incoming>,
    session_store: Arc<PersistentSessionStore>,
) -> Result<Response<Full<Bytes>>> {
    let candidates = crate::http::declarative::session_token_candidates(&req);

    let mut live = 0usize;
    for token in &candidates {
        if let Some(session) = session_store.get(token).await? {
            if !session.is_expired() {
                live += 1;
                let username: String = session.get("username").unwrap_or_default();
                log::info!("User logged out: {} (session: {})", username, token);
            }
            session_store.delete(token).await?;
        }
    }

    let (status, body) = if candidates.is_empty() {
        (
            StatusCode::UNAUTHORIZED,
            serde_json::json!({ "error": "No session token provided" }),
        )
    } else if live == 0 {
        (StatusCode::UNAUTHORIZED, serde_json::json!({ "error": "Invalid session" }))
    } else {
        (StatusCode::OK, serde_json::json!({ "message": "Logged out successfully" }))
    };

    let mut resp = json_response(status, body);
    // Same scope/flags as the login's Set-Cookie, Max-Age=0 — unconditionally.
    let cookie_config = effective_cookie_config(&req);
    if cookie_config.enabled {
        let clear = SessionCookie::new((*cookie_config).clone()).build_delete_cookie();
        resp.headers_mut().insert(hyper::header::SET_COOKIE, clear.parse()?);
    }
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
    use crate::session::{CookieConfig, MemorySessionStore, SessionConfig, SessionMiddleware};

    #[test]
    fn set_cookie_uses_the_effective_config_and_the_session_duration() {
        // No server in the loop → defaults.
        let req = Request::builder().body(()).expect("request");
        assert_eq!(
            session_set_cookie(&req, "abc123", 3600).as_deref(),
            Some("session_token=abc123; Path=/; Max-Age=3600; Secure; HttpOnly; SameSite=Lax")
        );

        // A server-injected config (what `LithairServer` does at dispatch).
        let mut req = Request::builder().body(()).expect("request");
        req.extensions_mut().insert(Arc::new(CookieConfig {
            name: "sid".to_string(),
            secure: false,
            ..CookieConfig::default()
        }));
        assert_eq!(
            session_set_cookie(&req, "abc123", 60).as_deref(),
            Some("sid=abc123; Path=/; Max-Age=60; HttpOnly; SameSite=Lax")
        );

        // Bearer-only mode: no cookie at all.
        let mut req = Request::builder().body(()).expect("request");
        req.extensions_mut()
            .insert(Arc::new(CookieConfig { enabled: false, ..CookieConfig::default() }));
        assert_eq!(session_set_cookie(&req, "abc123", 60), None);
    }

    /// D12(a): the cookie the login emits is the one BOTH extractors read
    /// back — the gate/guards' `extract_session_token` and
    /// `SessionMiddleware` — under the default and under an override.
    #[tokio::test]
    async fn issued_cookie_is_read_back_by_both_extractors() {
        for config in [
            CookieConfig::default(),
            CookieConfig { name: "sid".to_string(), ..CookieConfig::default() },
            CookieConfig { host_prefix: true, ..CookieConfig::default() },
        ] {
            let shared = Arc::new(config.clone());
            let mut login_req = Request::builder().body(()).expect("request");
            login_req.extensions_mut().insert(Arc::clone(&shared));
            let set_cookie = session_set_cookie(&login_req, "tok-42", 60).expect("cookie enabled");
            let cookie_pair = set_cookie.split(';').next().expect("name=value pair");
            assert_eq!(cookie_pair, format!("{}=tok-42", config.effective_name()));

            // 1) gate / guards / validate / logout extractor.
            let mut req = Request::builder()
                .header(hyper::header::COOKIE, format!("other=1; {cookie_pair}"))
                .body(())
                .expect("request");
            req.extensions_mut().insert(Arc::clone(&shared));
            assert_eq!(
                crate::http::declarative::extract_session_token(&req).as_deref(),
                Some("tok-42"),
                "config={config:?}"
            );

            // 2) SessionMiddleware built on the same CookieConfig.
            let store = Arc::new(MemorySessionStore::new());
            let expires_at = chrono::Utc::now() + Duration::hours(1);
            store.set(Session::new("tok-42".to_string(), expires_at)).await.unwrap();
            let mut session_config = SessionConfig::cookie_only();
            session_config.cookie_config = config.clone();
            let middleware = SessionMiddleware::new(store, session_config);
            let session = middleware.extract_session(&req).await.expect("store ok");
            assert_eq!(session.map(|s| s.id).as_deref(), Some("tok-42"), "config={config:?}");
        }
    }
}
