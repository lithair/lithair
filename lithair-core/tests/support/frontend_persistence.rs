use lithair_core::{
    engine::EventStore,
    frontend::{FrontendEngine, StaticAsset},
};
use std::{path::Path, sync::Arc};

fn persisted_bytes(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().metadata().unwrap().len())
        .sum()
}

pub async fn unchanged_loads_reloads_and_updates_do_not_append() {
    let data = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    std::fs::write(source.path().join("index.html"), b"first").unwrap();
    let engine = Arc::new(FrontendEngine::new("site", data.path()).await.unwrap());
    engine.load_directory(source.path()).await.unwrap();
    let original = engine.get_asset("/index.html").await.unwrap();
    let version = engine.version();
    let count = engine.engine().event_store().read().unwrap().event_count();
    let size = persisted_bytes(&data.path().join("frontend_site"));
    for _ in 0..4 {
        engine.load_directory(source.path()).await.unwrap();
        assert!(!engine.reload().await.unwrap().changed);
        engine.update_asset("/index.html", b"first".to_vec()).await.unwrap();
        engine.delete_asset("/absent").await.unwrap();
    }
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let engine = engine.clone();
        tasks.spawn(async move { engine.reload().await.unwrap() });
    }
    while let Some(result) = tasks.join_next().await {
        assert!(!result.unwrap().changed);
    }
    assert_eq!(engine.engine().event_store().read().unwrap().event_count(), count);
    assert_eq!(persisted_bytes(&data.path().join("frontend_site")), size);
    assert_eq!(engine.get_asset("/index.html").await.unwrap().id, original.id);
    assert_eq!(engine.version(), version);
    drop(engine);
    for _ in 0..3 {
        let engine = FrontendEngine::new("site", data.path()).await.unwrap();
        engine.load_directory(source.path()).await.unwrap();
        assert_eq!(engine.version(), version);
        assert_eq!(engine.engine().event_store().read().unwrap().event_count(), 0);
    }
}

pub async fn legacy_history_is_compacted_without_losing_deletes_or_mime() {
    let data = tempfile::tempdir().unwrap();
    let dir = data.path().join("frontend_site");
    let mut store = EventStore::new(dir.to_str().unwrap()).unwrap();
    let mut asset = StaticAsset::new("/page".into(), vec![b'x'; 16 * 1024]);
    asset.set_mime_type("text/html");
    for _ in 0..64 {
        store.append_event(&asset).unwrap();
    }
    store.append_event(&StaticAsset::new("/gone".into(), b"gone".to_vec())).unwrap();
    store.append_raw_line(r#"{"AssetDeleted":{"path":"/gone"}}"#).unwrap();
    store.force_flush().unwrap();
    drop(store);
    let before = persisted_bytes(&dir);
    for _ in 0..2 {
        let engine = FrontendEngine::new("site", data.path()).await.unwrap();
        assert_eq!(engine.asset_count(), 1);
        assert!(engine.get_asset("/gone").await.is_none());
        let restored = engine.get_asset("/page").await.unwrap();
        assert_eq!(restored.content, asset.content);
        assert_eq!(restored.mime_type, "text/html");
        assert_eq!(restored.id, asset.id);
        assert!(persisted_bytes(&dir) < before / 4, "obsolete history was retained");
    }
}

pub async fn changed_assets_and_removals_stay_bounded_across_restart() {
    let data = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    std::fs::write(source.path().join("page.html"), vec![b'a'; 32 * 1024]).unwrap();
    std::fs::write(source.path().join("removed.txt"), b"remove").unwrap();
    let engine = FrontendEngine::new("site", data.path()).await.unwrap();
    let other = FrontendEngine::new("other", data.path()).await.unwrap();
    other.update_asset("/removed.txt", b"other".to_vec()).await.unwrap();
    engine.load_directory(source.path()).await.unwrap();
    let before = persisted_bytes(&data.path().join("frontend_site"));
    for n in 0..40 {
        std::fs::write(source.path().join("page.html"), vec![b'b' + n % 2; 32 * 1024]).unwrap();
        assert!(engine.reload().await.unwrap().changed);
    }
    assert!(persisted_bytes(&data.path().join("frontend_site")) < before * 8);
    drop(engine);
    // A deletion between process runs is reconciled by load_directory too.
    std::fs::remove_file(source.path().join("removed.txt")).unwrap();
    let engine = FrontendEngine::new("site", data.path()).await.unwrap();
    engine.load_directory(source.path()).await.unwrap();
    assert!(engine.get_asset("/removed.txt").await.is_none());
    engine
        .update_asset_with_mime("/page.html", vec![b'c'; 32 * 1024], "text/plain")
        .await
        .unwrap();
    drop(engine);
    let engine = FrontendEngine::new("site", data.path()).await.unwrap();
    assert_eq!(engine.get_asset("/page.html").await.unwrap().mime_type, "text/plain");
    assert!(engine.get_asset("/removed.txt").await.is_none());
    assert_eq!(other.get_asset("/removed.txt").await.unwrap().content, b"other");
}

pub async fn corrupt_history_is_rejected_without_compacting_away_evidence() {
    for bad in ["00000000:{}\n", "{invalid\n"] {
        let data = tempfile::tempdir().unwrap();
        let dir = data.path().join("frontend_site");
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("events.raftlog");
        let asset = StaticAsset::new("/page".into(), b"ok".to_vec());
        let bytes = format!("{}\n{bad}", serde_json::to_string(&asset).unwrap());
        std::fs::write(&path, &bytes).unwrap();
        assert!(FrontendEngine::new("site", data.path()).await.is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), bytes);
    }
}
