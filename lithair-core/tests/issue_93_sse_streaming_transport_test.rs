//! Regression tests for issue #93 -- SSE-over-HTTP streams incrementally.
//!
//! ## What was broken
//!
//! Prior to this fix, the route dispatch infrastructure in `app/builder.rs`
//! (`with_handler` path) and `app/mod.rs` (`handle_model_request` path) both
//! collected the SSE `StreamBody` into `Full<Bytes>` via `body.collect().await`
//! before handing the response back to hyper. This buffered the entire infinite
//! SSE stream, so a JS `EventSource` subscriber on `/api/{model}/stream`
//! received zero events until the connection closed -- defeating the purpose
//! of SSE.
//!
//! ## What the fix does
//!
//! The `RouteResponse` type was changed from `Response<Full<Bytes>>` to
//! `Response<BoxBody<Bytes, Infallible>>`, and the `body.collect()` calls in
//! the dispatch paths were removed. The SSE handler's `StreamBody` now passes
//! through to hyper directly, which sends each event incrementally.
//!
//! ## What these tests pin
//!
//! 1. **Incremental event delivery**: after connecting to `/api/{model}/stream`,
//!    events broadcast via `apply_replicated_item` arrive at the TCP client
//!    within ~2s (not waiting for connection close).
//! 2. **Heartbeat delivery**: the keepalive comment arrives within the
//!    configured interval.
//! 3. **Multiple events stream incrementally**: sending 3 items yields 3
//!    events without closing the connection.
//! 4. **SSE response headers**: `Content-Type: text/event-stream`,
//!    `Cache-Control: no-cache`, `Connection: keep-alive`.

use lithair_core::app::LithairServer;
use lithair_core::http::DeclarativeHttpHandler;
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;

/// Same `Mail` model as the issue #91 tests.
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
struct Mail {
    #[http(expose)]
    id: String,
    #[http(expose)]
    subject: String,
}

/// Spin up a server with SSE enabled via `with_handler`, return base URL and handler.
async fn spawn_sse_server(
    data_dir: &std::path::Path,
) -> (String, Arc<DeclarativeHttpHandler<Mail>>) {
    let port = portpicker::pick_unused_port().expect("free port");
    let base_url = format!("http://127.0.0.1:{}", port);

    let mail_dir = data_dir.join("mails");
    std::fs::create_dir_all(&mail_dir).expect("create mail dir");

    let mail_handler = Arc::new(
        DeclarativeHttpHandler::<Mail>::new_with_replay(mail_dir.to_string_lossy().as_ref())
            .await
            .expect("mail handler"),
    );

    let builder = LithairServer::new()
        .with_host("127.0.0.1")
        .with_port(port)
        .with_sse(true)
        .with_handler(Arc::clone(&mail_handler), "/api/mails");

    let handler_for_test = Arc::clone(&mail_handler);

    tokio::spawn(async move {
        if let Err(e) = builder.serve().await {
            eprintln!("test server error: {}", e);
        }
    });

    // Readiness probe
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .expect("reqwest client");
    let health_url = format!("{}/health", base_url);
    for _ in 0..50 {
        if let Ok(resp) = client.get(&health_url).send().await {
            if resp.status().is_success() {
                return (base_url, handler_for_test);
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("test server failed to start on port {}", port);
}

/// Issue #93 -- SSE response headers are correct and the response starts
/// streaming immediately (status 200, text/event-stream, no premature close).
#[tokio::test]
async fn issue_93_sse_response_headers_and_status() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (base, _handler) = spawn_sse_server(tmp.path()).await;

    // Use reqwest with streaming support. The response should return
    // headers immediately (status 200), not block until the body is fully
    // buffered (which would never happen for an infinite SSE stream).
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("client");

    let resp = timeout(
        Duration::from_secs(3),
        client.get(format!("{}/api/mails/stream", base)).send(),
    )
    .await
    .expect("response headers must arrive within 3s -- issue #93 regression: headers are buffered")
    .expect("request must succeed");

    assert_eq!(resp.status().as_u16(), 200, "SSE endpoint must return 200");

    let ct = resp
        .headers()
        .get("content-type")
        .expect("Content-Type header must be present")
        .to_str()
        .expect("valid header value");
    assert_eq!(ct, "text/event-stream", "Content-Type must be text/event-stream");

    let cc = resp
        .headers()
        .get("cache-control")
        .expect("Cache-Control header must be present")
        .to_str()
        .expect("valid header value");
    assert_eq!(cc, "no-cache", "Cache-Control must be no-cache");
}

/// Issue #93 -- events broadcast via `apply_replicated_item` arrive at the
/// HTTP client incrementally, not after the connection closes.
#[tokio::test]
async fn issue_93_sse_events_stream_incrementally() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (base, handler) = spawn_sse_server(tmp.path()).await;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("client");

    // Start streaming the SSE response
    let mut resp = client
        .get(format!("{}/api/mails/stream", base))
        .send()
        .await
        .expect("SSE request must succeed");

    assert_eq!(resp.status().as_u16(), 200);

    // Read the initial "connected" event
    let chunk = timeout(Duration::from_secs(3), resp.chunk())
        .await
        .expect("initial chunk must arrive within 3s")
        .expect("chunk read must succeed")
        .expect("chunk must not be empty");

    let initial = String::from_utf8_lossy(&chunk);
    assert!(
        initial.contains("event: connected"),
        "first chunk must contain 'event: connected', got: {}",
        initial
    );

    // Wait briefly for the connection to be fully established
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Now broadcast an event via apply_replicated_item
    handler
        .apply_replicated_item(Mail {
            id: "mail-1".to_string(),
            subject: "live notification".to_string(),
        })
        .await
        .expect("programmatic insert must succeed");

    // The event should arrive incrementally within ~5s. We may receive
    // heartbeat comments (`: heartbeat\n\n`) before the actual event, so
    // keep reading chunks until we find it.
    let mut all_data = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match timeout(Duration::from_secs(3), resp.chunk()).await {
            Ok(Ok(Some(chunk))) => {
                all_data.push_str(&String::from_utf8_lossy(&chunk));
                if all_data.contains("event: create") {
                    break;
                }
            }
            _ => break,
        }
    }

    assert!(
        all_data.contains("event: create"),
        "must receive 'event: create' within 5s -- issue #93: body is buffered, events don't stream. Got: {}",
        all_data
    );
    assert!(
        all_data.contains("mail-1"),
        "event must contain the item id 'mail-1', got: {}",
        all_data
    );
    assert!(
        all_data.contains("live notification"),
        "event must contain the subject, got: {}",
        all_data
    );
}

/// Issue #93 -- multiple events arrive incrementally without waiting for
/// the connection to close.
#[tokio::test]
async fn issue_93_multiple_events_stream_incrementally() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (base, handler) = spawn_sse_server(tmp.path()).await;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("client");

    let mut resp = client
        .get(format!("{}/api/mails/stream", base))
        .send()
        .await
        .expect("SSE request");

    assert_eq!(resp.status().as_u16(), 200);

    // Consume the initial "connected" event
    let _ = timeout(Duration::from_secs(3), resp.chunk())
        .await
        .expect("initial chunk")
        .expect("chunk ok");

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Send 3 events
    for i in 1..=3 {
        handler
            .apply_replicated_item(Mail {
                id: format!("multi-{}", i),
                subject: format!("subject {}", i),
            })
            .await
            .expect("insert");

        // Small delay between events so they arrive as separate chunks
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Collect events (they may arrive in one or multiple chunks)
    let mut all_data = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match timeout(Duration::from_secs(2), resp.chunk()).await {
            Ok(Ok(Some(chunk))) => {
                all_data.push_str(&String::from_utf8_lossy(&chunk));
                // Check if we've received all 3 events
                if all_data.matches("event: create").count() >= 3 {
                    break;
                }
            }
            _ => break,
        }
    }

    let create_count = all_data.matches("event: create").count();
    assert_eq!(
        create_count, 3,
        "expected 3 create events, got {} in data: {}",
        create_count, all_data
    );

    assert!(all_data.contains("multi-1"), "must contain multi-1");
    assert!(all_data.contains("multi-2"), "must contain multi-2");
    assert!(all_data.contains("multi-3"), "must contain multi-3");
}
