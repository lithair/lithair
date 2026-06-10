//! Integration test for issue #107 (phase 2) — opt-in OpenTelemetry export.
//!
//! Compiled only with `--features otel`. CI builds default features only, so
//! `cargo test -p lithair-core --features otel` run locally is the gate for
//! this file.
//!
//! What this proves: with `LT_OTEL_ENDPOINT` pointing at an UNREACHABLE
//! collector, the otel layer still composes into the default tracing stack
//! without panicking, the server serves traffic normally (export is
//! fail-open by design), and graceful shutdown — including the bounded
//! exporter flush — completes within its timeout instead of hanging on the
//! dead endpoint.
//!
//! What this deliberately does NOT prove: an end-to-end span round-trip into
//! a real OTLP collector. CI has no collector container, so asserting on
//! received spans is out of scope here; the docker-compose Jaeger recipe in
//! docs/operations/observability.md is the manual verification path.
//!
//! Single test on purpose: `init_default_tracing()` installs process-global
//! state (subscriber + provider OnceLock) with first-wins semantics, so a
//! second test in this binary could not exercise a different
//! `LT_OTEL_ENDPOINT` value anyway.
//!
//! Modeled on tests/request_id_test.rs / tests/graceful_shutdown_test.rs:
//! real `LithairServer` on a random port, oneshot-driven shutdown.

#![cfg(feature = "otel")]

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn otel_layer_with_unreachable_endpoint_does_not_break_serving() {
    // Point the exporter at a well-formed but unreachable endpoint: nothing
    // listens on the TCP discard port. The tonic channel is lazy, so init
    // must succeed; per-batch export failures must stay invisible to request
    // handling. Set BEFORE the server starts — init_default_tracing() reads
    // the env at serve() time.
    std::env::set_var("LT_OTEL_ENDPOINT", "http://127.0.0.1:9");
    std::env::set_var("LT_OTEL_SERVICE_NAME", "lithair-otel-test");

    let port = portpicker::pick_unused_port().expect("free port available");
    let base_url = format!("http://127.0.0.1:{}", port);

    let tmp = tempfile::tempdir().expect("tmpdir");
    let widget_dir = tmp.path().join("widgets");
    std::fs::create_dir_all(&widget_dir).expect("create widget dir");

    let server = LithairServer::new()
        .with_host("127.0.0.1")
        .with_port(port)
        .with_model::<Widget>(widget_dir.to_string_lossy().to_string(), "/api/widgets");

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let shutdown = async move {
        let _ = shutdown_rx.await;
    };

    let serve_handle =
        tokio::spawn(async move { server.serve_with_graceful_shutdown(shutdown).await });

    // Readiness via the canonical /health probe. If otel init panicked or
    // blocked startup, this loop is what fails.
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
    assert!(ready, "server failed to start with otel layer enabled on port {}", port);

    // The server must serve normally while the exporter fails in the
    // background: exact /health body, and the http_request span path (which
    // now also feeds the otel layer) must not disturb the response.
    let resp = client.get(&health_url).send().await.expect("/health request");
    assert!(resp.status().is_success());
    let body = resp.text().await.expect("/health body");
    assert_eq!(body, r#"{"status":"healthy"}"#);

    // Graceful shutdown must complete despite the unreachable collector:
    // the flush in shutdown_otel_provider is bounded (SDK-internal 5s cap
    // plus our 6s outer timeout), on top of the 5s connection-drain grace.
    shutdown_tx.send(()).expect("shutdown receiver still alive");
    let outcome = tokio::time::timeout(Duration::from_secs(20), serve_handle).await;
    let joined = outcome.expect("shutdown (incl. bounded otel flush) did not finish in time");
    let serve_result = joined.expect("serve task panicked");
    serve_result.expect("serve_with_graceful_shutdown returned an error");
}
