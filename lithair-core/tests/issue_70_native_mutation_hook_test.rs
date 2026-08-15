//! Integration tests for issue #70 — native Rust hook for model mutation
//! events (`LithairServerBuilder::on_mutation`).
//!
//! ## What this pins
//!
//! - **Delivery**: a hook registered on the builder receives the
//!   `ModelChangeEvent` for a real HTTP `POST /api/{model}` — same event,
//!   same channel the SSE route consumes (single event path, second sink).
//! - **No implicit HTTP exposure**: registering a hook *without*
//!   `.with_sse(true)` keeps `GET /api/{model}/stream` at
//!   `404 SSE not enabled` — an in-process hook must not silently open the
//!   mutation stream to HTTP clients.
//! - **Write-path isolation**: a hook that panics on every event, or a hook
//!   that blocks indefinitely, never fails or stalls the HTTP write path,
//!   and never affects other hooks.
//! - **Coexistence**: `.with_sse(true)` + hook → both the broadcaster
//!   subscriber (what the SSE route consumes) and the hook see the event.
//!
//! Following docs/TESTING.md rule 1, nothing here asserts wall-clock time:
//! waiting is always a `recv()` with a deadline, and the "non-blocking"
//! property is proven by a request *completing* while the hook is provably
//! still parked, not by measuring latency.

use lithair_core::app::{LithairServer, LithairServerBuilder};
use lithair_core::http::{DeclarativeHttpHandler, HttpExposable, ModelChangeEvent};
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;

/// Blog-article shape — the concrete consumer for this feature (stela) reacts
/// to article mutations to invalidate caches / re-render pages.
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
struct Article {
    #[http(expose)]
    id: String,
    #[http(expose)]
    title: String,
}

/// Spin up a real `LithairServer` on a random port with an `Article` handler
/// mounted at `/api/articles`. `configure` lets each test attach hooks /
/// enable SSE on the builder. Returns the base URL and the handler Arc.
async fn spawn_server(
    data_dir: &std::path::Path,
    configure: impl FnOnce(LithairServerBuilder) -> LithairServerBuilder,
) -> (String, Arc<DeclarativeHttpHandler<Article>>) {
    let port = portpicker::pick_unused_port().expect("free port available");
    let base_url = format!("http://127.0.0.1:{}", port);

    let dir = data_dir.join("articles");
    std::fs::create_dir_all(&dir).expect("create article dir");

    let handler = Arc::new(
        DeclarativeHttpHandler::<Article>::new_with_replay(dir.to_string_lossy().as_ref())
            .await
            .expect("article handler"),
    );

    let builder = configure(
        LithairServer::new()
            .with_host("127.0.0.1")
            .with_port(port)
            .with_handler(Arc::clone(&handler), "/api/articles"),
    );

    let handler_for_test = Arc::clone(&handler);
    tokio::spawn(async move {
        if let Err(e) = builder.serve().await {
            eprintln!("test server error: {}", e);
        }
    });

    // Readiness probe (poll with deadline — mirrors issue_91 harness).
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

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("reqwest client")
}

async fn post_article(client: &reqwest::Client, base: &str, id: &str, title: &str) {
    let resp = client
        .post(format!("{}/api/articles", base))
        .json(&serde_json::json!({"id": id, "title": title}))
        .send()
        .await
        .expect("POST sent");
    assert!(
        resp.status().is_success(),
        "POST /api/articles must succeed, got {}",
        resp.status()
    );
}

/// Core delivery contract: HTTP POST → hook receives the create event with
/// the full item payload, without `.with_sse(true)` and therefore without
/// any HTTP/SSE round-trip. Also pins that the hook does NOT implicitly
/// expose the `/stream` HTTP route.
#[tokio::test]
async fn issue_70_hook_receives_create_event_without_sse() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ModelChangeEvent>();

    let (base, _handler) = spawn_server(tmp.path(), |b| {
        b.on_mutation(Article::http_base_path(), move |event| {
            let _ = tx.send(event);
        })
    })
    .await;

    let client = client();
    post_article(&client, &base, "a1", "hello lithair").await;

    let event = timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("hook must receive the mutation event within 2s")
        .expect("hook channel open");
    assert_eq!(event.operation, "create");
    assert_eq!(event.model_name, Article::http_base_path());
    assert_eq!(event.data["id"], "a1");
    assert_eq!(event.data["title"], "hello lithair");

    // Security gate: the broadcaster exists (hooks need it) but the HTTP
    // stream route must stay off without an explicit `.with_sse(true)`.
    let resp = client
        .get(format!("{}/api/articles/stream", base))
        .send()
        .await
        .expect("stream request sent");
    assert_eq!(
        resp.status().as_u16(),
        404,
        "on_mutation alone must NOT expose GET /api/articles/stream over HTTP"
    );
}

/// Write-path isolation: a hook that panics on every event must not fail
/// the HTTP write, kill the dispatch loop, or disturb other hooks.
#[tokio::test]
async fn issue_70_panicking_hook_isolated_from_writes_and_other_hooks() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ModelChangeEvent>();

    let (base, _handler) = spawn_server(tmp.path(), |b| {
        b.on_mutation(Article::http_base_path(), |_event| {
            panic!("hook goes boom");
        })
        .on_mutation(Article::http_base_path(), move |event| {
            let _ = tx.send(event);
        })
    })
    .await;

    let client = client();
    // Two writes: the second proves both the server AND the panicking
    // hook's dispatch loop survived the first panic (the well-behaved hook
    // keeps receiving).
    post_article(&client, &base, "p1", "first").await;
    post_article(&client, &base, "p2", "second").await;

    for expected_id in ["p1", "p2"] {
        let event = timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("well-behaved hook must keep receiving despite sibling panics")
            .expect("hook channel open");
        assert_eq!(event.operation, "create");
        assert_eq!(event.data["id"], expected_id);
    }
}

/// Non-blocking guarantee: the HTTP write completes while the hook is
/// provably still parked inside its callback. The write path only performs
/// a non-blocking broadcast send; dispatch runs on its own task.
///
/// Multi-thread flavor is required: the parked hook occupies one worker
/// thread for the duration (hooks run synchronously on their dispatch
/// task), which on the default current-thread test runtime would be the
/// *only* thread. Production servers run under `#[tokio::main]`
/// (multi-threaded), which is what this reproduces.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn issue_70_blocked_hook_does_not_block_write_path() {
    let tmp = tempfile::tempdir().expect("tmpdir");

    // The hook parks on this std channel until the test releases it.
    // (std::sync::mpsc::Receiver is !Sync, hence the Mutex.)
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let release_rx = std::sync::Mutex::new(release_rx);
    let (done_tx, mut done_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    let (base, _handler) = spawn_server(tmp.path(), |b| {
        b.on_mutation(Article::http_base_path(), move |event| {
            // Block until released — simulates a pathologically slow hook.
            let _ = release_rx.lock().expect("release rx lock").recv();
            let _ = done_tx.send(event.data["id"].as_str().unwrap_or_default().to_string());
        })
    })
    .await;

    let client = client();
    // Completes (within the client's own deadline) even though the hook has
    // not been released — this is the non-blocking property itself.
    post_article(&client, &base, "s1", "slow hook").await;

    // The hook is still parked: nothing delivered yet.
    assert!(done_rx.try_recv().is_err(), "hook must still be parked before release");

    // Release and confirm the event was delivered after all (nothing lost).
    release_tx.send(()).expect("release hook");
    let id = timeout(Duration::from_secs(2), done_rx.recv())
        .await
        .expect("hook must complete once released")
        .expect("done channel open");
    assert_eq!(id, "s1");
}

/// Coexistence: `.with_sse(true)` + hook — the SSE broadcaster channel (what
/// the `/stream` route consumes) and the native hook both observe the same
/// event, and the `/stream` HTTP route stays exposed.
#[tokio::test]
async fn issue_70_hook_and_sse_coexist() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (tx, mut hook_rx) = tokio::sync::mpsc::unbounded_channel::<ModelChangeEvent>();

    let (base, handler) = spawn_server(tmp.path(), |b| {
        b.with_sse(true).on_mutation(Article::http_base_path(), move |event| {
            let _ = tx.send(event);
        })
    })
    .await;

    // Subscribe on the same broadcaster the SSE route serves from.
    let broadcaster = handler
        .sse_broadcaster()
        .cloned()
        .expect("with_sse(true) must wire the broadcaster onto the handler");
    let mut sse_rx = broadcaster.subscribe(Article::http_base_path()).await;

    let client = client();
    post_article(&client, &base, "c1", "both sinks").await;

    let sse_event = timeout(Duration::from_secs(2), sse_rx.recv())
        .await
        .expect("SSE subscriber must receive the event")
        .expect("broadcast channel open");
    let hook_event = timeout(Duration::from_secs(2), hook_rx.recv())
        .await
        .expect("hook must receive the event")
        .expect("hook channel open");
    assert_eq!(sse_event.data["id"], "c1");
    assert_eq!(hook_event.data["id"], "c1");
    assert_eq!(sse_event.operation, hook_event.operation);

    // With explicit with_sse(true) the HTTP stream route stays available
    // (not 404) — issue_91's timeout technique: a streaming response never
    // completes body collection, so a timeout is the success signal.
    let short_client = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
        .expect("client");
    match short_client.get(format!("{}/api/articles/stream", base)).send().await {
        Ok(resp) => assert_ne!(
            resp.status().as_u16(),
            404,
            "with_sse(true) + hook must keep the /stream route exposed"
        ),
        Err(e) if e.is_timeout() => {} // streaming path reached — expected
        Err(other) => panic!("unexpected request error: {other}"),
    }
}
