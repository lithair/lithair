#![cfg(feature = "cluster")]

// Exercise the private source without publishing a premature storage API.
#[path = "../src/cluster/durable_log/mod.rs"]
mod durable_log;

use durable_log::{DurableLog, Stage};
use openraft::storage::{
    Adaptor, RaftLogStorage, RaftLogStorageExt, RaftStateMachine, StorageHelper,
};
use openraft::testing::{StoreBuilder, Suite};
use openraft::{
    CommittedLeaderId, Entry, EntryPayload, LogId, Membership, RaftLogReader, StorageError, Vote,
};
use openraft_memstore::{MemStore, TypeConfig};
use std::collections::BTreeSet;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};

type Store = Adaptor<TypeConfig, DurableLog<TypeConfig>>;
type TestStateMachine = Adaptor<TypeConfig, std::sync::Arc<MemStore>>;
async fn open(path: impl AsRef<std::path::Path>) -> Result<Store, StorageError<u64>> {
    Ok(DurableLog::open(path).await?.into_log_store())
}
async fn test_state_machine() -> TestStateMachine {
    Adaptor::new(MemStore::new_async().await).1
}
fn id(term: u64, index: u64) -> LogId<u64> {
    LogId::new(CommittedLeaderId::new(term, 1), index)
}
fn entry(term: u64, index: u64) -> Entry<TypeConfig> {
    Entry { log_id: id(term, index), payload: EntryPayload::Blank }
}
async fn indices(store: &mut Store) -> Vec<u64> {
    store
        .try_get_log_entries(..)
        .await
        .unwrap()
        .iter()
        .map(|e| e.log_id.index)
        .collect()
}

struct Builder;
impl StoreBuilder<TypeConfig, Store, TestStateMachine, tempfile::TempDir> for Builder {
    async fn build(
        &self,
    ) -> Result<(tempfile::TempDir, Store, TestStateMachine), StorageError<u64>> {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = open(dir.path()).await?;
        Ok((dir, store, test_state_machine().await))
    }
}
#[test]
fn upstream_openraft_storage_suite() {
    Suite::test_all(Builder).expect("upstream OpenRaft storage contract");
}

#[tokio::test]
async fn vote_membership_truncation_purge_and_commit_survive_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path()).await.unwrap();
    store.save_vote(&Vote::new_committed(3, 1)).await.unwrap();
    let membership = Membership::new(vec![BTreeSet::from([1, 2, 3])], None);
    store
        .blocking_append([
            entry(3, 0),
            Entry { log_id: id(3, 1), payload: EntryPayload::Membership(membership.clone()) },
            entry(3, 2),
            entry(3, 3),
        ])
        .await
        .unwrap();
    store.save_committed(Some(id(3, 1))).await.unwrap();
    drop(store);
    let mut store = open(dir.path()).await.unwrap();
    assert_eq!(store.read_vote().await.unwrap(), Some(Vote::new_committed(3, 1)));
    assert_eq!(store.read_committed().await.unwrap(), Some(id(3, 1)));
    assert!(
        matches!(&store.try_get_log_entries(1..2).await.unwrap()[0].payload, EntryPayload::Membership(m) if m == &membership)
    );
    store.truncate(id(3, 2)).await.unwrap();
    drop(store);
    let mut store = open(dir.path()).await.unwrap();
    assert_eq!(store.get_log_state().await.unwrap().last_log_id, Some(id(3, 1)));
    store.blocking_append([entry(4, 2), entry(4, 3)]).await.unwrap();
    store.purge(id(3, 1)).await.unwrap();
    drop(store);
    let mut store = open(dir.path()).await.unwrap();
    assert_eq!(store.get_log_state().await.unwrap().last_purged_log_id, Some(id(3, 1)));
    assert_eq!(
        store
            .try_get_log_entries(..)
            .await
            .unwrap()
            .iter()
            .map(|e| e.log_id)
            .collect::<Vec<_>>(),
        [id(4, 2), id(4, 3)]
    );
    store.purge(id(4, 3)).await.unwrap();
    drop(store);
    let mut store = open(dir.path()).await.unwrap();
    assert_eq!(store.get_log_state().await.unwrap().last_log_id, Some(id(4, 3)));
    store.blocking_append([entry(5, 4)]).await.unwrap();
    assert!(store.try_get_log_entries(..4).await.unwrap().is_empty());
}

#[tokio::test]
async fn uncommitted_entries_are_not_applied_on_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path()).await.unwrap();
    store.save_vote(&Vote::new_committed(1, 1)).await.unwrap();
    store.blocking_append([entry(1, 0), entry(1, 1), entry(1, 2)]).await.unwrap();
    store.save_committed(Some(id(1, 1))).await.unwrap();
    drop(store);
    let mut store = open(dir.path()).await.unwrap();
    let mut sm = test_state_machine().await;
    StorageHelper::new(&mut store, &mut sm).get_initial_state().await.unwrap();
    assert_eq!(sm.applied_state().await.unwrap().0, Some(id(1, 1)));
    assert_eq!(store.get_log_state().await.unwrap().last_log_id, Some(id(1, 2)));
}

#[tokio::test]
async fn rejects_holes_backwards_votes_and_conflicting_committed_truncation() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path()).await.unwrap();
    store.save_vote(&Vote::new_committed(3, 1)).await.unwrap();
    assert!(store.save_vote(&Vote::new(2, 1)).await.is_err());
    assert!(store.blocking_append([entry(3, 0), entry(3, 2)]).await.is_err());
    store.blocking_append([entry(3, 0)]).await.unwrap();
    assert!(store.blocking_append([entry(3, 2)]).await.is_err());
    store.save_committed(Some(id(3, 0))).await.unwrap();
    assert!(store.truncate(id(3, 0)).await.is_err());
    assert!(store.blocking_append([entry(4, 0)]).await.is_err());
    assert!(store.blocking_append((1..=4097).map(|n| entry(3, n))).await.is_err());
    assert_eq!(store.get_log_state().await.unwrap().last_log_id, Some(id(3, 0)));
    assert!(store
        .try_get_log_entries((std::ops::Bound::Excluded(u64::MAX), std::ops::Bound::Unbounded))
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn same_directory_is_exclusive_until_all_handles_close() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let mut store = open(a.path()).await.unwrap();
    let reader = store.get_log_reader().await;
    assert!(open(a.path()).await.is_err());
    let mut other = open(b.path()).await.unwrap();
    store.save_vote(&Vote::new(3, 1)).await.unwrap();
    assert!(other.read_vote().await.unwrap().is_none());
    drop(store);
    assert!(open(a.path()).await.is_err());
    drop(reader);
    open(a.path()).await.unwrap();
}

#[tokio::test]
async fn cancelled_caller_does_not_cancel_an_admitted_write() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path()).await.unwrap();
    let (reached_tx, reached_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    store.storage().await.pause_at(Stage::JournalSync, reached_tx, release_rx).await;
    let mut writer = store.clone();
    let task = tokio::spawn(async move { writer.blocking_append([entry(1, 0)]).await });
    tokio::time::timeout(std::time::Duration::from_secs(10), reached_rx)
        .await
        .unwrap()
        .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    release_tx.send(()).unwrap();
    let mut reader = store.storage().await.clone();
    assert_eq!(reader.try_get_log_entries(..).await.unwrap()[0].log_id, id(1, 0));
    drop(reader);
    drop(store);
    assert_eq!(
        open(dir.path()).await.unwrap().get_log_state().await.unwrap().last_log_id,
        Some(id(1, 0))
    );
}

#[derive(Debug, Clone, serde::Deserialize)]
struct PanickingPayload;
impl serde::Serialize for PanickingPayload {
    fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        panic!("application serializer failed")
    }
}
openraft::declare_raft_types!(PanicConfig: D = PanickingPayload, R = (), SnapshotData = std::io::Cursor<Vec<u8>>);

#[tokio::test]
async fn serializer_panic_invalidates_handles_without_losing_acknowledged_data() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = DurableLog::<PanicConfig>::open(dir.path()).await.unwrap().into_log_store();
    store.save_vote(&Vote::new(1, 1)).await.unwrap();
    assert!(store
        .blocking_append([Entry {
            log_id: id(1, 0),
            payload: EntryPayload::Normal(PanickingPayload)
        }])
        .await
        .is_err());
    assert!(store.read_vote().await.is_err());
    drop(store);
    let mut store = DurableLog::<PanicConfig>::open(dir.path()).await.unwrap().into_log_store();
    assert_eq!(store.read_vote().await.unwrap(), Some(Vote::new(1, 1)));
    assert!(store.try_get_log_entries(..).await.unwrap().is_empty());
}

#[tokio::test]
async fn real_metadata_replace_error_is_propagated() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path()).await.unwrap();
    store.save_vote(&Vote::new(1, 1)).await.unwrap();
    let meta = dir.path().join("durable.meta");
    let backup = dir.path().join("saved.meta");
    std::fs::rename(&meta, &backup).unwrap();
    std::fs::create_dir(&meta).unwrap();
    assert!(store.blocking_append([entry(1, 0)]).await.is_err());
    assert!(store.read_vote().await.is_err());
    drop(store);
    std::fs::remove_dir(&meta).unwrap();
    std::fs::rename(&backup, &meta).unwrap();
    let mut store = open(dir.path()).await.unwrap();
    assert_eq!(store.read_vote().await.unwrap(), Some(Vote::new(1, 1)));
    assert!(store.try_get_log_entries(..).await.unwrap().is_empty());
}

const STAGES: [Stage; 7] = [
    Stage::PartialWrite,
    Stage::JournalSync,
    Stage::MetadataWrite,
    Stage::MetadataSync,
    Stage::MetadataRename,
    Stage::DirectorySync,
    Stage::Complete,
];

#[tokio::test]
async fn io_failures_never_acknowledge_and_poison_every_handle_until_reopen() {
    for stage in STAGES {
        let dir = tempfile::tempdir().unwrap();
        let mut store = open(dir.path()).await.unwrap();
        store.save_vote(&Vote::new(1, 1)).await.unwrap();
        store.blocking_append([entry(1, 0)]).await.unwrap();
        let mut reader = store.get_log_reader().await;
        store.storage().await.inject_failure(stage, false).await;
        assert!(store.blocking_append([entry(1, 1)]).await.is_err(), "{stage:?}");
        assert!(reader.try_get_log_entries(..).await.is_err());
        assert!(store.save_vote(&Vote::new(2, 1)).await.is_err());
        drop(reader);
        drop(store);
        let mut store = open(dir.path()).await.unwrap();
        assert_eq!(store.read_vote().await.unwrap(), Some(Vote::new(1, 1)));
        assert_eq!(store.try_get_log_entries(..1).await.unwrap()[0].log_id, id(1, 0));
        let after_rename = matches!(stage, Stage::DirectorySync | Stage::Complete);
        assert_eq!(indices(&mut store).await.len(), if after_rename { 2 } else { 1 }, "{stage:?}");
        store
            .blocking_append([entry(2, if after_rename { 2 } else { 1 })])
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn corruption_and_missing_durable_bytes_fail_closed() {
    for damage in ["checksum", "length", "header", "metadata", "missing", "short"] {
        let dir = tempfile::tempdir().unwrap();
        let mut store = open(dir.path()).await.unwrap();
        store.save_vote(&Vote::new(1, 1)).await.unwrap();
        drop(store);
        let path = dir.path().join(if damage == "metadata" || damage == "missing" {
            "durable.meta"
        } else {
            "journal"
        });
        if damage == "missing" {
            std::fs::remove_file(&path).unwrap();
        } else {
            let mut file = OpenOptions::new().read(true).write(true).open(&path).unwrap();
            match damage {
                "length" => {
                    file.seek(SeekFrom::Start(24)).unwrap();
                    file.write_all(&u32::MAX.to_le_bytes()).unwrap();
                }
                "short" => file.set_len(25).unwrap(),
                _ => {
                    let at = if damage == "checksum" { 30 } else { 0 };
                    file.seek(SeekFrom::Start(at)).unwrap();
                    let mut byte = [0];
                    file.read_exact(&mut byte).unwrap();
                    file.seek(SeekFrom::Start(at)).unwrap();
                    file.write_all(&[byte[0] ^ 0xff]).unwrap();
                }
            }
            file.sync_all().unwrap();
        }
        assert!(open(dir.path()).await.is_err(), "{damage}");
    }
}

#[tokio::test]
async fn legacy_or_mismatched_files_are_never_adopted() {
    let legacy = tempfile::tempdir().unwrap();
    std::fs::write(legacy.path().join("events.raftlog"), b"old events").unwrap();
    assert!(open(legacy.path()).await.is_err());
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    drop(open(a.path()).await.unwrap());
    drop(open(b.path()).await.unwrap());
    std::fs::copy(a.path().join("durable.meta"), b.path().join("durable.meta")).unwrap();
    assert!(open(b.path()).await.is_err());
}

#[tokio::test]
async fn unsupported_metadata_version_is_rejected_even_with_valid_checksum() {
    let dir = tempfile::tempdir().unwrap();
    drop(open(dir.path()).await.unwrap());
    let path = dir.path().join("durable.meta");
    let bytes = std::fs::read(&path).unwrap();
    let mut metadata: serde_json::Value =
        serde_json::from_slice(&bytes[4..bytes.len() - 4]).unwrap();
    metadata["version"] = 2.into();
    let body = serde_json::to_vec(&metadata).unwrap();
    let mut frame = (body.len() as u32).to_le_bytes().to_vec();
    frame.extend_from_slice(&body);
    frame.extend_from_slice(&crc32fast::hash(&frame).to_le_bytes());
    std::fs::write(path, frame).unwrap();
    assert!(open(dir.path()).await.is_err());
}

#[tokio::test]
async fn normal_payload_and_size_limit() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path()).await.unwrap();
    let request =
        |status| openraft_memstore::ClientRequest { client: "client-a".into(), serial: 42, status };
    store
        .blocking_append([Entry {
            log_id: id(1, 0),
            payload: EntryPayload::Normal(request("saved".into())),
        }])
        .await
        .unwrap();
    assert!(store
        .blocking_append([Entry {
            log_id: id(1, 1),
            payload: EntryPayload::Normal(request("x".repeat(8 * 1024 * 1024)))
        }])
        .await
        .is_err());
    store.save_vote(&Vote::new(1, 1)).await.unwrap();
    drop(store);
    let mut store = open(dir.path()).await.unwrap();
    let entries = store.try_get_log_entries(..).await.unwrap();
    assert_eq!(entries.len(), 1);
    assert!(
        matches!(&entries[0].payload, EntryPayload::Normal(data) if data.status == "saved" && data.serial == 42 && data.client == "client-a")
    );
    assert!(
        store.applied_state().await.is_err(),
        "log handle must not masquerade as a state machine"
    );
}

#[tokio::test]
async fn compacted_entries_cannot_be_resurrected() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path()).await.unwrap();
    store.blocking_append([entry(1, 0), entry(1, 1)]).await.unwrap();
    store.purge(id(1, 1)).await.unwrap();
    store.blocking_append([entry(1, 0), entry(1, 1), entry(1, 2)]).await.unwrap();
    assert!(store.blocking_append([entry(1, 2), entry(1, 0)]).await.is_err());
    drop(store);
    assert_eq!(indices(&mut open(dir.path()).await.unwrap()).await, [2]);
}

// Exit inside an operation, without running store/tempfile destructors.
#[test]
fn crash_child() {
    let Ok(path) = std::env::var("LITHAIR_STORAGE_CRASH_DIR") else {
        return;
    };
    let stage: usize = std::env::var("LITHAIR_STORAGE_CRASH_STAGE").unwrap().parse().unwrap();
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let mut store = open(path).await.unwrap();
        store.storage().await.inject_failure(STAGES[stage], true).await;
        match std::env::var("LITHAIR_STORAGE_CRASH_OPERATION").unwrap().as_str() {
            "append" => store.blocking_append([entry(1, 3)]).await.unwrap(),
            "vote" => store.save_vote(&Vote::new_committed(2, 1)).await.unwrap(),
            "truncate" => store.truncate(id(1, 1)).await.unwrap(),
            "purge" => store.purge(id(1, 1)).await.unwrap(),
            _ => panic!("unexpected operation"),
        }
        panic!("crash point was not reached");
    });
}

#[tokio::test]
async fn process_death_recovers_an_atomic_operation_at_each_boundary() {
    for operation in ["append", "vote", "truncate", "purge"] {
        for (n, stage) in STAGES.into_iter().enumerate() {
            let dir = tempfile::tempdir().unwrap();
            let mut store = open(dir.path()).await.unwrap();
            store.save_vote(&Vote::new_committed(1, 1)).await.unwrap();
            store.blocking_append([entry(1, 0), entry(1, 1), entry(1, 2)]).await.unwrap();
            drop(store);
            let mut child = tokio::process::Command::new(std::env::current_exe().unwrap());
            let output = child
                .args(["--exact", "crash_child", "--nocapture"])
                .env("LITHAIR_STORAGE_CRASH_DIR", dir.path())
                .env("LITHAIR_STORAGE_CRASH_STAGE", n.to_string())
                .env("LITHAIR_STORAGE_CRASH_OPERATION", operation)
                .kill_on_drop(true)
                .output();
            let output = tokio::time::timeout(std::time::Duration::from_secs(10), output)
                .await
                .expect("child deadline")
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(86),
                "{operation} at {stage:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let mut store = open(dir.path()).await.unwrap();
            let replaced = matches!(stage, Stage::DirectorySync | Stage::Complete);
            let vote = if operation == "vote" && replaced {
                Vote::new_committed(2, 1)
            } else {
                Vote::new_committed(1, 1)
            };
            assert_eq!(store.read_vote().await.unwrap(), Some(vote));
            let expected = match (operation, replaced) {
                ("append", true) => vec![0, 1, 2, 3],
                ("truncate", true) => vec![0],
                ("purge", true) => vec![2],
                _ => vec![0, 1, 2],
            };
            assert_eq!(indices(&mut store).await, expected, "{operation} at {stage:?}");
            let purged = if operation == "purge" && replaced { Some(id(1, 1)) } else { None };
            assert_eq!(store.get_log_state().await.unwrap().last_purged_log_id, purged);
        }
    }
}
