//! Issue #149 (PR 2) — `with_admin_roles(...)` scopes the admin API planes.
//!
//! Opt-in role tiers: the frontend plane (`/_admin/frontend/*`) is allowed to a
//! broader set (content-manager / admin / super-admin), the data plane
//! (`/_admin/data/*`) only to admin / super-admin. A session's role lives in
//! `session.data["role"]`.
//!
//! Asserts the security boundary:
//!   - content-manager → frontend plane OK, data plane 403;
//!   - admin → both planes OK;
//!   - unauthenticated → 403 on both.

use chrono::Utc;
use lithair_core::app::LithairServer;
use lithair_core::session::{PersistentSessionStore, Session, SessionManager, SessionStore};
use std::time::Duration;

async fn seed(store: &PersistentSessionStore, role: &str) -> String {
    let token = format!("{}-{}", role, uuid::Uuid::new_v4());
    let mut s = Session::new(token.clone(), Utc::now() + chrono::Duration::hours(1));
    s.set("role", role).expect("set role");
    store.set(s).await.expect("seed");
    token
}

/// Returns (base_url, content_manager_token, admin_token).
async fn spawn(dir: &std::path::Path) -> (String, String, String) {
    let port = portpicker::pick_unused_port().expect("free port");
    let base = format!("http://127.0.0.1:{}", port);
    let session_dir = dir.join("sessions");
    std::fs::create_dir_all(&session_dir).expect("session dir");

    let (cm_token, admin_token) = {
        let store = PersistentSessionStore::new(session_dir.clone()).expect("store");
        (seed(&store, "content-manager").await, seed(&store, "admin").await)
    };

    let store = PersistentSessionStore::new(session_dir).expect("store");
    let manager = SessionManager::new(store);
    let builder = LithairServer::new()
        .with_host("127.0.0.1")
        .with_port(port)
        .with_sessions(manager)
        .with_admin_roles(
            ["content-manager", "admin", "super-admin"], // frontend plane
            ["admin", "super-admin"],                    // data plane
        );

    tokio::spawn(async move {
        if let Err(e) = builder.serve().await {
            eprintln!("test server error: {}", e);
        }
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .expect("client");
    let health = format!("{}/health", base);
    for _ in 0..50 {
        if let Ok(r) = client.get(&health).send().await {
            if r.status().is_success() {
                return (base, cm_token, admin_token);
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("server failed to start on {}", port);
}

#[tokio::test]
async fn content_manager_reaches_frontend_plane_but_not_data_plane() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (base, cm, _admin) = spawn(tmp.path()).await;
    let client = reqwest::Client::new();

    // Frontend plane: allowed (content-manager is in the frontend role set).
    let fe = client
        .get(format!("{}/_admin/frontend", base))
        .bearer_auth(&cm)
        .send()
        .await
        .expect("frontend req");
    assert_ne!(fe.status(), 403, "content-manager must reach the frontend plane");
    assert!(fe.status().is_success(), "frontend list should succeed: {}", fe.status());

    // Data plane: denied (content-manager is NOT in the data role set).
    let data = client
        .get(format!("{}/_admin/data/models", base))
        .bearer_auth(&cm)
        .send()
        .await
        .expect("data req");
    assert_eq!(data.status(), 403, "content-manager must NOT reach the data plane");
}

#[tokio::test]
async fn admin_reaches_both_planes() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (base, _cm, admin) = spawn(tmp.path()).await;
    let client = reqwest::Client::new();

    for path in ["/_admin/frontend", "/_admin/data/models"] {
        let resp = client
            .get(format!("{}{}", base, path))
            .bearer_auth(&admin)
            .send()
            .await
            .expect("req");
        assert_ne!(resp.status(), 403, "admin must reach {}", path);
        assert!(
            resp.status().is_success(),
            "{} should succeed for admin: {}",
            path,
            resp.status()
        );
    }
}

#[tokio::test]
async fn unauthenticated_is_denied_on_both_planes() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (base, _cm, _admin) = spawn(tmp.path()).await;
    let client = reqwest::Client::new();

    for path in ["/_admin/frontend", "/_admin/data/models"] {
        let resp = client.get(format!("{}{}", base, path)).send().await.expect("req");
        assert_eq!(resp.status(), 403, "unauthenticated must be denied on {}", path);
    }
}
