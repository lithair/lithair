//! Regression coverage for issue #227: withdrawn assets must stay withdrawn.
use lithair_core::frontend::{AssetServer, FrontendEngine};
use std::sync::Arc;

#[tokio::test]
async fn deletion_removes_served_content_and_isolates_paths_and_hosts() {
    let data = tempfile::tempdir().unwrap();
    let engine = Arc::new(FrontendEngine::new("blog", data.path()).await.unwrap());
    let other = FrontendEngine::new("other", data.path()).await.unwrap();
    engine
        .update_asset_with_mime("/posts/hello", b"published".to_vec(), "text/html")
        .await
        .unwrap();
    engine.update_asset("/keep.txt", b"keep".to_vec()).await.unwrap();
    other.update_asset("/posts/hello", b"other host".to_vec()).await.unwrap();
    let server = AssetServer::new_scc2(engine.clone());
    assert!(server.serve_asset("/posts/hello").await.is_some());

    engine.delete_asset("/posts/hello").await.unwrap();
    assert!(engine.get_asset("/posts/hello").await.is_none());
    assert!(server.serve_asset("/posts/hello").await.is_none());
    assert_eq!(engine.asset_count(), 1);
    assert_eq!(engine.list_assets()[0].path, "/keep.txt");
    assert_eq!(engine.total_bytes(), 4);
    assert_eq!(other.get_asset("/posts/hello").await.unwrap().content, b"other host");
    let version = engine.version();
    engine.delete_asset("/posts/hello").await.unwrap();
    engine.delete_asset("/never-existed").await.unwrap();
    assert_eq!(engine.version(), version);
    assert_eq!(engine.asset_count(), 1);
}

#[tokio::test]
async fn deletion_and_recreation_survive_restart() {
    let data = tempfile::tempdir().unwrap();
    {
        let engine = FrontendEngine::new("blog", data.path()).await.unwrap();
        engine.update_asset("/removed", b"old".to_vec()).await.unwrap();
        engine.update_asset("/keep", b"kept".to_vec()).await.unwrap();
        engine.delete_asset("/removed").await.unwrap();
    }
    {
        let engine = FrontendEngine::new("blog", data.path()).await.unwrap();
        assert!(engine.get_asset("/removed").await.is_none());
        assert_eq!(engine.get_asset("/keep").await.unwrap().content, b"kept");
        assert_eq!(engine.asset_count(), 1);
        engine
            .update_asset_with_mime("/removed", b"new".to_vec(), "text/html; charset=utf-8")
            .await
            .unwrap();
        assert_eq!(engine.get_asset("/removed").await.unwrap().content, b"new");
    }
    let engine = FrontendEngine::new("blog", data.path()).await.unwrap();
    let recreated = engine.get_asset("/removed").await.unwrap();
    assert_eq!(recreated.content, b"new");
    assert_eq!(recreated.mime_type, "text/html; charset=utf-8");
    assert_eq!(engine.asset_count(), 2);
    assert_eq!(engine.total_bytes(), 7);
}

#[tokio::test]
async fn legacy_asset_events_can_be_replayed_and_deleted() {
    use lithair_core::{engine::EventStore, frontend::StaticAsset};

    let data = tempfile::tempdir().unwrap();
    {
        let path = data.path().join("frontend_blog");
        let mut store = EventStore::new(path.to_str().unwrap()).unwrap();
        // This is the bare StaticAsset format written before issue #227.
        store
            .append_event(&StaticAsset::new("/legacy".into(), b"legacy".to_vec()))
            .unwrap();
        store.force_flush().unwrap();
    }
    {
        let engine = FrontendEngine::new("blog", data.path()).await.unwrap();
        assert_eq!(engine.get_asset("/legacy").await.unwrap().content, b"legacy");
        engine.delete_asset("/legacy").await.unwrap();
    }
    let engine = FrontendEngine::new("blog", data.path()).await.unwrap();
    assert!(engine.get_asset("/legacy").await.is_none());
    assert_eq!(engine.asset_count(), 0);
    assert_eq!(engine.total_bytes(), 0);
    assert_eq!(engine.version(), FrontendEngine::compute_version_pub(&[]));
}

#[tokio::test]
async fn directory_reload_removals_survive_restart() {
    let data = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    std::fs::write(source.path().join("old.html"), b"old").unwrap();
    {
        let engine = FrontendEngine::new("blog", data.path()).await.unwrap();
        engine.load_directory(source.path()).await.unwrap();
        std::fs::remove_file(source.path().join("old.html")).unwrap();
        std::fs::write(source.path().join("new.html"), b"new").unwrap();
        engine.reload().await.unwrap();
        assert!(engine.get_asset("/old.html").await.is_none());
    }
    let engine = FrontendEngine::new("blog", data.path()).await.unwrap();
    assert!(engine.get_asset("/old.html").await.is_none());
    assert_eq!(engine.get_asset("/new.html").await.unwrap().content, b"new");
    assert_eq!(engine.asset_count(), 1);
}

#[tokio::test]
async fn storage_failure_is_reported_without_removing_the_live_asset() {
    let data = tempfile::tempdir().unwrap();
    let engine = FrontendEngine::new("blog", data.path()).await.unwrap();
    engine.update_asset("/keep", b"kept".to_vec()).await.unwrap();
    let store = engine.engine().event_store();
    // Poisoning the store provides a deterministic failure without filesystem
    // permission assumptions (the tests also run as root in CI containers).
    assert!(std::thread::spawn(move || {
        let _guard = store.write().unwrap();
        panic!("injected storage failure");
    })
    .join()
    .is_err());
    assert!(engine.delete_asset("/keep").await.is_err());
    assert_eq!(engine.get_asset("/keep").await.unwrap().content, b"kept");
}
