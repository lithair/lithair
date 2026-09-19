#![cfg(feature = "cluster")]
#[path = "../src/cluster/durable_log/mod.rs"]
#[allow(dead_code)]
mod durable_log;

use durable_log::{DurableLog, Stage};
use openraft::{
    storage::{RaftLogStorage, RaftLogStorageExt},
    CommittedLeaderId, Entry, EntryPayload, LogId, Membership, RaftLogReader, SnapshotMeta,
    StoredMembership, Vote,
};
use openraft_memstore::{ClientRequest, TypeConfig};
use std::{
    collections::BTreeSet,
    io::{Seek, SeekFrom, Write},
    path::Path,
    time::Duration,
};

fn id(index: u64) -> LogId<u64> {
    LogId::new(CommittedLeaderId::new(3, 1), index)
}
fn meta(index: u64) -> SnapshotMeta<u64, ()> {
    SnapshotMeta {
        last_log_id: Some(id(index)),
        last_membership: StoredMembership::new(
            Some(id(0)),
            Membership::new(vec![BTreeSet::from([1, 2, 3])], None),
        ),
        snapshot_id: format!("snapshot-{index}"),
    }
}
async fn seeded(path: &Path) -> DurableLog<TypeConfig> {
    let store = DurableLog::<TypeConfig>::create(path).await.unwrap();
    let mut log = store.clone().into_log_store();
    log.save_vote(&Vote::new_committed(3, 1)).await.unwrap();
    log.blocking_append((0..100).map(|index| Entry {
        log_id: id(index),
        payload: EntryPayload::Normal(ClientRequest {
            client: index.to_string(),
            serial: index,
            status: "x".repeat(1024),
        }),
    }))
    .await
    .unwrap();
    log.save_committed(Some(id(98))).await.unwrap();
    store.save_snapshot(meta(95), b"old snapshot".to_vec()).await.unwrap();
    log.purge(id(95)).await.unwrap();
    store
}
fn manifest(path: &Path) -> serde_json::Value {
    let bytes = std::fs::read(path.join("durable.meta")).unwrap();
    serde_json::from_slice(&bytes[4..bytes.len() - 4]).unwrap()
}
fn generation_file(path: &Path, field: &str) -> std::path::PathBuf {
    let value = manifest(path);
    let uuid =
        if field == "journal" { value[field].clone() } else { value[field]["generation"].clone() };
    let generation: [u8; 16] = serde_json::from_value(uuid).unwrap();
    path.join(format!("{field}-{}", uuid::Uuid::from_bytes(generation).simple()))
}
async fn verify(store: &DurableLog<TypeConfig>) {
    let mut log = store.clone().into_log_store();
    assert_eq!(log.read_vote().await.unwrap(), Some(Vote::new_committed(3, 1)));
    assert_eq!(log.read_committed().await.unwrap(), Some(id(98)));
    assert_eq!(log.get_log_state().await.unwrap().last_purged_log_id, Some(id(95)));
    assert_eq!(
        log.try_get_log_entries(..)
            .await
            .unwrap()
            .iter()
            .map(|e| e.log_id)
            .collect::<Vec<_>>(),
        (96..100).map(id).collect::<Vec<_>>()
    );
    let snapshot = store.snapshot().await.unwrap().unwrap();
    assert_eq!(snapshot.meta.last_membership, meta(95).last_membership);
    assert!(snapshot.data == b"old snapshot"[..] || snapshot.data == b"new snapshot"[..]);
}

#[tokio::test]
async fn snapshot_compaction_retains_uncommitted_suffix_and_reclaims_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let store = seeded(dir.path()).await;
    let original = std::fs::metadata(dir.path().join("journal")).unwrap().len();
    store.compact().await.unwrap();
    assert!(!dir.path().join("journal").exists());
    assert!(
        std::fs::metadata(generation_file(dir.path(), "journal")).unwrap().len() < original / 5
    );
    verify(&store).await;
    let retained = store.snapshot().await.unwrap().unwrap();
    store.save_snapshot(meta(98), b"new snapshot".to_vec()).await.unwrap();
    assert_eq!(retained.data, b"old snapshot"[..]);
    for _ in 0..3 {
        store.compact().await.unwrap();
    }
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 4); // LOCK, manifest, journal, snapshot
    drop(store);
    let store = DurableLog::<TypeConfig>::open(dir.path()).await.unwrap();
    verify(&store).await;
    let mut log = store.clone().into_log_store();
    log.truncate(id(99)).await.unwrap(); // uncommitted remains replaceable
    log.blocking_append([Entry { log_id: id(99), payload: EntryPayload::Blank }])
        .await
        .unwrap();
    store.compact().await.unwrap();
    drop(log);
    drop(store);
    let store = DurableLog::<TypeConfig>::open(dir.path()).await.unwrap();
    verify(&store).await;
}

#[tokio::test]
async fn version_one_upgrade_is_explicit_and_empty_compaction_recovers() {
    let dir = tempfile::tempdir().unwrap();
    let store = DurableLog::<TypeConfig>::create(dir.path()).await.unwrap();
    assert_eq!(manifest(dir.path())["version"], 1);
    drop(store);
    let store = DurableLog::<TypeConfig>::open(dir.path()).await.unwrap();
    assert_eq!(manifest(dir.path())["version"], 1);
    store.compact().await.unwrap();
    assert_eq!(manifest(dir.path())["version"], 2);
    drop(store);
    let store = DurableLog::<TypeConfig>::open(dir.path()).await.unwrap();
    assert!(store.snapshot().await.unwrap().is_none());
    assert!(store.into_log_store().get_log_state().await.unwrap().last_log_id.is_none());
}

#[tokio::test]
async fn stale_snapshot_and_unsafe_purge_leave_the_store_usable() {
    let dir = tempfile::tempdir().unwrap();
    let store = seeded(dir.path()).await;
    assert!(store.save_snapshot(meta(94), b"stale".to_vec()).await.is_err());
    let mut bad = meta(96);
    bad.last_membership =
        StoredMembership::new(Some(id(97)), Membership::new(vec![BTreeSet::from([1, 2, 3])], None));
    assert!(store.save_snapshot(bad, vec![]).await.is_err());
    assert!(store.save_snapshot(meta(96), vec![0; 64 * 1024 * 1024 + 1]).await.is_err());
    assert!(store.clone().into_log_store().purge(id(96)).await.is_err());
    verify(&store).await;
}

#[tokio::test]
async fn active_snapshot_and_journal_corruption_fail_closed() {
    for mode in [
        "snapshot-bytes",
        "snapshot-missing",
        "snapshot-truncated",
        "journal-identity",
        "journal-generation",
        "journal-missing",
        "manifest-version",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let store = seeded(dir.path()).await;
        store.compact().await.unwrap();
        drop(store);
        if mode.starts_with("snapshot") {
            let path = generation_file(dir.path(), "snapshot");
            match mode {
                "snapshot-missing" => std::fs::remove_file(path).unwrap(),
                "snapshot-truncated" => {
                    std::fs::OpenOptions::new().write(true).open(path).unwrap().set_len(12).unwrap()
                }
                _ => {
                    let mut f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
                    f.seek(SeekFrom::End(-1)).unwrap();
                    f.write_all(b"!").unwrap();
                }
            }
        } else if mode.starts_with("journal") {
            let path = generation_file(dir.path(), "journal");
            if mode == "journal-missing" {
                std::fs::remove_file(path).unwrap();
            } else {
                let mut f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
                f.seek(SeekFrom::Start(if mode == "journal-identity" { 8 } else { 24 }))
                    .unwrap();
                f.write_all(&[0; 16]).unwrap();
            }
        } else {
            let mut value = manifest(dir.path());
            value["version"] = serde_json::json!(99);
            let body = serde_json::to_vec(&value).unwrap();
            let mut frame = (body.len() as u32).to_le_bytes().to_vec();
            frame.extend(body);
            frame.extend(crc32fast::hash(&frame).to_le_bytes());
            std::fs::write(dir.path().join("durable.meta"), frame).unwrap();
        }
        assert!(DurableLog::<TypeConfig>::open(dir.path()).await.is_err(), "{mode}");
    }
}

const STAGES: [Stage; 9] = [
    Stage::GenerationWrite,
    Stage::GenerationSync,
    Stage::GenerationDirectorySync,
    Stage::MetadataWrite,
    Stage::MetadataSync,
    Stage::MetadataRename,
    Stage::DirectorySync,
    Stage::Cleanup,
    Stage::Complete,
];
async fn operation(
    store: &DurableLog<TypeConfig>,
    name: &str,
) -> Result<(), openraft::StorageError<u64>> {
    match name {
        "compact" => store.compact().await,
        "snapshot" => store.save_snapshot(meta(98), b"new snapshot".to_vec()).await,
        _ => panic!("unknown operation"),
    }
}
#[tokio::test]
async fn io_failures_invalidate_handles_and_recover_complete_generations() {
    for name in ["compact", "snapshot"] {
        for stage in STAGES {
            let dir = tempfile::tempdir().unwrap();
            let store = seeded(dir.path()).await;
            let other = store.clone();
            store.inject_failure(stage, false).await;
            assert!(operation(&store, name).await.is_err(), "{name} {stage:?}");
            assert!(other.snapshot().await.is_err());
            drop(other);
            drop(store);
            let store = DurableLog::<TypeConfig>::open(dir.path()).await.unwrap();
            verify(&store).await;
            store.compact().await.unwrap();
            assert_eq!(
                std::fs::read_dir(dir.path())
                    .unwrap()
                    .filter_map(Result::ok)
                    .filter(|e| e.file_name().to_string_lossy().starts_with("journal-"))
                    .count(),
                1
            );
        }
    }
}
#[tokio::test]
async fn checkpoint_crash_child() {
    let Some(path) = std::env::var_os("LITHAIR_CHECKPOINT_CRASH_DIR") else {
        return;
    };
    let stage: usize = std::env::var("LITHAIR_CHECKPOINT_CRASH_STAGE").unwrap().parse().unwrap();
    let store = DurableLog::<TypeConfig>::open(path).await.unwrap();
    store.inject_failure(STAGES[stage], true).await;
    operation(&store, &std::env::var("LITHAIR_CHECKPOINT_CRASH_OP").unwrap())
        .await
        .unwrap();
    panic!("fault did not terminate process");
}
#[tokio::test]
async fn process_death_at_each_generation_boundary_preserves_acknowledged_state() {
    for name in ["compact", "snapshot"] {
        for stage in 0..STAGES.len() {
            let dir = tempfile::tempdir().unwrap();
            let store = seeded(dir.path()).await;
            drop(store);
            let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "checkpoint_crash_child", "--nocapture"])
                .env("LITHAIR_CHECKPOINT_CRASH_DIR", dir.path())
                .env("LITHAIR_CHECKPOINT_CRASH_STAGE", stage.to_string())
                .env("LITHAIR_CHECKPOINT_CRASH_OP", name)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::inherit())
                .kill_on_drop(true)
                .spawn()
                .unwrap();
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(15), child.wait())
                    .await
                    .unwrap()
                    .unwrap()
                    .code(),
                Some(86),
                "{name} {stage}"
            );
            let store = DurableLog::<TypeConfig>::open(dir.path()).await.unwrap();
            verify(&store).await;
            store.compact().await.unwrap();
            assert_eq!(
                std::fs::read_dir(dir.path()).unwrap().count(),
                4,
                "orphan generations after {name} {stage}"
            );
        }
    }
}
#[tokio::test]
async fn cancellation_finishes_an_admitted_generation_replacement() {
    for name in ["compact", "snapshot"] {
        let dir = tempfile::tempdir().unwrap();
        let store = seeded(dir.path()).await;
        let (reached, arrived) = tokio::sync::oneshot::channel();
        let (release, wait) = std::sync::mpsc::channel();
        store.pause_at(Stage::MetadataRename, reached, wait).await;
        let task_store = store.clone();
        let task = tokio::spawn(async move { operation(&task_store, name).await });
        tokio::time::timeout(Duration::from_secs(10), arrived).await.unwrap().unwrap();
        task.abort();
        let _ = task.await;
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(10), verify(&store)).await.unwrap();
        drop(store);
        let store = DurableLog::<TypeConfig>::open(dir.path()).await.unwrap();
        verify(&store).await;
    }
}

#[path = "support/openraft_snapshot_machine.rs"]
mod snapshot_machine;

#[tokio::test]
async fn state_machine_restores_snapshot_before_replaying_retained_suffix() {
    use openraft::{RaftSnapshotBuilder, RaftStorage};
    let dir = tempfile::tempdir().unwrap();
    let store = DurableLog::<TypeConfig>::create(dir.path()).await.unwrap();
    let mut machine = snapshot_machine::SnapshotMachine::restore(store.clone()).await.unwrap();
    let entries = (0..4)
        .map(|index| Entry {
            log_id: id(index),
            payload: EntryPayload::Normal(ClientRequest {
                client: index.to_string(),
                serial: 1,
                status: format!("value-{index}"),
            }),
        })
        .collect::<Vec<_>>();
    let mut log = store.clone().into_log_store();
    log.blocking_append(entries.clone()).await.unwrap();
    log.save_committed(Some(id(3))).await.unwrap();
    machine.apply_to_state_machine(&entries[..3]).await.unwrap();
    machine.build_snapshot().await.unwrap();
    machine.purge_logs_upto(id(2)).await.unwrap();
    drop(machine);
    drop(store);
    drop(log);
    let store = DurableLog::<TypeConfig>::open(dir.path()).await.unwrap();
    let machine = snapshot_machine::SnapshotMachine::restore(store.clone()).await.unwrap();
    let memory = machine.memory.clone();
    let (_, mut sm) = openraft::storage::Adaptor::new(machine);
    let mut log = store.into_log_store();
    openraft::storage::StorageHelper::new(&mut log, &mut sm)
        .get_initial_state()
        .await
        .unwrap();
    let state = memory.get_state_machine().await;
    assert_eq!(state.last_applied_log, Some(id(3)));
    assert_eq!(state.client_status.len(), 4);
    assert_eq!(state.client_status["3"], "value-3");
}

#[tokio::test]
async fn real_manifest_replace_error_preserves_the_previous_generation() {
    let dir = tempfile::tempdir().unwrap();
    let store = seeded(dir.path()).await;
    let path = dir.path().join("durable.meta");
    let previous = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(store.save_snapshot(meta(98), b"new snapshot".to_vec()).await.is_err());
    assert!(store.snapshot().await.is_err());
    drop(store);
    std::fs::remove_dir(&path).unwrap();
    std::fs::write(&path, previous).unwrap();
    let store = DurableLog::<TypeConfig>::open(dir.path()).await.unwrap();
    verify(&store).await;
    assert_eq!(store.snapshot().await.unwrap().unwrap().data, b"old snapshot"[..]);
}

#[tokio::test]
async fn snapshot_waiters_finish_on_publication_or_storage_failure() {
    for fail in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let store = seeded(dir.path()).await;
        let waiting = store.clone();
        let task = tokio::spawn(async move { waiting.wait_for_snapshot(id(98)).await });
        assert!(store.clone().into_log_store().purge(id(98)).await.is_err());
        if fail {
            store.inject_failure(Stage::GenerationSync, false).await;
        }
        let result = store.save_snapshot(meta(98), b"new snapshot".to_vec()).await;
        assert_eq!(result.is_err(), fail);
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), task)
                .await
                .unwrap()
                .unwrap()
                .is_err(),
            fail
        );
        if !fail {
            store.clone().into_log_store().purge(id(98)).await.unwrap();
        }
    }
}
