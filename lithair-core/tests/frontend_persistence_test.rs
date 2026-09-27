//! Issue #277: frontend history must scale with live assets, not deployments.
use lithair_core::frontend::{FrontendEngine, StaticAsset};
use std::io::Write;
#[path = "support/frontend_persistence.rs"]
mod cases;

#[tokio::test]
async fn unchanged_loads_reloads_and_updates_do_not_append() {
    cases::unchanged_loads_reloads_and_updates_do_not_append().await;
}

#[tokio::test]
async fn legacy_history_is_compacted_without_losing_deletes_or_mime() {
    cases::legacy_history_is_compacted_without_losing_deletes_or_mime().await;
}

#[tokio::test]
async fn changed_assets_and_removals_stay_bounded_across_restart() {
    cases::changed_assets_and_removals_stay_bounded_across_restart().await;
}

#[tokio::test]
async fn corrupt_history_is_rejected_without_compacting_away_evidence() {
    cases::corrupt_history_is_rejected_without_compacting_away_evidence().await;
}

#[tokio::test]
async fn checkpoint_failure_is_reported_and_recovery_preserves_the_durable_prefix() {
    let data = tempfile::tempdir().unwrap();
    let dir = data.path().join("frontend_site");
    let engine = FrontendEngine::new("site", data.path()).await.unwrap();
    engine.update_asset("/page", b"one".to_vec()).await.unwrap();
    engine.update_asset("/page", b"two".to_vec()).await.unwrap();
    // Third event crosses the compaction threshold. Rename over a directory
    // fails deterministically, even when tests run as root.
    std::fs::create_dir(dir.join("state.raftsnap")).unwrap();
    assert!(engine.update_asset("/page", b"three".to_vec()).await.is_err());
    assert!(engine.update_asset("/page", b"four".to_vec()).await.is_err());
    std::fs::remove_dir(dir.join("state.raftsnap")).unwrap();
    drop(engine);
    let engine = FrontendEngine::new("site", data.path()).await.unwrap();
    assert_eq!(engine.get_asset("/page").await.unwrap().content, b"three");
    drop(engine);
    std::fs::write(dir.join("state.raftsnap"), b"{broken").unwrap();
    assert!(FrontendEngine::new("site", data.path()).await.is_err());
}

#[tokio::test]
async fn frontend_ignores_model_binary_and_async_settings_child() {
    let Ok(path) = std::env::var("FRONTEND_JSON_CHILD") else {
        return;
    };
    for _ in 0..2 {
        let engine = FrontendEngine::new("site", &path).await.unwrap();
        engine.update_asset("/page", b"one".to_vec()).await.unwrap();
        engine.update_asset("/page", b"two".to_vec()).await.unwrap();
        engine.update_asset("/page", b"three".to_vec()).await.unwrap();
    }
    let engine = FrontendEngine::new("site", &path).await.unwrap();
    assert_eq!(engine.get_asset("/page").await.unwrap().content, b"three");
}
#[test]
fn frontend_json_checkpoints_are_independent_of_model_persistence_settings() {
    let data = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "frontend_ignores_model_binary_and_async_settings_child",
            "--nocapture",
        ])
        .env("FRONTEND_JSON_CHILD", data.path())
        .env("LT_ENABLE_BINARY", "1")
        .env("LT_OPT_PERSIST", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(target_os = "linux")]
fn peak_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|line| {
            line.strip_prefix("VmHWM:")
                .map(|s| s.split_whitespace().next().unwrap().parse().unwrap())
        })
        .unwrap()
}
#[cfg(target_os = "linux")]
#[tokio::test]
async fn streaming_replay_memory_child() {
    let Ok(path) = std::env::var("FRONTEND_REPLAY_CHILD") else {
        return;
    };
    let before = peak_kib();
    let engine = FrontendEngine::new("site", &path).await.unwrap();
    assert_eq!(engine.asset_count(), 1);
    eprintln!("frontend replay peak increase: {} KiB", peak_kib().saturating_sub(before));
    assert!(
        peak_kib().saturating_sub(before) < 24 * 1024,
        "replay retained history-sized allocations: {} KiB",
        peak_kib() - before
    );
}
#[cfg(target_os = "linux")]
#[test]
fn replay_memory_does_not_scale_with_legacy_log_size() {
    let data = tempfile::tempdir().unwrap();
    let dir = data.path().join("frontend_site");
    std::fs::create_dir(&dir).unwrap();
    let line =
        serde_json::to_string(&StaticAsset::new("/bundle.js".into(), vec![b'x'; 128 * 1024]))
            .unwrap();
    let mut file = std::fs::File::create(dir.join("events.raftlog")).unwrap();
    for _ in 0..128 {
        writeln!(file, "{line}").unwrap();
    }
    drop(file);
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "streaming_replay_memory_child", "--nocapture"])
        .env("FRONTEND_REPLAY_CHILD", data.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    println!("{}", String::from_utf8_lossy(&output.stderr));
}
