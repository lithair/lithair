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

struct TestServer {
    base: String,
    content_manager_token: String,
    admin_token: String,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<anyhow::Result<()>>,
}

impl TestServer {
    async fn stop(mut self) {
        self.stop.take().expect("shutdown sender").send(()).expect("signal shutdown");
        tokio::time::timeout(Duration::from_secs(15), &mut self.task)
            .await
            .expect("shutdown deadline")
            .expect("server task")
            .expect("serve result");
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        // Also clean up on a failed assertion/startup timeout. The task owns
        // its temporary directory, so files outlive the server future.
        self.task.abort();
    }
}

async fn spawn() -> TestServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("reserve listener");
    let base = format!("http://{}", listener.local_addr().expect("listener address"));
    let dir = tempfile::tempdir().expect("tmpdir");
    let session_dir = dir.path().join("sessions");
    std::fs::create_dir_all(&session_dir).expect("session dir");

    let (cm_token, admin_token) = {
        let store = PersistentSessionStore::new(session_dir.clone()).expect("store");
        (seed(&store, "content-manager").await, seed(&store, "admin").await)
    };

    let store = PersistentSessionStore::new(session_dir).expect("store");
    let server = LithairServer::new()
        .with_data_dir(dir.path().to_string_lossy())
        .with_sessions(SessionManager::new(store))
        .with_admin_roles(
            ["content-manager", "admin", "super-admin"], // frontend plane
            ["admin", "super-admin"],                    // data plane
        )
        .build()
        .expect("build server");
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let _dir = dir;
        server
            .serve_with_listener(listener, async {
                let _ = stopped.await;
            })
            .await
    });
    let mut fixture =
        TestServer { base, content_manager_token: cm_token, admin_token, stop: Some(stop), task };
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .expect("client");
    let health = format!("{}/health", fixture.base);
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            tokio::select! {
                biased;
                result = &mut fixture.task => panic!("server exited during startup: {result:?}"),
                response = client.get(&health).send() => {
                    if response.is_ok_and(|r| r.status().is_success()) { break; }
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("server readiness deadline");
    fixture
}

#[tokio::test]
async fn content_manager_reaches_frontend_plane_but_not_data_plane() {
    let server = spawn().await;
    let base = &server.base;
    let cm = &server.content_manager_token;
    let client = reqwest::Client::new();

    // Frontend plane: allowed (content-manager is in the frontend role set).
    let fe = client
        .get(format!("{}/_admin/frontend", base))
        .bearer_auth(cm)
        .send()
        .await
        .expect("frontend req");
    assert_ne!(fe.status(), 403, "content-manager must reach the frontend plane");
    assert!(fe.status().is_success(), "frontend list should succeed: {}", fe.status());

    // Data plane: denied (content-manager is NOT in the data role set).
    let data = client
        .get(format!("{}/_admin/data/models", base))
        .bearer_auth(cm)
        .send()
        .await
        .expect("data req");
    assert_eq!(data.status(), 403, "content-manager must NOT reach the data plane");
    server.stop().await;
}

#[tokio::test]
async fn admin_reaches_both_planes() {
    let server = spawn().await;
    let base = &server.base;
    let admin = &server.admin_token;
    let client = reqwest::Client::new();

    for path in ["/_admin/frontend", "/_admin/data/models"] {
        let resp = client
            .get(format!("{}{}", base, path))
            .bearer_auth(admin)
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
    server.stop().await;
}

#[tokio::test]
async fn unauthenticated_is_denied_on_both_planes() {
    let server = spawn().await;
    let base = &server.base;
    let client = reqwest::Client::new();

    for path in ["/_admin/frontend", "/_admin/data/models"] {
        let resp = client.get(format!("{}{}", base, path)).send().await.expect("req");
        assert_eq!(resp.status(), 403, "unauthenticated must be denied on {}", path);
    }
    server.stop().await;
}

#[tokio::test]
async fn concurrent_servers_keep_their_own_listeners_and_sessions() {
    let (first, second) = tokio::join!(spawn(), spawn());
    assert_ne!(first.base, second.base);
    let client = reqwest::Client::new();
    for (server, foreign_token) in [(&first, &second.admin_token), (&second, &first.admin_token)] {
        for path in ["/_admin/frontend", "/_admin/data/models"] {
            let url = format!("{}{path}", server.base);
            let own = client
                .get(&url)
                .bearer_auth(&server.admin_token)
                .send()
                .await
                .expect("own session");
            assert!(own.status().is_success());
            let foreign = client
                .get(&url)
                .bearer_auth(foreign_token)
                .send()
                .await
                .expect("foreign session");
            assert_eq!(foreign.status(), 403, "a different server's session must be denied");
        }
    }
    tokio::join!(first.stop(), second.stop());
}
