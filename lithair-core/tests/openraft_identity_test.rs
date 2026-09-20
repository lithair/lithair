#![cfg(feature = "cluster")]
#[path = "../src/cluster/durable_log/mod.rs"]
#[allow(dead_code)]
mod durable_log;

use durable_log::{DurableLog, NodeIdentity};
use openraft_memstore::TypeConfig;

fn identity(node_id: u64) -> NodeIdentity {
    NodeIdentity {
        cluster_id: "identity-test".into(),
        node_id,
        bootstrap_node: 1,
        voters: [(1, [1; 32]), (2, [2; 32]), (3, [3; 32])].into(),
    }
}

#[tokio::test]
async fn restart_requires_the_original_node_and_cluster_identity() {
    let dir = tempfile::tempdir().unwrap();
    let store = DurableLog::<TypeConfig>::create_for_node(dir.path(), identity(1))
        .await
        .unwrap();
    drop(store);
    assert!(DurableLog::<TypeConfig>::open_for_node(dir.path(), identity(2)).await.is_err());
    let mut wrong = identity(1);
    wrong.cluster_id = "another-cluster".into();
    assert!(DurableLog::<TypeConfig>::open_for_node(dir.path(), wrong).await.is_err());
    assert!(DurableLog::<TypeConfig>::open(dir.path()).await.is_err());
    let store = DurableLog::<TypeConfig>::open_for_node(dir.path(), identity(1)).await.unwrap();
    assert_eq!(store.node_identity().await.unwrap(), identity(1));
}

#[tokio::test]
async fn only_the_designated_node_can_claim_bootstrap_once_across_restart() {
    for node in [1, 2, 3] {
        let dir = tempfile::tempdir().unwrap();
        let store = DurableLog::<TypeConfig>::create_for_node(dir.path(), identity(node))
            .await
            .unwrap();
        assert_eq!(store.claim_bootstrap().await.is_ok(), node == 1);
        assert!(store.claim_bootstrap().await.is_err());
        drop(store);
        let store = DurableLog::<TypeConfig>::open_for_node(dir.path(), identity(node))
            .await
            .unwrap();
        assert!(store.claim_bootstrap().await.is_err());
    }
}

use durable_log::Stage;
use openraft::{
    storage::{RaftLogStorage, RaftLogStorageExt},
    CommittedLeaderId, Entry, EntryPayload, LogId, SnapshotMeta, Vote,
};
use std::{collections::BTreeMap, io::Write, path::Path, time::Duration};

fn files(path: &Path) -> BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(path)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().to_str().unwrap().to_owned(),
                std::fs::read(entry.path()).unwrap(),
            )
        })
        .collect()
}
fn id() -> LogId<u64> {
    LogId::new(CommittedLeaderId::new(1, 1), 0)
}

#[tokio::test]
async fn identity_drift_is_rejected_before_tail_truncation_or_generation_cleanup() {
    let dir = tempfile::tempdir().unwrap();
    let store = DurableLog::<TypeConfig>::create_for_node(dir.path(), identity(1))
        .await
        .unwrap();
    drop(store);
    std::fs::OpenOptions::new()
        .append(true)
        .open(dir.path().join("journal"))
        .unwrap()
        .write_all(b"unacknowledged tail")
        .unwrap();
    std::fs::write(dir.path().join("manifest-00000000000000000000000000000001"), b"orphan")
        .unwrap();
    let original = files(dir.path());
    for mode in 0..5 {
        let mut wrong = identity(1);
        match mode {
            0 => wrong.node_id = 2,
            1 => wrong.cluster_id = "another-cluster".into(),
            2 => wrong.bootstrap_node = 2,
            3 => {
                wrong.voters.insert(2, [4; 32]);
            }
            _ => {
                let previous = wrong.voters.remove(&3).unwrap();
                wrong.voters.insert(4, previous);
            }
        }
        assert!(DurableLog::<TypeConfig>::open_for_node(dir.path(), wrong).await.is_err());
        assert_eq!(files(dir.path()), original, "configuration mismatch mutated disk");
    }
    let store = DurableLog::<TypeConfig>::open_for_node(dir.path(), identity(1)).await.unwrap();
    assert_eq!(store.node_identity().await.unwrap(), identity(1));
    assert_eq!(std::fs::metadata(dir.path().join("journal")).unwrap().len(), 24);
    assert_eq!(files(dir.path()).len(), 3);
}

#[tokio::test]
async fn invalid_enrollment_and_unbound_stores_are_never_adopted() {
    for mode in 0..6 {
        let dir = tempfile::tempdir().unwrap();
        let mut invalid = identity(1);
        match mode {
            0 => invalid.cluster_id.clear(),
            1 => invalid.cluster_id = "x".repeat(129),
            2 => invalid.node_id = 4,
            3 => invalid.bootstrap_node = 4,
            4 => {
                invalid.voters.remove(&3);
            }
            _ => {
                invalid.voters.insert(2, [1; 32]);
            }
        }
        assert!(DurableLog::<TypeConfig>::create_for_node(dir.path(), invalid).await.is_err());
        assert!(files(dir.path()).is_empty());
    }
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let store = DurableLog::<TypeConfig>::create(dir.path()).await.unwrap();
        if compact {
            store.compact().await.unwrap();
        }
        assert!(store.claim_bootstrap().await.is_err());
        drop(store);
        let before = files(dir.path());
        assert!(DurableLog::<TypeConfig>::open_for_node(dir.path(), identity(1)).await.is_err());
        assert_eq!(files(dir.path()), before);
        assert!(DurableLog::<TypeConfig>::open(dir.path()).await.is_ok());
    }
    let dir = tempfile::tempdir().unwrap();
    assert!(DurableLog::<TypeConfig>::open_for_node(dir.path(), identity(1)).await.is_err());
    assert!(files(dir.path()).is_empty());
}

#[tokio::test]
async fn bound_manifest_corruption_and_store_swaps_fail_closed() {
    for mode in ["bytes", "missing", "version", "binding", "swapped"] {
        let dir = tempfile::tempdir().unwrap();
        let store = DurableLog::<TypeConfig>::create_for_node(dir.path(), identity(1))
            .await
            .unwrap();
        drop(store);
        let path = dir.path().join("durable.meta");
        match mode {
            "bytes" => {
                std::fs::write(path, b"corrupt").unwrap();
            }
            "missing" => {
                std::fs::remove_file(path).unwrap();
            }
            "swapped" => {
                let other = tempfile::tempdir().unwrap();
                let store = DurableLog::<TypeConfig>::create_for_node(other.path(), identity(1))
                    .await
                    .unwrap();
                drop(store);
                std::fs::copy(other.path().join("durable.meta"), path).unwrap();
            }
            _ => {
                let bytes = std::fs::read(&path).unwrap();
                let mut meta: serde_json::Value =
                    serde_json::from_slice(&bytes[4..bytes.len() - 4]).unwrap();
                if mode == "version" {
                    meta["version"] = serde_json::json!(99);
                } else {
                    meta.as_object_mut().unwrap().remove("node");
                }
                let body = serde_json::to_vec(&meta).unwrap();
                let mut frame = (body.len() as u32).to_le_bytes().to_vec();
                frame.extend(body);
                frame.extend(crc32fast::hash(&frame).to_le_bytes());
                std::fs::write(path, frame).unwrap();
            }
        }
        assert!(
            DurableLog::<TypeConfig>::open_for_node(dir.path(), identity(1)).await.is_err(),
            "{mode}"
        );
    }
}

#[tokio::test]
async fn consensus_history_prevents_bootstrap_and_binding_survives_checkpoints() {
    for mode in ["vote", "log", "commit", "purge", "snapshot"] {
        let dir = tempfile::tempdir().unwrap();
        let store = DurableLog::<TypeConfig>::create_for_node(dir.path(), identity(1))
            .await
            .unwrap();
        let mut log = store.clone().into_log_store();
        match mode {
            "vote" => log.save_vote(&Vote::new(1, 1)).await.unwrap(),
            "log" => log
                .blocking_append([Entry { log_id: id(), payload: EntryPayload::Blank }])
                .await
                .unwrap(),
            "commit" => log.save_committed(Some(id())).await.unwrap(),
            "purge" => log.purge(id()).await.unwrap(),
            _ => store.save_snapshot(SnapshotMeta::default(), vec![]).await.unwrap(),
        }
        assert!(store.claim_bootstrap().await.is_err(), "{mode}");
        store.compact().await.unwrap();
        drop(log);
        drop(store);
        let store = DurableLog::<TypeConfig>::open_for_node(dir.path(), identity(1)).await.unwrap();
        assert_eq!(store.node_identity().await.unwrap(), identity(1));
        assert!(store.claim_bootstrap().await.is_err(), "{mode}");
    }
    let dir = tempfile::tempdir().unwrap();
    let store = DurableLog::<TypeConfig>::create_for_node(dir.path(), identity(1))
        .await
        .unwrap();
    store.compact().await.unwrap();
    assert!(store.claim_bootstrap().await.is_ok());
    store.save_snapshot(SnapshotMeta::default(), vec![]).await.unwrap();
    store.compact().await.unwrap();
    drop(store);
    let store = DurableLog::<TypeConfig>::open_for_node(dir.path(), identity(1)).await.unwrap();
    assert!(store.claim_bootstrap().await.is_err());
    assert_eq!(store.node_identity().await.unwrap(), identity(1));
}

#[tokio::test]
async fn concurrent_bootstrap_claims_have_one_winner() {
    let dir = tempfile::tempdir().unwrap();
    let store = DurableLog::<TypeConfig>::create_for_node(dir.path(), identity(1))
        .await
        .unwrap();
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..16 {
        let store = store.clone();
        tasks.spawn(async move { store.claim_bootstrap().await.is_ok() });
    }
    let mut accepted = 0;
    while let Some(result) = tasks.join_next().await {
        accepted += usize::from(result.unwrap());
    }
    assert_eq!(accepted, 1);
}

const STAGES: [Stage; 5] = [
    Stage::MetadataWrite,
    Stage::MetadataSync,
    Stage::MetadataRename,
    Stage::DirectorySync,
    Stage::Complete,
];

#[tokio::test]
async fn claim_io_failures_poison_handles_and_reopen_resolves_the_boundary() {
    for (index, stage) in STAGES.iter().enumerate() {
        let dir = tempfile::tempdir().unwrap();
        let store = DurableLog::<TypeConfig>::create_for_node(dir.path(), identity(1))
            .await
            .unwrap();
        let other = store.clone();
        store.inject_failure(*stage, false).await;
        assert!(store.claim_bootstrap().await.is_err());
        assert!(other.node_identity().await.is_err());
        drop(other);
        drop(store);
        let store = DurableLog::<TypeConfig>::open_for_node(dir.path(), identity(1)).await.unwrap();
        assert_eq!(store.claim_bootstrap().await.is_ok(), index < 3, "{stage:?}");
    }
}

#[tokio::test]
async fn bootstrap_crash_child() {
    let Some(path) = std::env::var_os("LITHAIR_BOOTSTRAP_CRASH_DIR") else {
        return;
    };
    let index: usize = std::env::var("LITHAIR_BOOTSTRAP_CRASH_STAGE").unwrap().parse().unwrap();
    let store = DurableLog::<TypeConfig>::open_for_node(path, identity(1)).await.unwrap();
    store.inject_failure(STAGES[index], true).await;
    assert!(store.claim_bootstrap().await.is_ok());
    panic!("fault did not terminate process");
}

#[tokio::test]
async fn process_death_cannot_reissue_a_durable_bootstrap_claim() {
    for index in 0..STAGES.len() {
        let dir = tempfile::tempdir().unwrap();
        let store = DurableLog::<TypeConfig>::create_for_node(dir.path(), identity(1))
            .await
            .unwrap();
        drop(store);
        let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "bootstrap_crash_child", "--nocapture"])
            .env("LITHAIR_BOOTSTRAP_CRASH_DIR", dir.path())
            .env("LITHAIR_BOOTSTRAP_CRASH_STAGE", index.to_string())
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
            Some(86)
        );
        let store = DurableLog::<TypeConfig>::open_for_node(dir.path(), identity(1)).await.unwrap();
        assert_eq!(store.node_identity().await.unwrap(), identity(1));
        assert_eq!(store.claim_bootstrap().await.is_ok(), index < 3);
        assert!(store.claim_bootstrap().await.is_err());
    }
}

#[tokio::test]
async fn cancellation_does_not_restore_bootstrap_permission() {
    let dir = tempfile::tempdir().unwrap();
    let store = DurableLog::<TypeConfig>::create_for_node(dir.path(), identity(1))
        .await
        .unwrap();
    let (reached, arrived) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    store.pause_at(Stage::MetadataRename, reached, wait).await;
    let claimed = store.clone();
    let task = tokio::spawn(async move { claimed.claim_bootstrap().await });
    tokio::time::timeout(Duration::from_secs(10), arrived).await.unwrap().unwrap();
    task.abort();
    let _ = task.await;
    release.send(()).unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(10), store.claim_bootstrap())
        .await
        .unwrap()
        .is_err());
    drop(store);
    let store = DurableLog::<TypeConfig>::open_for_node(dir.path(), identity(1)).await.unwrap();
    assert!(store.claim_bootstrap().await.is_err());
}
