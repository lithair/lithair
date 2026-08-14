//! Integration test for issue #33 — opt-in strict host routing (421).
//!
//! Spins up two real `LithairServer` instances, each with a single vhost
//! frontend and no host-agnostic frontend:
//!
//!   - default mode: unknown `Host:` falls through and ends as 404
//!     (backward-compatible behaviour from PR #31);
//!   - `.strict_host_routing()`: unknown `Host:` is answered with
//!     `421 Misdirected Request`, while a known host is served exactly
//!     as before (the flag only affects lookup misses).
//!
//! Server-spawn pattern mirrors `frontend_admin_api_test.rs` (real server,
//! random port, `serve_with_graceful_shutdown` + oneshot, CWD guard for
//! the frontend event store).

use lithair_core::app::LithairServer;
use std::time::Duration;

struct CwdGuard(std::path::PathBuf);
impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.0);
    }
}

/// Wait until the server answers *anything* on `/`. In strict mode even
/// `/health` returns 421 for an unregistered probe host, so readiness is
/// "any HTTP response", not "2xx".
async fn wait_until_up(client: &reqwest::Client, base_url: &str) {
    for _ in 0..50 {
        if client.get(base_url).send().await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("test server failed to start at {}", base_url);
}

async fn spawn_server(
    port: u16,
    site_dir: String,
    strict: bool,
) -> (tokio::sync::oneshot::Sender<()>, tokio::task::JoinHandle<anyhow::Result<()>>) {
    let mut server = LithairServer::new()
        .with_host("127.0.0.1")
        .with_port(port)
        .with_vhost("site.test", move |v| v.with_frontend_at("/", site_dir.clone()));
    if strict {
        server = server.strict_host_routing();
    }

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let shutdown = async move {
        let _ = shutdown_rx.await;
    };
    let handle = tokio::spawn(async move { server.serve_with_graceful_shutdown(shutdown).await });
    (shutdown_tx, handle)
}

async fn shutdown_server(
    tx: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<anyhow::Result<()>>,
) {
    tx.send(()).expect("shutdown receiver alive");
    let joined = tokio::time::timeout(Duration::from_secs(15), handle)
        .await
        .expect("server did not return after shutdown signal");
    joined.expect("serve task panicked").expect("serve returned an error");
}

#[tokio::test]
async fn strict_host_routing_returns_421_only_when_opted_in() {
    // Single test fn: the CWD guard is process-global, so both servers
    // (default-mode and strict) run sequentially inside it.
    let tmp = tempfile::tempdir().expect("tmpdir");
    let site_dir = tmp.path().join("site");
    std::fs::create_dir_all(&site_dir).expect("create site dir");
    std::fs::write(site_dir.join("index.html"), b"SITE-CONTENT").expect("write index");

    // FrontendEngine persists its event store under ./data/frontend;
    // isolate it to the temp dir (restored on drop, panic or not).
    let _cwd_guard = CwdGuard(std::env::current_dir().expect("cwd"));
    std::env::set_current_dir(tmp.path()).expect("chdir tmp");
    let site_dir_s = site_dir.to_string_lossy().to_string();

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("reqwest client");

    // ── Default mode: unknown host falls through to 404 (unchanged). ──
    let port = portpicker::pick_unused_port().expect("free port");
    let base_url = format!("http://127.0.0.1:{}", port);
    let (tx, handle) = spawn_server(port, site_dir_s.clone(), false).await;
    wait_until_up(&client, &base_url).await;

    let known = client
        .get(&base_url)
        .header("Host", "site.test")
        .send()
        .await
        .expect("known-host request");
    assert_eq!(known.status(), 200, "known host is served in default mode");
    assert_eq!(known.text().await.expect("body"), "SITE-CONTENT");

    let unknown = client
        .get(&base_url)
        .header("Host", "unknown.test")
        .send()
        .await
        .expect("unknown-host request");
    assert_eq!(
        unknown.status(),
        404,
        "default mode: unknown host falls through to 404 (backward compat)"
    );

    shutdown_server(tx, handle).await;

    // ── Strict mode: unknown host -> 421, known host unaffected. ──
    let port = portpicker::pick_unused_port().expect("free port");
    let base_url = format!("http://127.0.0.1:{}", port);
    let (tx, handle) = spawn_server(port, site_dir_s, true).await;
    wait_until_up(&client, &base_url).await;

    let unknown = client
        .get(&base_url)
        .header("Host", "unknown.test")
        .send()
        .await
        .expect("unknown-host request");
    assert_eq!(unknown.status(), 421, "strict mode: unknown host gets 421 Misdirected Request");

    // Known host is insensitive to the flag: frontend still served...
    let known = client
        .get(&base_url)
        .header("Host", "site.test")
        .send()
        .await
        .expect("known-host request");
    assert_eq!(known.status(), 200, "strict mode: known host still served");
    assert_eq!(known.text().await.expect("body"), "SITE-CONTENT");

    // ...and the rest of the pipeline (ops endpoints) still runs for it.
    let health = client
        .get(format!("{}/health", base_url))
        .header("Host", "site.test")
        .send()
        .await
        .expect("health request");
    assert_eq!(health.status(), 200, "strict mode: pipeline continues for known hosts");

    // The strict gate covers the whole pipeline: even /health is 421 for
    // an unregistered host (documented in the builder rustdoc).
    let health_unknown = client
        .get(format!("{}/health", base_url))
        .header("Host", "unknown.test")
        .send()
        .await
        .expect("health unknown-host request");
    assert_eq!(health_unknown.status(), 421, "strict mode gates ops endpoints too");

    shutdown_server(tx, handle).await;
}
