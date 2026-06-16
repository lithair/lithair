//! Logical backup round-trip (issue #133, part 2 — Strategy A).
//!
//! Exercises the documented logical export path: a live server with
//! `with_data_admin()` exposes `POST /_admin/data/backup`, which walks every
//! registered model and returns one JSON document. This test asserts the
//! export shape and that it contains the actual records.
//!
//! Two restore paths are covered:
//!   - `logical_backup_exports_records_and_manual_restore_loads_them` — the
//!     original operator procedure: re-POST each exported record to the
//!     model's write endpoint (works with no special endpoint).
//!   - `logical_import_endpoint_restores_a_whole_backup_in_one_call` — the
//!     `POST /_admin/data/import` endpoint (issue #37): feed the backup JSON
//!     back in one call. Also asserts idempotency (re-import does not
//!     duplicate, keyed by `id`) and the 207 partial-success path for an
//!     unknown model.

use lithair_core::app::LithairServer;
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, DeclarativeModel)]
struct Article {
    #[http(expose)]
    id: String,
    #[http(expose)]
    title: String,
    #[http(expose)]
    body: String,
    #[http(expose)]
    views: i64,
}

/// Spin up a server with one Article model + data admin, wait for /health.
async fn spawn_server(dir: std::path::PathBuf, port: u16) -> reqwest::Client {
    let builder = LithairServer::new()
        .with_host("127.0.0.1")
        .with_port(port)
        .with_data_admin()
        .with_model::<Article>(dir.to_string_lossy().to_string(), "/api/articles");

    tokio::spawn(async move {
        if let Err(e) = builder.serve().await {
            eprintln!("test server error: {}", e);
        }
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .expect("reqwest client");
    let health = format!("http://127.0.0.1:{}/health", port);
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

#[tokio::test]
async fn logical_backup_exports_records_and_manual_restore_loads_them() {
    let tmp = tempfile::tempdir().expect("tmpdir");

    // --- Source server with known content ---------------------------------
    let src_dir = tmp.path().join("source");
    std::fs::create_dir_all(&src_dir).expect("create source dir");
    let src_port = portpicker::pick_unused_port().expect("free port");
    let src = spawn_server(src_dir, src_port).await;
    let src_base = format!("http://127.0.0.1:{}", src_port);

    let articles = vec![
        Article { id: "a1".into(), title: "First".into(), body: "hello".into(), views: 10 },
        Article { id: "a2".into(), title: "Second".into(), body: "world".into(), views: 0 },
        Article { id: "a3".into(), title: "Third".into(), body: "lithair".into(), views: 42 },
    ];
    for a in &articles {
        let resp = src
            .post(format!("{}/api/articles", src_base))
            .json(a)
            .send()
            .await
            .expect("POST article");
        assert!(resp.status().is_success(), "create article {} -> {}", a.id, resp.status());
    }

    // --- Strategy A export: POST /_admin/data/backup ----------------------
    let resp = src
        .post(format!("{}/_admin/data/backup", src_base))
        .send()
        .await
        .expect("POST backup");
    assert_eq!(resp.status(), 200, "backup endpoint returns 200");
    // The runbook documents this header so operators can `-o file.json`.
    let disposition = resp
        .headers()
        .get("Content-Disposition")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        disposition.contains("lithair_backup.json"),
        "backup advertises a downloadable filename; got {:?}",
        disposition
    );

    let backup: serde_json::Value = resp.json().await.expect("backup json");

    // --- Assert the export SHAPE (matches the runbook example) ------------
    assert_eq!(backup["backup_type"], "full", "backup_type is full");
    assert!(backup["timestamp"].is_string(), "timestamp present");
    assert_eq!(backup["model_count"].as_u64(), Some(1), "one registered model");
    let models = backup["models"].as_array().expect("models array");
    assert_eq!(models.len(), 1, "one model export");
    let model = &models[0];
    assert_eq!(model["model"], "Article", "model name");
    assert_eq!(model["count"].as_u64(), Some(3), "export reports 3 records");

    // --- Assert the export CONTENT (the actual records) -------------------
    let data = model["data"].as_array().expect("data array");
    assert_eq!(data.len(), 3, "all three articles in the export");
    let exported: Vec<Article> = data
        .iter()
        .map(|v| serde_json::from_value(v.clone()).expect("article from export"))
        .collect();
    for original in &articles {
        let found = exported
            .iter()
            .find(|e| e.id == original.id)
            .unwrap_or_else(|| panic!("article {} present in export", original.id));
        assert_eq!(found, original, "exported article {} is field-identical", original.id);
    }

    // --- Manual restore (the documented Strategy A restore, no fake import)
    // Re-POST every exported record into a fresh, empty server. This is the
    // exact operator procedure until issue #37 adds an import endpoint.
    let dst_dir = tmp.path().join("restored");
    std::fs::create_dir_all(&dst_dir).expect("create restored dir");
    let dst_port = portpicker::pick_unused_port().expect("free port");
    let dst = spawn_server(dst_dir, dst_port).await;
    let dst_base = format!("http://127.0.0.1:{}", dst_port);

    for a in &exported {
        let resp = dst
            .post(format!("{}/api/articles", dst_base))
            .json(a)
            .send()
            .await
            .expect("re-POST article");
        assert!(resp.status().is_success(), "restore article {} -> {}", a.id, resp.status());
    }

    // Confirm the restored server now serves all three, field-identical.
    let resp = dst
        .post(format!("{}/_admin/data/backup", dst_base))
        .send()
        .await
        .expect("POST backup on restored");
    let restored_backup: serde_json::Value = resp.json().await.expect("restored backup json");
    let restored_data = restored_backup["models"][0]["data"].as_array().expect("restored data");
    assert_eq!(restored_data.len(), 3, "manual restore loaded all 3 records");
    let restored_articles: Vec<Article> = restored_data
        .iter()
        .map(|v| serde_json::from_value(v.clone()).expect("article"))
        .collect();
    for original in &articles {
        let found = restored_articles
            .iter()
            .find(|e| e.id == original.id)
            .unwrap_or_else(|| panic!("article {} present after manual restore", original.id));
        assert_eq!(found, original, "manually restored article {} matches", original.id);
    }
}

#[tokio::test]
async fn logical_import_endpoint_restores_a_whole_backup_in_one_call() {
    let tmp = tempfile::tempdir().expect("tmpdir");

    // --- Source server with known content ---------------------------------
    let src_dir = tmp.path().join("source");
    std::fs::create_dir_all(&src_dir).expect("create source dir");
    let src_port = portpicker::pick_unused_port().expect("free port");
    let src = spawn_server(src_dir, src_port).await;
    let src_base = format!("http://127.0.0.1:{}", src_port);

    let articles = vec![
        Article { id: "a1".into(), title: "First".into(), body: "hello".into(), views: 10 },
        Article { id: "a2".into(), title: "Second".into(), body: "world".into(), views: 0 },
        Article { id: "a3".into(), title: "Third".into(), body: "lithair".into(), views: 42 },
    ];
    for a in &articles {
        let resp = src
            .post(format!("{}/api/articles", src_base))
            .json(a)
            .send()
            .await
            .expect("POST article");
        assert!(resp.status().is_success(), "create article {} -> {}", a.id, resp.status());
    }

    let backup: serde_json::Value = src
        .post(format!("{}/_admin/data/backup", src_base))
        .send()
        .await
        .expect("POST backup")
        .json()
        .await
        .expect("backup json");

    // --- Import the WHOLE backup into a fresh, empty server in one call ----
    let dst_dir = tmp.path().join("imported");
    std::fs::create_dir_all(&dst_dir).expect("create imported dir");
    let dst_port = portpicker::pick_unused_port().expect("free port");
    let dst = spawn_server(dst_dir, dst_port).await;
    let dst_base = format!("http://127.0.0.1:{}", dst_port);

    let resp = dst
        .post(format!("{}/_admin/data/import", dst_base))
        .json(&backup)
        .send()
        .await
        .expect("POST import");
    assert_eq!(resp.status(), 200, "clean import returns 200");
    let import: serde_json::Value = resp.json().await.expect("import json");
    assert_eq!(import["status"], "imported", "import payload: {}", import);
    assert_eq!(import["total_imported"].as_u64(), Some(3), "all three imported");
    assert_eq!(import["models"][0]["model"], "Article");
    assert_eq!(import["models"][0]["imported"].as_u64(), Some(3));

    // The imported server now serves all three, field-identical.
    let restored: serde_json::Value = dst
        .post(format!("{}/_admin/data/backup", dst_base))
        .send()
        .await
        .expect("POST backup on imported")
        .json()
        .await
        .expect("restored backup json");
    let restored_data = restored["models"][0]["data"].as_array().expect("restored data");
    assert_eq!(restored_data.len(), 3, "import loaded all 3 records");
    let restored_articles: Vec<Article> = restored_data
        .iter()
        .map(|v| serde_json::from_value(v.clone()).expect("article"))
        .collect();
    for original in &articles {
        let found = restored_articles
            .iter()
            .find(|e| e.id == original.id)
            .unwrap_or_else(|| panic!("article {} present after import", original.id));
        assert_eq!(found, original, "imported article {} is field-identical", original.id);
    }

    // --- Idempotency: re-importing overwrites by id, never duplicates ------
    let resp = dst
        .post(format!("{}/_admin/data/import", dst_base))
        .json(&backup)
        .send()
        .await
        .expect("POST import again");
    assert_eq!(resp.status(), 200, "re-import returns 200");
    let count_after: serde_json::Value = dst
        .post(format!("{}/_admin/data/backup", dst_base))
        .send()
        .await
        .expect("POST backup after re-import")
        .json()
        .await
        .expect("json");
    assert_eq!(
        count_after["models"][0]["count"].as_u64(),
        Some(3),
        "re-import did not duplicate entities (idempotent by id)"
    );

    // --- Partial success: an unknown model yields 207 Multi-Status --------
    let mixed = serde_json::json!({
        "models": [
            { "model": "Article", "data": [] },
            { "model": "DoesNotExist", "data": [ { "id": "x" } ] }
        ]
    });
    let resp = dst
        .post(format!("{}/_admin/data/import", dst_base))
        .json(&mixed)
        .send()
        .await
        .expect("POST mixed import");
    assert_eq!(resp.status(), 207, "partial import returns 207 Multi-Status");
    let mixed_res: serde_json::Value = resp.json().await.expect("mixed json");
    assert_eq!(mixed_res["status"], "partial");
    let entries = mixed_res["models"].as_array().expect("models array");
    assert!(
        entries
            .iter()
            .any(|e| e["model"] == "DoesNotExist" && e["status"] == "unknown_model"),
        "unknown model reported: {}",
        mixed_res
    );
}
