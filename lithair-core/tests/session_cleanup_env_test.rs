//! `[sessions]` env vars that were read but inert until D8 / the PR-B holes:
//! - `LT_SESSION_CLEANUP_INTERVAL=1`: `with_rbac_config` now wraps its store
//!   in a `SessionManager`, so an expired session planted in the store
//!   disappears within a couple of seconds — the cleanup task is running;
//! - `LT_SESSION_COOKIE_ENABLED=false`: Bearer-only mode — the login answers
//!   without `Set-Cookie`, the gate still accepts the Bearer, ignores the
//!   cookie, and the logout emits no clear.
//!
//! One process, one test: env vars are process-wide.

use chrono::{Duration as ChronoDuration, Utc};
use lithair_core::app::LithairServer;
use lithair_core::rbac::{RbacUser, ServerRbacConfig};
use lithair_core::session::{PersistentSessionStore, Session, SessionManager, SessionStore};
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
struct Account {
    #[http(expose)]
    id: String,
    #[http(expose)]
    name: String,
}

async fn spawn_server(data_dir: &std::path::Path) -> (String, Arc<PersistentSessionStore>) {
    let port = portpicker::pick_unused_port().expect("free port available");
    let base_url = format!("http://127.0.0.1:{}", port);

    let rbac = ServerRbacConfig::new()
        .with_user(RbacUser::new("alice", "s3cret", "Admin"))
        .with_session_store(data_dir.join("sessions").to_string_lossy().to_string())
        .with_session_duration(1234);

    let builder = LithairServer::new()
        .with_host("127.0.0.1")
        .with_port(port)
        .with_model::<Account>(
            data_dir.join("accounts").to_string_lossy().to_string(),
            "/api/accounts",
        )
        .with_rbac_config(rbac)
        .with_models_require_session(true);

    let manager: Arc<SessionManager<PersistentSessionStore>> = builder
        .session_manager()
        .expect("with_rbac_config registers a session store")
        .downcast()
        .expect("with_rbac_config registers an Arc<SessionManager<PersistentSessionStore>>");
    assert_eq!(manager.config().cleanup_interval, Duration::from_secs(1), "env var consumed");
    let store = manager.store();

    tokio::spawn(async move {
        if let Err(e) = builder.serve().await {
            eprintln!("test server error: {}", e);
        }
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .expect("client");
    for _ in 0..50 {
        if let Ok(resp) = client.get(format!("{}/health", base_url)).send().await {
            if resp.status().is_success() {
                return (base_url, store);
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("test server failed to start on port {}", port);
}

#[tokio::test]
async fn cleanup_runs_and_cookie_can_be_disabled() {
    std::env::set_var("LT_SESSION_CLEANUP_INTERVAL", "1");
    std::env::set_var("LT_SESSION_COOKIE_ENABLED", "false");

    let tmp = tempfile::tempdir().expect("tmpdir");
    let (base, store) = spawn_server(tmp.path()).await;
    let client = reqwest::Client::new();

    // --- D8: the manager's cleanup task sweeps expired sessions. ---
    store
        .set(Session::new("expired".to_string(), Utc::now() - ChronoDuration::seconds(5)))
        .await
        .expect("set");
    store
        .set(Session::new("live".to_string(), Utc::now() + ChronoDuration::hours(1)))
        .await
        .expect("set");
    let mut swept = false;
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if store.get("expired").await.expect("get").is_none() {
            swept = true;
            break;
        }
    }
    assert!(swept, "the expired session must be swept within ~4s at cleanup_interval=1");
    assert!(
        store.get("live").await.expect("get").is_some(),
        "live sessions survive the sweep"
    );

    // --- cookie_enabled=false: Bearer-only. ---
    let resp = client
        .post(format!("{}/auth/login", base))
        .json(&serde_json::json!({"username": "alice", "password": "s3cret"}))
        .send()
        .await
        .expect("login sent");
    assert_eq!(resp.status(), 200);
    assert!(resp.headers().get("set-cookie").is_none(), "no Set-Cookie in Bearer-only mode");
    let body: serde_json::Value = resp.json().await.expect("json body");
    let token = body["session_token"].as_str().expect("token still in the body").to_string();

    let resp = client
        .get(format!("{}/api/accounts", base))
        .bearer_auth(&token)
        .send()
        .await
        .expect("gated request sent");
    assert_eq!(resp.status(), 200, "the gate still accepts the Bearer");
    let resp = client
        .get(format!("{}/api/accounts", base))
        .header("cookie", format!("session_token={token}"))
        .send()
        .await
        .expect("gated request sent");
    assert_eq!(resp.status(), 401, "the cookie is not read in Bearer-only mode");

    let resp = client
        .post(format!("{}/auth/logout", base))
        .bearer_auth(&token)
        .send()
        .await
        .expect("logout sent");
    assert_eq!(resp.status(), 200);
    assert!(resp.headers().get("set-cookie").is_none(), "no clear in Bearer-only mode");
    assert!(store.get(&token).await.expect("get").is_none());
}
