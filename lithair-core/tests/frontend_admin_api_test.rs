//! Integration test for issue #134 — frontend lifecycle admin API.
//!
//! Spins up a real `LithairServer` on a random port with TWO frontends loaded
//! from a temp directory:
//!   - a host-agnostic path-prefix frontend declared via `with_frontend_at("/")`
//!     (lives in `frontend_engines`)
//!   - a per-vhost frontend declared via `with_vhost("blog.test", ...)`
//!     (lives in `vhost_frontend_router`)
//!
//! It then exercises the full admin surface without ever restarting the server:
//!   1. `GET /_admin/frontend` lists BOTH frontends, each with a version.
//!   2. A file on disk backing the vhost frontend is modified.
//!   3. `POST /_admin/frontend/{vhost-key}/reload` reloads only that frontend.
//!   4. Asserts: the served vhost content changed, its version changed, and the
//!      path-prefix frontend's version did NOT change — all on the same process.
//!
//! Server-spawn pattern mirrors `graceful_shutdown_test.rs` (real server,
//! random port, `serve_with_graceful_shutdown` + oneshot).

use lithair_core::app::LithairServer;
use std::time::Duration;

/// Pull the `version` for a given frontend `key` out of the list payload.
fn version_for<'a>(list: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    list["frontends"]
        .as_array()?
        .iter()
        .find(|f| f["key"].as_str() == Some(key))
        .and_then(|f| f["version"].as_str())
}

#[tokio::test]
async fn frontend_admin_api_lists_inspects_and_reloads_per_vhost_without_restart() {
    let port = portpicker::pick_unused_port().expect("free port available");
    let base_url = format!("http://127.0.0.1:{}", port);

    // Two distinct on-disk frontends.
    let tmp = tempfile::tempdir().expect("tmpdir");
    let public_dir = tmp.path().join("public");
    let blog_dir = tmp.path().join("blog");
    std::fs::create_dir_all(&public_dir).expect("create public dir");
    std::fs::create_dir_all(&blog_dir).expect("create blog dir");

    std::fs::write(public_dir.join("index.html"), b"PUBLIC-V1").expect("write public index");
    let blog_index = blog_dir.join("index.html");
    std::fs::write(&blog_index, b"BLOG-V1").expect("write blog index");

    // FrontendEngine persists an event store under ./data/frontend; isolate it
    // to the temp dir so the test never writes into the repo.
    let data_dir = tmp.path().join("data");
    std::fs::create_dir_all(&data_dir).expect("create data dir");
    let prev_cwd = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(tmp.path()).expect("chdir tmp");

    let public_dir_s = public_dir.to_string_lossy().to_string();
    let blog_dir_s = blog_dir.to_string_lossy().to_string();

    let server = LithairServer::new()
        .with_host("127.0.0.1")
        .with_port(port)
        .with_data_admin() // gate for /_admin/frontend
        .with_frontend_at("/", public_dir_s) // path-prefix frontend
        .with_vhost("blog.test", move |v| v.with_frontend_at("/", blog_dir_s.clone())); // vhost frontend

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let shutdown = async move {
        let _ = shutdown_rx.await;
    };
    let serve_handle =
        tokio::spawn(async move { server.serve_with_graceful_shutdown(shutdown).await });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("reqwest client");

    // Wait for readiness.
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

    let path_key = "path:/";
    let vhost_key = "vhost:blog.test:/";
    // Percent-encode ':' and '/' so the key survives as a single path segment.
    let vhost_key_enc = "vhost%3Ablog.test%3A%2F";

    // 1. LIST — both frontends present, each with a version.
    let list: serde_json::Value = client
        .get(format!("{}/_admin/frontend", base_url))
        .send()
        .await
        .expect("list request")
        .json()
        .await
        .expect("list json");

    assert_eq!(list["total"].as_u64(), Some(2), "expected both frontends listed: {}", list);
    let path_v1 = version_for(&list, path_key).expect("path frontend listed").to_string();
    let blog_v1 = version_for(&list, vhost_key).expect("vhost frontend listed").to_string();
    assert!(path_v1.starts_with("sha256:"), "path version is a fingerprint: {}", path_v1);
    assert!(blog_v1.starts_with("sha256:"), "vhost version is a fingerprint: {}", blog_v1);

    // Sanity: the vhost frontend serves the original content.
    let served_v1 = client
        .get(&base_url)
        .header("Host", "blog.test")
        .send()
        .await
        .expect("serve v1")
        .text()
        .await
        .expect("serve v1 body");
    assert_eq!(served_v1, "BLOG-V1", "vhost serves original content before reload");

    // 2. INSPECT — detail carries config + version + manifest.
    let detail: serde_json::Value = client
        .get(format!("{}/_admin/frontend/{}", base_url, vhost_key_enc))
        .send()
        .await
        .expect("inspect request")
        .json()
        .await
        .expect("inspect json");
    assert_eq!(detail["kind"].as_str(), Some("vhost"));
    assert_eq!(detail["host"].as_str(), Some("blog.test"));
    assert_eq!(detail["version"].as_str(), Some(blog_v1.as_str()));
    assert_eq!(detail["asset_count"].as_u64(), Some(1));
    assert!(
        detail["paths"]
            .as_array()
            .map(|p| p.iter().any(|x| x == "/index.html"))
            .unwrap_or(false),
        "manifest lists /index.html: {}",
        detail
    );

    // 3. MODIFY the vhost frontend's file on disk, then RELOAD just that one.
    std::fs::write(&blog_index, b"BLOG-V2-CHANGED").expect("rewrite blog index");

    let reload: serde_json::Value = client
        .post(format!("{}/_admin/frontend/{}/reload", base_url, vhost_key_enc))
        .send()
        .await
        .expect("reload request")
        .json()
        .await
        .expect("reload json");
    assert_eq!(reload["status"].as_str(), Some("reloaded"), "reload payload: {}", reload);
    assert_eq!(reload["changed"].as_bool(), Some(true), "reload reports a content change");
    let blog_v2 = reload["version"].as_str().expect("reload returns version").to_string();
    assert_ne!(blog_v2, blog_v1, "vhost version changed after reload");

    // 4a. The vhost now serves the NEW content — no restart.
    let served_v2 = client
        .get(&base_url)
        .header("Host", "blog.test")
        .send()
        .await
        .expect("serve v2")
        .text()
        .await
        .expect("serve v2 body");
    assert_eq!(served_v2, "BLOG-V2-CHANGED", "vhost serves reloaded content");

    // 4b. The path-prefix frontend's version is UNCHANGED — reload was isolated.
    let list_after: serde_json::Value = client
        .get(format!("{}/_admin/frontend", base_url))
        .send()
        .await
        .expect("list-after request")
        .json()
        .await
        .expect("list-after json");
    let path_v2 = version_for(&list_after, path_key).expect("path frontend still listed");
    assert_eq!(path_v2, path_v1, "untouched path frontend version is unchanged");
    let blog_listed_v2 = version_for(&list_after, vhost_key).expect("vhost still listed");
    assert_eq!(blog_listed_v2, blog_v2, "list reflects the reloaded vhost version");

    // The public frontend still serves its original content.
    let public_served = client
        .get(&base_url)
        .send()
        .await
        .expect("serve public")
        .text()
        .await
        .expect("serve public body");
    assert_eq!(public_served, "PUBLIC-V1", "untouched path frontend content is unchanged");

    // Reload-all returns a per-frontend result set.
    let reload_all: serde_json::Value = client
        .post(format!("{}/_admin/frontend/reload", base_url))
        .send()
        .await
        .expect("reload-all request")
        .json()
        .await
        .expect("reload-all json");
    assert_eq!(
        reload_all["results"].as_array().map(|r| r.len()),
        Some(2),
        "reload-all covers both frontends: {}",
        reload_all
    );

    // Shutdown.
    shutdown_tx.send(()).expect("shutdown receiver alive");
    let outcome = tokio::time::timeout(Duration::from_secs(15), serve_handle).await;
    let joined = outcome.expect("server did not return after shutdown signal");
    let serve_result = joined.expect("serve task panicked");
    serve_result.expect("serve returned an error");

    // Restore cwd for any sibling tests in the same process.
    std::env::set_current_dir(prev_cwd).expect("restore cwd");
}
