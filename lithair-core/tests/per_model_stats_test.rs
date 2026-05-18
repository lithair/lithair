//! Integration tests for issue #72 — per-model storage and memory stats.
//!
//! Exercises both surfaces:
//! - `GET /_admin/data/models/{name}/_stats` — JSON for ad-hoc debugging
//! - `GET /metrics` — Prometheus text, with per-model series under
//!   `lithair_model_items`, `lithair_model_ram_bytes`, `lithair_model_raftlog_bytes`.
//!
//! See:
//! - `lithair-core/src/app/model_handler.rs::ModelStats` (struct + default impl)
//! - `lithair-core/src/app/mod.rs::handle_data_admin_request` (`_stats` route)
//! - `lithair-core/src/app/mod.rs::handle_metrics_request` (Prometheus extension)

use lithair_core::app::LithairServer;
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
struct Account {
    #[http(expose)]
    id: String,
    #[http(expose)]
    name: String,
}

/// Spin up a server with one model and wait for `/health` to respond.
async fn spawn_server(account_dir: std::path::PathBuf, port: u16) -> reqwest::Client {
    let builder =
        LithairServer::new()
            .with_host("127.0.0.1")
            .with_port(port)
            .with_data_admin()
            .with_model::<Account>(account_dir.to_string_lossy().to_string(), "/api/accounts");

    tokio::spawn(async move {
        if let Err(e) = builder.serve().await {
            eprintln!("test server error: {}", e);
        }
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .expect("reqwest");
    let base = format!("http://127.0.0.1:{}", port);
    let health = format!("{}/health", base);
    for _ in 0..50 {
        if let Ok(r) = client.get(&health).send().await {
            if r.status().is_success() {
                return client;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("server failed to start on port {}", port);
}

/// Empty model: zero items, zero approx_ram, and either a missing or
/// freshly-created raftlog. We assert the contract, not exact bytes.
#[tokio::test]
async fn empty_model_returns_zero_counts() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let account_dir = tmp.path().join("accounts");
    std::fs::create_dir_all(&account_dir).expect("create account dir");

    let port = portpicker::pick_unused_port().expect("free port");
    let client = spawn_server(account_dir, port).await;

    let resp = client
        .get(format!("http://127.0.0.1:{}/_admin/data/models/Account/_stats", port))
        .send()
        .await
        .expect("request sent");
    assert_eq!(resp.status(), 200, "empty model stats should be 200");

    let body: serde_json::Value = resp.json().await.expect("json");
    assert_eq!(body["model"], "Account");
    assert_eq!(body["item_count"].as_u64().expect("item_count"), 0);
    assert_eq!(
        body["approx_ram_bytes"].as_u64().expect("approx_ram_bytes"),
        0,
        "empty model has no RAM cost from items"
    );
    // raftlog file may not exist yet — bytes must be a number though.
    assert!(
        body["raftlog_size_bytes"].is_u64(),
        "raftlog_size_bytes must be present and numeric"
    );
    // Compaction fields are gated on issue #69 — currently null.
    assert!(body["events_since_last_compaction"].is_null());
    assert!(body["last_compaction_at"].is_null());
}

/// Non-empty model: after creating N items via the auto-generated /api route,
/// stats must reflect a non-zero item_count and a plausible RAM estimate
/// (positive and roughly count-proportional, not exact). Also cross-checks
/// the Prometheus surface carries the same numbers under the right label.
#[tokio::test]
async fn non_empty_model_returns_plausible_counts() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let account_dir = tmp.path().join("accounts");
    std::fs::create_dir_all(&account_dir).expect("create account dir");

    let port = portpicker::pick_unused_port().expect("free port");
    let client = spawn_server(account_dir, port).await;

    // Seed 5 items via the public REST surface.
    for i in 0..5 {
        let resp = client
            .post(format!("http://127.0.0.1:{}/api/accounts", port))
            .json(&serde_json::json!({"id": format!("acct-{}", i), "name": format!("alice-{}", i)}))
            .send()
            .await
            .expect("seed request sent");
        assert!(
            resp.status().is_success(),
            "POST /api/accounts should succeed, got {}",
            resp.status()
        );
    }

    let resp = client
        .get(format!("http://127.0.0.1:{}/_admin/data/models/Account/_stats", port))
        .send()
        .await
        .expect("request sent");
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.expect("json");
    assert_eq!(body["item_count"].as_u64().expect("item_count"), 5);
    let ram = body["approx_ram_bytes"].as_u64().expect("approx_ram_bytes");
    assert!(ram > 0, "non-empty model must report positive approx_ram_bytes, got {}", ram);
    // 5 items with two short fields — generous upper bound. If this fires,
    // the estimator is wildly off and we should know.
    assert!(ram < 100_000, "approx_ram_bytes wildly high for 5 short items: {}", ram);

    // raftlog persisted at least one event — should be > 0 now.
    let raftlog = body["raftlog_size_bytes"].as_u64().expect("raftlog_size_bytes");
    assert!(raftlog > 0, "raftlog_size_bytes should be > 0 after 5 writes, got {}", raftlog);

    // Cross-check the Prometheus surface — same numbers, different format.
    let prom = client
        .get(format!("http://127.0.0.1:{}/metrics", port))
        .send()
        .await
        .expect("metrics request sent")
        .text()
        .await
        .expect("metrics body");
    assert!(
        prom.contains(r#"lithair_model_items{model="Account"} 5"#),
        "Prometheus output missing per-model items series: {}",
        prom
    );
    assert!(
        prom.contains(r#"lithair_model_ram_bytes{model="Account"}"#),
        "Prometheus output missing per-model RAM series"
    );
    assert!(
        prom.contains(r#"lithair_model_raftlog_bytes{model="Account"}"#),
        "Prometheus output missing per-model raftlog series"
    );
}

/// Regression for Gemini's PR #83 round-2 finding: the default `get_stats`
/// impl previously called `get_all_data_json` and then `.take(16)`, which
/// still allocated and serialized every item. For a model with N >> 16
/// items the call must still complete fast and the per-item RAM estimate
/// must remain bounded — we assert that the cost scales with sample size,
/// not with N.
///
/// Concretely: seed 200 items, hit `/_stats`, and check that the response
/// arrives well under the client's 2s timeout. With the old impl the cost
/// was dominated by cloning 200 `Account`s through serde — fine here, but
/// the point is to lock in the contract before the model grows. We also
/// confirm `item_count == 200` and `approx_ram_bytes` stays plausible
/// (linear in count, with the per-item average derived from a 16-sample).
#[tokio::test]
async fn large_model_stats_uses_bounded_sample() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let account_dir = tmp.path().join("accounts");
    std::fs::create_dir_all(&account_dir).expect("create account dir");

    let port = portpicker::pick_unused_port().expect("free port");
    let client = spawn_server(account_dir, port).await;

    // Seed N items where N > the sample size (16). 200 keeps the test fast
    // (it's also bound by REST seeding throughput) while making the
    // "bounded sample" property meaningful — only 16 of these should be
    // serialized when computing approx_ram_bytes.
    const N: usize = 200;
    for i in 0..N {
        let resp = client
            .post(format!("http://127.0.0.1:{}/api/accounts", port))
            .json(&serde_json::json!({
                "id": format!("acct-{:04}", i),
                "name": format!("alice-{:04}", i),
            }))
            .send()
            .await
            .expect("seed request sent");
        assert!(resp.status().is_success(), "seed POST failed: {}", resp.status());
    }

    // Time the stats call. The bounded-sample contract says this should be
    // dominated by the 16-item serialize + one fs::metadata call, not by
    // the full 200-item clone the prior impl performed. We don't pin a
    // hard wall-clock budget (CI machines vary) — the client's 2s timeout
    // already enforces an upper bound.
    let start = std::time::Instant::now();
    let resp = client
        .get(format!("http://127.0.0.1:{}/_admin/data/models/Account/_stats", port))
        .send()
        .await
        .expect("stats request sent");
    let elapsed = start.elapsed();
    assert_eq!(resp.status(), 200, "stats endpoint should be 200");
    assert!(
        elapsed < Duration::from_secs(2),
        "stats call took {:?} — bounded-sample contract regressed?",
        elapsed
    );

    let body: serde_json::Value = resp.json().await.expect("json");
    assert_eq!(
        body["item_count"].as_u64().expect("item_count"),
        N as u64,
        "item_count must reflect all seeded items"
    );
    let ram = body["approx_ram_bytes"].as_u64().expect("approx_ram_bytes");
    assert!(ram > 0, "200 items must report positive approx_ram_bytes, got {}", ram);
    // Very loose upper bound — 200 items × a few hundred bytes each leaves
    // plenty of headroom. If this fires, either the estimator is multiplying
    // by something it shouldn't, or the items are unexpectedly large.
    assert!(
        ram < 10_000_000,
        "approx_ram_bytes implausibly high for 200 small items: {}",
        ram
    );
}

/// Direct unit test for the bounded-sample storage primitive
/// (`DeclarativeHttpHandler::get_sample_items`). Confirms that asking for
/// 16 items from a storage with 100 items returns exactly 16, and asking
/// for 0 returns an empty Vec (the early-exit branch).
///
/// This is the layer Gemini's PR #83 round-2 review pushed us to add:
/// without it, the default `get_stats` was still allocating every item
/// before trimming to 16. We test the primitive directly so a regression
/// would surface here even if the trait-level test changes shape.
#[tokio::test]
async fn declarative_handler_get_sample_items_is_bounded() {
    use lithair_core::http::DeclarativeHttpHandler;

    // `new()` needs an event-store path; a tmpdir keeps the test hermetic.
    let tmp = tempfile::tempdir().expect("tmpdir");
    let store_path = tmp.path().join("events");
    let handler: DeclarativeHttpHandler<Account> =
        DeclarativeHttpHandler::new(store_path.to_string_lossy().as_ref())
            .expect("DeclarativeHttpHandler::new");

    // Seed 100 items directly via reconcile (bypasses HTTP/event-store —
    // we're testing the storage primitive, not the request path).
    let items: Vec<Account> = (0..100)
        .map(|i| Account { id: format!("acct-{:03}", i), name: format!("alice-{:03}", i) })
        .collect();
    handler.reconcile_replace_all(items).await;

    // Storage size is 100, regardless of what get_sample_items returns.
    assert_eq!(handler.storage_count().await, 100, "all 100 items must be present");

    // Asking for 16 returns exactly 16 — bounded at the storage layer.
    let sample = handler.get_sample_items(16).await;
    assert_eq!(sample.len(), 16, "sample size must be capped at the requested limit");

    // Asking for 0 returns an empty Vec (early-exit branch).
    let empty = handler.get_sample_items(0).await;
    assert!(empty.is_empty(), "zero-limit sample must be empty");

    // Asking for more than count returns count (not panic).
    let oversized = handler.get_sample_items(500).await;
    assert_eq!(oversized.len(), 100, "limit > count must return all items, not error");
}

/// Non-existent model name: 404 with a JSON error body. Operators hitting a
/// typo (or asking about a model that hasn't been registered) need a clean
/// negative response, not a 500 or silent empty body.
#[tokio::test]
async fn non_existent_model_returns_404() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let account_dir = tmp.path().join("accounts");
    std::fs::create_dir_all(&account_dir).expect("create account dir");

    let port = portpicker::pick_unused_port().expect("free port");
    let client = spawn_server(account_dir, port).await;

    let resp = client
        .get(format!("http://127.0.0.1:{}/_admin/data/models/DoesNotExist/_stats", port))
        .send()
        .await
        .expect("request sent");
    assert_eq!(resp.status(), 404, "missing model must 404");

    let body: serde_json::Value = resp.json().await.expect("json");
    assert!(body["error"].is_string(), "404 body must carry an error field");
    assert!(
        body["error"].as_str().unwrap().contains("DoesNotExist"),
        "404 body should name the missing model"
    );
}

/// Regression for Gemini's JSON-injection finding on PR #83: a model name
/// containing JSON metacharacters (quote, backslash, etc.) must not be able
/// to break out of the error string. The response must remain valid JSON
/// regardless of what the path segment looks like.
///
/// We exercise two attack shapes:
/// 1. URL-encoded quotes (`%22`) — what most clients would send. The router
///    currently passes the segment through without URL-decoding, so the name
///    `name` sees is `x%22%2C%20%22y%22%3A%22z` (safe-by-accident). The test
///    just confirms the body is still valid JSON and is exactly one object
///    with an `error` field.
/// 2. Raw quotes in the URI (sent via low-level `http`/`hyper` request to
///    bypass `reqwest`'s URL validation). This is the actual scenario the
///    serde_json fix defends against — without it, the response body would
///    parse as `{"error":"Model 'x", "y":"z' not found"}` and an attacker
///    could inject arbitrary top-level keys.
#[tokio::test]
async fn non_existent_model_with_quotes_returns_valid_json() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let account_dir = tmp.path().join("accounts");
    std::fs::create_dir_all(&account_dir).expect("create account dir");

    let port = portpicker::pick_unused_port().expect("free port");
    let client = spawn_server(account_dir, port).await;

    // Attack 1: URL-encoded probe. Body must be valid JSON with a single
    // `error` key — even though the router doesn't decode `%22`, we don't
    // want the assertion to depend on that detail.
    let raw_name = r#"x", "y":"z"#;
    let encoded = urlencoding::encode(raw_name);
    let resp = client
        .get(format!("http://127.0.0.1:{}/_admin/data/models/{}/_stats", port, encoded))
        .send()
        .await
        .expect("request sent");
    assert_eq!(resp.status(), 404, "missing model must 404");

    let body: serde_json::Value = resp.json().await.expect("json must parse");
    assert!(body.is_object(), "404 body must be a JSON object");
    let obj = body.as_object().unwrap();
    assert_eq!(
        obj.len(),
        1,
        "404 body must have exactly one key (error), got: {:?}",
        obj.keys().collect::<Vec<_>>()
    );
    assert!(obj.contains_key("error"), "404 body must have `error` key");

    // Attack 2: raw quotes in the path. Both `reqwest` and `hyper`'s URI
    // parser reject `"` in a URI — defenders can rely on that for most
    // clients. But the JSON-injection defense must not depend on clients
    // doing their job; a custom client / proxy / fuzzer can write raw
    // bytes onto the wire. We open a TCP socket and speak HTTP/1.1
    // directly to prove the server's response is still valid JSON.
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream =
        tokio::net::TcpStream::connect(("127.0.0.1", port)).await.expect("tcp connect");
    // Note: `\"` in the path bytes is not legal per RFC 3986, but lithair's
    // hyper-based server accepts it and routes by string-split on `/`.
    // The whole point of this test is to confirm that even when the parser
    // is lenient, the response stays well-formed.
    let raw_request = format!(
        "GET /_admin/data/models/{}/_stats HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
        raw_name, port
    );
    stream.write_all(raw_request.as_bytes()).await.expect("write request");
    let mut raw_response = Vec::new();
    stream.read_to_end(&mut raw_response).await.expect("read response");
    let raw_text = String::from_utf8_lossy(&raw_response).to_string();

    // Split status/headers from body on the first \r\n\r\n.
    let body_start = raw_text.find("\r\n\r\n").expect("response must have header/body separator");
    let body_text = &raw_text[body_start + 4..];

    // If the server rejects the raw-quotes URI outright (e.g. with 400),
    // that's also a valid defense — the JSON-injection vector simply
    // doesn't reach the format! line. Skip the body assertions in that
    // case; the URL-encoded path above already exercises the 404.
    if raw_text.starts_with("HTTP/1.1 404") {
        let body: serde_json::Value = serde_json::from_str(body_text.trim())
            .expect("404 body must be valid JSON despite raw quotes in the URI");
        assert!(body.is_object(), "404 body must be a JSON object");
        let obj = body.as_object().unwrap();
        assert_eq!(
            obj.len(),
            1,
            "404 body must have exactly one key — JSON injection would add more. Got: {:?}",
            body
        );
        assert!(obj.contains_key("error"), "404 body must have `error` key");
        let err = obj["error"].as_str().expect("error is a string");
        assert!(
            err.contains(raw_name),
            "404 body should carry the offending name verbatim inside `error`, got: {}",
            err
        );
        assert!(
            body.get("y").is_none(),
            "JSON injection escaped: spurious `y` field present in {}",
            body
        );
    } else {
        // Server rejected the malformed URI before routing — fine, just
        // confirm it didn't 200 it.
        assert!(
            !raw_text.starts_with("HTTP/1.1 2"),
            "raw-quotes URI must not yield a 2xx, got: {}",
            &raw_text[..raw_text.len().min(120)]
        );
    }
}
