//! Integration test for issue #107 (phase 1) — X-Request-ID correlation.
//!
//! The serve path must attach an `X-Request-ID` header to every response:
//! - echoing a sane inbound value unchanged,
//! - minting a UUID v4 when the header is absent,
//! - replacing hostile/oversized inbound values with a fresh UUID
//!   (header-injection hygiene — never reflect unvalidated bytes).
//!
//! Modeled on tests/graceful_shutdown_test.rs: spin up a real
//! `LithairServer` on a random port via `serve_with_graceful_shutdown`,
//! exercise it with reqwest, then fire the oneshot shutdown so the test
//! never hangs.

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

#[tokio::test]
async fn response_carries_x_request_id() {
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

    // 1. A sane inbound X-Request-ID is echoed back unchanged.
    let resp = client
        .get(&health_url)
        .header("X-Request-ID", "test-correlation-id-123")
        .send()
        .await
        .expect("request with inbound id");
    let echoed = resp
        .headers()
        .get("x-request-id")
        .expect("response must carry X-Request-ID")
        .to_str()
        .expect("header value must be valid ASCII");
    assert_eq!(echoed, "test-correlation-id-123");

    // 2. Without an inbound header, a UUID v4 is generated.
    let resp = client.get(&health_url).send().await.expect("request without inbound id");
    let generated = resp
        .headers()
        .get("x-request-id")
        .expect("response must carry X-Request-ID")
        .to_str()
        .expect("header value must be valid ASCII")
        .to_string();
    assert!(!generated.is_empty(), "generated request id must be non-empty");
    let parsed = uuid::Uuid::parse_str(&generated)
        .unwrap_or_else(|e| panic!("generated id {generated:?} is not a UUID: {e}"));
    assert_eq!(parsed.get_version_num(), 4, "generated id must be UUID v4");

    // 3. An oversized inbound value (>128 chars) is NOT reflected back —
    //    a fresh UUID replaces it.
    let oversized = "z".repeat(200);
    let resp = client
        .get(&health_url)
        .header("X-Request-ID", &oversized)
        .send()
        .await
        .expect("request with oversized id");
    let replaced = resp
        .headers()
        .get("x-request-id")
        .expect("response must carry X-Request-ID")
        .to_str()
        .expect("header value must be valid ASCII");
    assert_ne!(replaced, oversized, "oversized inbound id must not be reflected");
    assert!(
        uuid::Uuid::parse_str(replaced).is_ok(),
        "replacement id {replaced:?} must be a UUID"
    );

    // 4. Error-path responses carry the header too: the 404 branch goes
    //    through the same central attach point as success responses.
    let resp = client
        .get(format!("{}/definitely/not/a/route", base_url))
        .header("X-Request-ID", "err-path-id")
        .send()
        .await
        .expect("request to unknown route");
    let on_error = resp
        .headers()
        .get("x-request-id")
        .expect("error responses must carry X-Request-ID too")
        .to_str()
        .expect("header value must be valid ASCII");
    assert_eq!(on_error, "err-path-id");

    // Clean shutdown so the test never hangs (same pattern as
    // graceful_shutdown_test.rs).
    shutdown_tx.send(()).expect("shutdown receiver still alive");
    let outcome = tokio::time::timeout(Duration::from_secs(15), serve_handle).await;
    let joined = outcome.expect("server did not shut down after signal");
    let serve_result = joined.expect("serve task panicked");
    serve_result.expect("serve_with_graceful_shutdown returned an error");
}
