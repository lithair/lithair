#[path = "support/native_checkpoint.rs"]
mod support;
use lithair_core::engine::{EventEnvelope, EventStore};
use support::*;

#[tokio::test]
async fn snapshot_then_replay_applies_only_uncovered_increments() {
    support::snapshot_then_replay_applies_only_uncovered_increments().await;
}
#[tokio::test]
async fn compacted_scc_snapshot_survives_reopen() {
    support::compacted_scc_snapshot_survives_reopen().await;
}
#[test]
fn truncation_requires_a_current_checkpoint() {
    support::truncation_requires_a_current_checkpoint();
}

#[test]
fn json_and_binary_checkpoints_keep_tail_and_hash_anchor() {
    for binary in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let open =
            || EventStore::new_with_options(dir.path().to_str().unwrap(), false, binary).unwrap();
        let mut store = open();
        for cycle in 0..3 {
            let append = |store: &mut EventStore, id: String| {
                let mut envelope = EventEnvelope::new(
                    "Increment".into(),
                    id,
                    0,
                    "null".into(),
                    Some("key".into()),
                    None,
                );
                envelope.event_hash = None; // Let the store assign the local chain link.
                store.append_envelope(&envelope).unwrap();
                store.flush().unwrap();
            };
            append(&mut store, format!("{cycle}-before"));
            store.save_snapshot("{}").unwrap();
            assert!(open().recovery_state().unwrap().1.is_empty());
            append(&mut store, format!("{cycle}-after"));
            assert!(store.truncate_events().is_err(), "stale snapshot must not erase tail");
            assert_eq!(open().recovery_state().unwrap().1.len(), 1);
            store.save_snapshot("{}").unwrap();
            let anchor = store.get_last_event_hash().cloned();
            store.truncate_events().unwrap();
            store = open();
            assert_eq!(store.get_last_event_hash().cloned(), anchor);
            append(&mut store, format!("{cycle}-new-generation"));
            assert_eq!(store.get_all_envelopes().unwrap()[0].previous_hash, anchor);
            assert!(store.verify_chain().unwrap().is_valid);
        }
    }
}

#[test]
fn missing_or_corrupt_checkpoint_files_fail_closed() {
    for damage in [
        "snapshot-missing",
        "snapshot-json",
        "snapshot-checksum",
        "journal-missing",
        "prefix",
        "tail-crc",
        "tail-json",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        let mut store = EventStore::new(path).unwrap();
        store.append_raw_line("1").unwrap();
        store.flush().unwrap();
        store.save_snapshot("1").unwrap();
        store.truncate_events().unwrap();
        store.append_raw_line("1").unwrap();
        store.flush().unwrap();
        if damage == "prefix" {
            store.save_snapshot("2").unwrap();
        }
        drop(store);
        let snapshot = dir.path().join("state.raftsnap");
        let journal = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.file_name().unwrap().to_str().unwrap().starts_with("native-journal-"))
            .unwrap();
        match damage {
            "snapshot-missing" => std::fs::remove_file(snapshot).unwrap(),
            "snapshot-json" => std::fs::write(snapshot, "{").unwrap(),
            "snapshot-checksum" => {
                let mut value: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(&snapshot).unwrap()).unwrap();
                value["__lithair_native_checkpoint"]["state"] = "9".into();
                std::fs::write(snapshot, value.to_string()).unwrap();
            }
            "journal-missing" => std::fs::remove_file(journal).unwrap(),
            "prefix" => std::fs::write(journal, "2\n").unwrap(),
            "tail-crc" => std::fs::write(journal, "00000000:1\n").unwrap(),
            _ => std::fs::write(journal, "{invalid\n").unwrap(),
        }
        assert!(EventStore::new(path).is_err(), "{damage}");
    }
}

#[tokio::test]
async fn queued_and_concurrent_writes_are_included_once() {
    for (immediate, binary) in [(false, false), (false, true), (true, false), (true, true)] {
        let dir = tempfile::tempdir().unwrap();
        let live = engine_mode(dir.path(), immediate, binary);
        // No explicit flush: snapshot itself drains the queue.
        live.apply_event("key".into(), Increment, true).await.unwrap();
        live.snapshot().unwrap();
        live.truncate_log().unwrap();
        let mut jobs = tokio::task::JoinSet::new();
        for _ in 0..32 {
            let live = live.clone();
            jobs.spawn(async move {
                live.apply_event("key".into(), Increment, true).await.unwrap();
            });
        }
        let other = live.clone();
        let snapshots = tokio::task::spawn_blocking(move || {
            for _ in 0..8 {
                other.snapshot().unwrap();
            }
        });
        while let Some(result) = jobs.join_next().await {
            result.unwrap();
        }
        snapshots.await.unwrap();
        live.flush().await.unwrap();
        assert!(live.event_store().read().unwrap().verify_chain().unwrap().is_valid);
        live.snapshot().unwrap();
        live.truncate_log().unwrap();
        let reopened = engine_mode(dir.path(), immediate, binary);
        reopened.replay_events::<Increment>().unwrap();
        assert_eq!(reopened.read("key", |v| v.value), Some(33));
        reopened.apply_event("key".into(), Increment, true).await.unwrap();
        reopened.flush().await.unwrap();
        assert!(reopened.event_store().read().unwrap().verify_chain().unwrap().is_valid);
    }
}

#[tokio::test]
async fn legacy_raw_snapshot_is_adopted_without_losing_state() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("state.raftsnap"),
        r#"{"key":{"version":4,"last_updated":0,"data":{"value":4}}}"#,
    )
    .unwrap();
    let live = engine(dir.path());
    live.replay_events::<Increment>().unwrap();
    assert_eq!(live.read("key", |v| v.value), Some(4));
    live.apply_event("key".into(), Increment, true).await.unwrap();
    live.snapshot().unwrap();
    live.truncate_log().unwrap();
    let reopened = engine(dir.path());
    reopened.replay_events::<Increment>().unwrap();
    assert_eq!(reopened.read("key", |v| v.value), Some(5));
}

#[tokio::test]
async fn warm_state_reconstructs_from_checkpoint_and_cannot_be_compacted() {
    use lithair_core::lifecycle::RetentionConfig;
    let dir = tempfile::tempdir().unwrap();
    let live = engine(dir.path());
    live.apply_event("first".into(), Increment, true).await.unwrap();
    live.apply_event("second".into(), Increment, true).await.unwrap();
    live.snapshot().unwrap();
    live.truncate_log().unwrap();
    live.apply_event("first".into(), Increment, true).await.unwrap();
    live.apply_event("second".into(), Increment, true).await.unwrap();
    live.flush().await.unwrap();
    let mut reopened = engine(dir.path());
    reopened
        .enable_retention(RetentionConfig { memory_count: Some(1), ..Default::default() }, vec![]);
    reopened.replay_events::<Increment>().unwrap();
    for key in ["first", "second"] {
        assert_eq!(reopened.read_or_load::<_, _, Increment>(key, |s| s.value), Some(2));
    }
    assert!(reopened.snapshot().unwrap_err().to_string().contains("warm records"));
    assert!(reopened.truncate_log().is_err());
}

#[tokio::test]
async fn repeated_http_compaction_preserves_updates_and_deletes() {
    support::repeated_http_compaction_preserves_updates_and_deletes().await;
}

#[test]
fn checkpointed_binary_journals_reject_torn_or_undecodable_frames() {
    use std::io::Write;
    for suffix in [vec![1, 2], [3u64.to_le_bytes().as_slice(), &[255, 255, 255]].concat()] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        let mut store = EventStore::new_with_options(path, false, true).unwrap();
        store.save_snapshot("{}").unwrap();
        store.truncate_events().unwrap();
        drop(store);
        let journal = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.extension().is_some_and(|e| e == "raftlog"))
            .unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(journal)
            .unwrap()
            .write_all(&suffix)
            .unwrap();
        assert!(EventStore::new_with_options(path, false, true).is_err());
    }
}

#[tokio::test]
async fn failed_snapshot_publication_stops_scc_queue_admission() {
    for immediate in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let live = engine_mode(dir.path(), immediate, false);
        live.apply_event("key".into(), Increment, true).await.unwrap();
        std::fs::create_dir(dir.path().join("state.raftsnap")).unwrap();
        assert!(live.snapshot().is_err());
        assert!(live.apply_event("key".into(), Increment, true).await.is_err());
        assert_eq!(live.read("key", |s| s.value), Some(1));
        std::fs::remove_dir(dir.path().join("state.raftsnap")).unwrap();
        let restored = engine(dir.path());
        restored.replay_events::<Increment>().unwrap();
        assert_eq!(restored.read("key", |s| s.value), Some(1));
    }
}
