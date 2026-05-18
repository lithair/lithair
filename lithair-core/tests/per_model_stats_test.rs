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
