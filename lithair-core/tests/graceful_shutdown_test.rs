//! Integration test for issue #112 — `serve_with_graceful_shutdown` must
//! actually return once the supplied shutdown future resolves.
//!
//! `serve()` (and historically the whole accept path) runs an unconditional
//! infinite loop and never returns; the new hook adds a `tokio::select!`
//! against a caller-supplied future. This test spins up a real
//! `LithairServer` on a random port, confirms it serves requests, fires a
//! `oneshot` to resolve the shutdown future, and asserts the
//! `serve_with_graceful_shutdown` task completes within a timeout.
//!
//! Without the fix the loop would never break and this test would hang until
//! the outer `tokio::time::timeout` fired — i.e. the `.is_ok()` assertion on
//! the timeout result is precisely what would fail.

use lithair_core::app::LithairServer;
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
struct Widget {
    #[http(expose)]
    id: String,
    #[http(expose)]
    name: String,
}

/// The accept loop must stop and the future must resolve to `Ok(())` once the
/// shutdown signal fires.
#[tokio::test]
async fn serve_with_graceful_shutdown_returns_on_signal() {
    let port = portpicker::pick_unused_port().expect("free port available");
    let base_url = format!("http://127.0.0.1:{}", port);

    let tmp = tempfile::tempdir().expect("tmpdir");
    let widget_dir = tmp.path().join("widgets");
    std::fs::create_dir_all(&widget_dir).expect("create widget dir");

    let server = LithairServer::new()
        .with_host("127.0.0.1")
        .with_port(port)
        .with_model::<Widget>(widget_dir.to_string_lossy().to_string(), "/api/widgets");

    // Drive the shutdown future via a oneshot. The future resolves to `()`
    // when the sender fires (or is dropped) — exactly the `Output = ()`
    // contract `serve_with_graceful_shutdown` requires.
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let shutdown = async move {
        let _ = shutdown_rx.await;
    };

    let serve_handle =
        tokio::spawn(async move { server.serve_with_graceful_shutdown(shutdown).await });

    // Wait for readiness via the canonical /health probe.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .expect("reqwest client");
    let health_url = format!("{}/health", base_url);
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
    assert!(ready, "test server failed to start on port {}", port);

    // Fire the shutdown signal.
    shutdown_tx.send(()).expect("shutdown receiver still alive");

    // The server must return Ok within the grace window plus headroom. The
    // drain grace defaults to 5s; allow generous slack for the join.
    // Without the fix this `timeout` would elapse → Err.
    let outcome = tokio::time::timeout(Duration::from_secs(15), serve_handle).await;

    let joined =
        outcome.expect("serve_with_graceful_shutdown did not return after shutdown signal");
    let serve_result = joined.expect("serve task panicked");
    serve_result.expect("serve_with_graceful_shutdown returned an error");
}

/// Issue #115 — graceful shutdown must drain Lithair's OWN background tasks
/// (per-model WAL flusher, auto-compaction) and precisely join per-connection
/// tasks, leaving zero orphan tasks on the runtime after
/// `serve_with_graceful_shutdown` returns.
///
/// The assertion is the *property* (alive-task count back to baseline,
/// polled with a deadline — see docs/TESTING.md rule 1), not wall-clock
/// time. Before the fix, the flusher held a strong `Arc` to its store and
/// the auto-compaction loop had no shutdown branch, so both ran forever and
/// the poll deadline below elapsed.
#[tokio::test]
async fn graceful_shutdown_leaves_no_orphan_tasks() {
    let metrics = tokio::runtime::Handle::current().metrics();
    let baseline = metrics.num_alive_tasks();

    let port = portpicker::pick_unused_port().expect("free port available");
    let base_url = format!("http://127.0.0.1:{}", port);

    let tmp = tempfile::tempdir().expect("tmpdir");
    let widget_dir = tmp.path().join("widgets");
    std::fs::create_dir_all(&widget_dir).expect("create widget dir");

    let server = LithairServer::new()
        .with_host("127.0.0.1")
        .with_port(port)
        .with_model::<Widget>(widget_dir.to_string_lossy().to_string(), "/api/widgets")
        // Long interval: the compaction task sits waiting on its ticker, so
        // only the shutdown signal (not a tick) can end it — exactly the
        // orphan scenario #115 describes.
        .with_auto_compaction(10_000, Duration::from_secs(3600))
        // Exercise the configurable grace (issue #115) with a small value so
        // even the abort path would stay well inside the test timeout.
        .with_shutdown_grace(Duration::from_secs(3));

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let shutdown = async move {
        let _ = shutdown_rx.await;
    };
    let serve_handle =
        tokio::spawn(async move { server.serve_with_graceful_shutdown(shutdown).await });

    // Readiness probe. The client keeps the connection pooled (keep-alive),
    // so at shutdown time the server has a live idle connection to drain —
    // hyper's per-connection `graceful_shutdown` must close it promptly
    // instead of letting keep-alive outlive the grace window.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .expect("reqwest client");
    let health_url = format!("{}/health", base_url);
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
    assert!(ready, "test server failed to start on port {}", port);

    shutdown_tx.send(()).expect("shutdown receiver still alive");

    let outcome = tokio::time::timeout(Duration::from_secs(15), serve_handle)
        .await
        .expect("serve_with_graceful_shutdown did not return after shutdown signal")
        .expect("serve task panicked");
    outcome.expect("serve_with_graceful_shutdown returned an error");

    // Drop the client so its own pool tasks wind down too, then poll the
    // runtime's alive-task count back to the pre-server baseline. Each
    // #[tokio::test] owns its runtime, so leftover tasks here can only be
    // ours (server internals) or the dropped client's. The WAL flusher is
    // Weak-tied to its store and exits within one flush interval (100ms) of
    // the server dropping; everything else is joined before serve returns.
    drop(client);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let alive = metrics.num_alive_tasks();
        if alive <= baseline {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "orphan task(s) after graceful shutdown: {} alive vs baseline {}",
            alive,
            baseline
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
