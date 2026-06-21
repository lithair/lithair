//! Issue #149 (PR 1) — `RouteGuard::RequireRole` is implemented, not a stub.
//!
//! Before, `RequireRole` denied every request ("not yet implemented"). It now
//! resolves the live session (same recognizer path #143 fixed) and checks the
//! role stored at `session.data["role"]` — the convention the login examples
//! establish via `session.set("role", …)`.
//!
//! Contract:
//!   - a session whose role is in the allowed set passes;
//!   - a session whose role is NOT allowed gets 403;
//!   - no session (unauthenticated) gets 403 (authz fails closed).
//!
//! Guards run before routing, so a passing request to a path with no route
//! 404s — that's the signal the guard allowed it (NOT 403).

use chrono::Utc;
use lithair_core::app::LithairServer;
use lithair_core::http::RouteGuard;
use lithair_core::session::{PersistentSessionStore, Session, SessionManager, SessionStore};
use std::time::Duration;

/// Spawn a server guarding `/protected/*` with `RequireRole(["Admin"])`, seeding
/// two sessions: one `Admin`, one `User`. Returns (base_url, admin_token,
/// user_token).
async fn spawn(dir: &std::path::Path) -> (String, String, String) {
    let port = portpicker::pick_unused_port().expect("free port");
    let base = format!("http://127.0.0.1:{}", port);
    let session_dir = dir.join("sessions");
    std::fs::create_dir_all(&session_dir).expect("session dir");

    // Seed Admin + User sessions into the on-disk store the server will reload.
    let (admin_token, user_token) = {
        let store = PersistentSessionStore::new(session_dir.clone()).expect("store");
        let admin = format!("admin-{}", uuid::Uuid::new_v4());
        let user = format!("user-{}", uuid::Uuid::new_v4());
        let mut a = Session::new(admin.clone(), Utc::now() + chrono::Duration::hours(1));
        a.set("role", "Admin").expect("set admin role");
        store.set(a).await.expect("seed admin");
        let mut u = Session::new(user.clone(), Utc::now() + chrono::Duration::hours(1));
        u.set("role", "User").expect("set user role");
        store.set(u).await.expect("seed user");
        (admin, user)
    };

    let store = PersistentSessionStore::new(session_dir).expect("store");
    let manager = SessionManager::new(store);
    let builder = LithairServer::new()
        .with_host("127.0.0.1")
        .with_port(port)
        .with_sessions(manager)
        .with_route_guard(
            "/protected/*",
            RouteGuard::RequireRole { roles: vec!["Admin".to_string()], redirect_to: None },
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
                return (base, admin_token, user_token);
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("server failed to start on {}", port);
}

#[tokio::test]
async fn require_role_allows_matching_role() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (base, admin_token, _user) = spawn(tmp.path()).await;

    let resp = reqwest::Client::new()
        .get(format!("{}/protected/thing", base))
        .bearer_auth(&admin_token)
        .send()
        .await
        .expect("request");
    // Guard passed (no route under /protected → 404). The point: NOT 403.
    assert_ne!(resp.status(), 403, "an allowed role must pass the guard");
}

#[tokio::test]
async fn require_role_denies_wrong_role() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (base, _admin, user_token) = spawn(tmp.path()).await;

    let resp = reqwest::Client::new()
        .get(format!("{}/protected/thing", base))
        .bearer_auth(&user_token)
        .send()
        .await
        .expect("request");
    assert_eq!(resp.status(), 403, "a non-allowed role must be 403");
}

#[tokio::test]
async fn require_role_denies_unauthenticated() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (base, _admin, _user) = spawn(tmp.path()).await;

    let resp = reqwest::Client::new()
        .get(format!("{}/protected/thing", base))
        .send()
        .await
        .expect("request");
    assert_eq!(resp.status(), 403, "no session must be 403 (authz fails closed)");
}
