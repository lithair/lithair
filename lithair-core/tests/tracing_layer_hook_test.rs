//! `with_tracing_layer` (the log-provider extension point) + the
//! `LT_LOG_LEVEL` fallback, proven together in one boot.
//!
//! Own integration-test FILE on purpose: each file is a separate test
//! process, so Lithair's global subscriber install (first-wins) is
//! guaranteed to be ours — the 400+ lib tests can't have claimed it.
//!
//! The custom layer counts every event it receives. The server boots with
//! RUST_LOG unset and LT_LOG_LEVEL=info: startup emits info-level lifecycle
//! lines, so a non-zero count proves BOTH that the user layer is composed
//! into the subscriber AND that LT_LOG_LEVEL (previously read-but-never-
//! applied) now actually drives the filter — under the historical `error`
//! fallback those events would never reach the layer.

use lithair_core::app::LithairServer;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
struct CountingLayer(Arc<AtomicUsize>);

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for CountingLayer {
    fn on_event(
        &self,
        _event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[tokio::test]
async fn custom_layer_receives_events_under_lt_log_level() {
    // Env must be set before serve() runs init_default_tracing. Safe here:
    // this file is its own process and this is its only test.
    std::env::remove_var("RUST_LOG");
    std::env::set_var("LT_LOG_LEVEL", "info");

    let count = Arc::new(AtomicUsize::new(0));
    let port = portpicker::pick_unused_port().expect("free port");
    let server = LithairServer::new()
        .with_host("127.0.0.1")
        .with_port(port)
        .with_tracing_layer(CountingLayer(count.clone()));

    tokio::spawn(async move {
        if let Err(e) = server.serve().await {
            eprintln!("test server error: {}", e);
        }
    });

    // Wait for readiness (startup logs info lines through the subscriber).
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .expect("client");
    let health = format!("http://127.0.0.1:{}/health", port);
    let mut ready = false;
    for _ in 0..50 {
        if let Ok(r) = client.get(&health).send().await {
            if r.status().is_success() {
                ready = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(ready, "server failed to start on {}", port);

    let seen = count.load(Ordering::Relaxed);
    assert!(
        seen > 0,
        "the custom tracing layer must receive events (got {seen}); either \
         with_tracing_layer is not composed into the subscriber, or \
         LT_LOG_LEVEL no longer drives the filter"
    );
}
