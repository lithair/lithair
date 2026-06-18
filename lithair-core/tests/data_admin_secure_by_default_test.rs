//! Issue #143 — `with_data_admin()` must be secure by default.
//!
//! Before the fix, enabling the data-admin / frontend-admin planes added no
//! auth guard unless `with_data_admin_ui()` was also called; `/_admin/frontend/*`
//! had no guard at all. On a server secured with sessions/RBAC (the documented
//! production pattern) that left the admin planes open to the world.
//!
//! These tests assert the contract:
//!   - with sessions configured, an UNauthenticated request to a mutating admin
//!     plane (`/_admin/frontend`, `/_admin/data/import`) gets 401;
//!   - a request bearing a valid session token is NOT blocked by the guard;
//!   - `with_data_admin_public()` opts out (no guard);
//!   - with NO session store, the guard is a no-op (single-user deployments are
//!     not broken).
//!
//! Session-setup pattern mirrors `require_session_test.rs`.

use chrono::Utc;
use lithair_core::app::LithairServer;
use lithair_core::session::{
    MemorySessionStore, PersistentSessionStore, Session, SessionManager, SessionStore,
};
use std::time::Duration;

/// Spawn a server with the data-admin plane enabled.
///
/// `public` selects `with_data_admin_public()` (no guard) over `with_data_admin()`.
/// `with_session_store` wires a session store (so the guard can actually enforce)
/// and seeds one valid token, returned for authenticated requests.
async fn spawn_admin_server(
    public: bool,
    with_session_store: bool,
    data_dir: &std::path::Path,
) -> (String, Option<String>) {
    let port = portpicker::pick_unused_port().expect("free port available");
    let base_url = format!("http://127.0.0.1:{}", port);

    let session_dir = data_dir.join("sessions");
    std::fs::create_dir_all(&session_dir).expect("create session dir");

    let seeded_token = if with_session_store {
        let store = PersistentSessionStore::new(session_dir.clone()).expect("session store");
        let token = format!("test-session-{}", uuid::Uuid::new_v4());
        let session = Session::new(token.clone(), Utc::now() + chrono::Duration::hours(1));
        store.set(session).await.expect("seed session");
        Some(token)
    } else {
        None
    };

    let mut builder = LithairServer::new().with_host("127.0.0.1").with_port(port);
    builder = if public { builder.with_data_admin_public() } else { builder.with_data_admin() };

    if with_session_store {
        let store = PersistentSessionStore::new(session_dir).expect("session store");
        let manager = SessionManager::new(store);
        builder = builder.with_sessions(manager);
    }

    tokio::spawn(async move {
        if let Err(e) = builder.serve().await {
            eprintln!("test server error: {}", e);
        }
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .expect("reqwest client");
    let health_url = format!("{}/health", base_url);
    for _ in 0..50 {
        if let Ok(resp) = client.get(&health_url).send().await {
            if resp.status().is_success() {
                return (base_url, seeded_token);
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("test server failed to start on port {}", port);
}

/// The core of #143: with sessions configured, the frontend admin plane is NOT
/// reachable unauthenticated.
#[tokio::test]
async fn frontend_admin_unauthenticated_is_401() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (base, _token) = spawn_admin_server(false, true, tmp.path()).await;

    let resp = reqwest::Client::new()
        .get(format!("{}/_admin/frontend", base))
        .send()
        .await
        .expect("request sent");
    assert_eq!(resp.status(), 401, "GET /_admin/frontend must be 401 without auth (#143)");
    let body: serde_json::Value = resp.json().await.expect("json body");
    assert_eq!(body["error"], "Authentication required", "canonical 401 shape");
}

/// The data-admin import plane is likewise guarded (a mutating write).
#[tokio::test]
async fn data_import_unauthenticated_is_401() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (base, _token) = spawn_admin_server(false, true, tmp.path()).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/_admin/data/import", base))
        .json(&serde_json::json!({ "models": [] }))
        .send()
        .await
        .expect("request sent");
    assert_eq!(resp.status(), 401, "POST /_admin/data/import must be 401 without auth (#143)");
}

/// A valid session token passes the guard (the plane is reachable, not 401).
#[tokio::test]
async fn frontend_admin_with_valid_session_is_not_blocked() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (base, token) = spawn_admin_server(false, true, tmp.path()).await;
    let token = token.expect("seeded session token");

    let resp = reqwest::Client::new()
        .get(format!("{}/_admin/frontend", base))
        .bearer_auth(&token)
        .send()
        .await
        .expect("request sent");
    assert_ne!(resp.status(), 401, "a valid session must not be blocked by the guard");
    assert!(
        resp.status().is_success(),
        "authenticated list should succeed: {}",
        resp.status()
    );
}

/// Opt-out: `with_data_admin_public()` registers no guard, so the plane is open
/// even with sessions configured (documented dev/internal escape hatch).
#[tokio::test]
async fn public_opt_out_leaves_plane_open() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (base, _token) = spawn_admin_server(true, true, tmp.path()).await;

    let resp = reqwest::Client::new()
        .get(format!("{}/_admin/frontend", base))
        .send()
        .await
        .expect("request sent");
    assert_ne!(resp.status(), 401, "with_data_admin_public() must not guard the plane");
    assert!(resp.status().is_success(), "public plane should serve: {}", resp.status());
}

/// A `MemorySessionStore` (the documented dev/test store) must be honored, not
/// locked out. Before the recognizer covered it, a valid session would still
/// get 401 because the guard could not resolve the store shape (issue #143
/// review). Asserts: valid token → not blocked; no token → still 401 (enforced).
#[tokio::test]
async fn memory_session_store_is_not_locked_out() {
    let port = portpicker::pick_unused_port().expect("free port available");
    let base = format!("http://127.0.0.1:{}", port);

    // Seed a session, then hand the SAME store (Arc-shared internally) to the
    // server so the seeded token is visible.
    let store = MemorySessionStore::new();
    let token = format!("mem-session-{}", uuid::Uuid::new_v4());
    store
        .set(Session::new(token.clone(), Utc::now() + chrono::Duration::hours(1)))
        .await
        .expect("seed session");
    let manager = SessionManager::new(store);

    let builder = LithairServer::new()
        .with_host("127.0.0.1")
        .with_port(port)
        .with_data_admin()
        .with_sessions(manager);
    tokio::spawn(async move {
        if let Err(e) = builder.serve().await {
            eprintln!("test server error: {}", e);
        }
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .expect("reqwest client");
    let health_url = format!("{}/health", base);
    let mut ready = false;
    for _ in 0..50 {
        if let Ok(resp) = client.get(&health_url).send().await {
            if resp.status().is_success() {
                ready = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(ready, "server failed to start on {}", port);

    // Valid token → NOT locked out (the recognizer now resolves MemorySessionStore).
    let authed = client
        .get(format!("{}/_admin/frontend", base))
        .bearer_auth(&token)
        .send()
        .await
        .expect("authed request");
    assert_ne!(
        authed.status(),
        401,
        "MemorySessionStore deployment must not lock out a valid session"
    );
    assert!(authed.status().is_success(), "authed list should succeed: {}", authed.status());

    // No token → still enforced.
    let anon = client
        .get(format!("{}/_admin/frontend", base))
        .send()
        .await
        .expect("anon request");
    assert_eq!(anon.status(), 401, "MemorySessionStore: unauthenticated must still be 401");
}

/// Backward compat: with NO session store the guard is a no-op — a single-user
/// deployment that never wired sessions is not suddenly locked out.
#[tokio::test]
async fn no_session_store_does_not_lock_out() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (base, _token) = spawn_admin_server(false, false, tmp.path()).await;

    let resp = reqwest::Client::new()
        .get(format!("{}/_admin/frontend", base))
        .send()
        .await
        .expect("request sent");
    assert_ne!(resp.status(), 401, "without a session store the guard must be a no-op");
    assert!(resp.status().is_success(), "plane should serve when no auth is configured");
}
