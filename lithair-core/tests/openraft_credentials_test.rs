#![cfg(feature = "cluster")]
#[path = "../src/cluster/durable_log/mod.rs"]
#[allow(dead_code)]
mod durable_log;

use durable_log::{credentials::PendingCertificate, CredentialPolicy, DurableLog, NodeIdentity};
use openraft::{
    storage::{RaftLogStorage, RaftLogStorageExt},
    CommittedLeaderId, Entry, EntryPayload, LogId, Vote,
};
use openraft_memstore::TypeConfig;
use std::{collections::BTreeMap, io::Write, path::Path};

fn identity() -> NodeIdentity {
    NodeIdentity {
        cluster_id: "rotation".into(),
        node_id: 1,
        bootstrap_node: 1,
        voters: [(1, [1; 32]), (2, [2; 32]), (3, [3; 32])].into(),
    }
}
fn overlap() -> CredentialPolicy {
    CredentialPolicy {
        generation: 1,
        pending: Some(PendingCertificate { node_id: 2, certificate_sha256: [4; 32] }),
        ..CredentialPolicy::initial(&identity())
    }
}
fn retired() -> CredentialPolicy {
    let mut current = identity().voters;
    current.insert(2, [4; 32]);
    CredentialPolicy { generation: 2, current, pending: None }
}
fn files(path: &Path) -> BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(path)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            (e.file_name().to_str().unwrap().into(), std::fs::read(e.path()).unwrap())
        })
        .collect()
}

#[tokio::test]
async fn rotation_preserves_consensus_binding_bootstrap_and_compacted_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let store = DurableLog::<TypeConfig>::create_for_node(dir.path(), identity()).await.unwrap();
    store.claim_bootstrap().await.unwrap();
    let mut log = store.clone().into_log_store();
    log.save_vote(&Vote::new(3, 1)).await.unwrap();
    let id = LogId::new(CommittedLeaderId::new(3, 1), 0);
    log.blocking_append([Entry { log_id: id, payload: EntryPayload::Blank }])
        .await
        .unwrap();
    log.save_committed(Some(id)).await.unwrap();
    drop(log);
    drop(store);
    let before = DurableLog::<TypeConfig>::inspect_for_node(dir.path(), identity())
        .await
        .unwrap();
    let bytes = std::fs::read(dir.path().join("journal")).unwrap();
    for (generation, policy) in [(0, overlap()), (1, retired())] {
        DurableLog::<TypeConfig>::transition_credentials(
            dir.path(),
            identity(),
            generation,
            policy.clone(),
        )
        .await
        .unwrap();
        assert!(DurableLog::<TypeConfig>::open_for_node(dir.path(), identity()).await.is_err());
        assert_eq!(std::fs::read(dir.path().join("journal")).unwrap(), bytes);
        let store =
            DurableLog::<TypeConfig>::open_with_credentials(dir.path(), identity(), policy.clone())
                .await
                .unwrap();
        assert_eq!(store.node_identity().await.unwrap(), identity());
        assert_eq!(store.credential_policy().await.unwrap(), policy);
        assert!(store.claim_bootstrap().await.is_err());
        let mut log = store.clone().into_log_store();
        assert_eq!(log.read_vote().await.unwrap(), Some(Vote::new(3, 1)));
        assert_eq!(log.read_committed().await.unwrap(), Some(id));
        drop(log);
        drop(store);
    }
    let store = DurableLog::<TypeConfig>::open_with_credentials(dir.path(), identity(), retired())
        .await
        .unwrap();
    store.compact().await.unwrap();
    store.save_snapshot(Default::default(), vec![42]).await.unwrap();
    drop(store);
    let report = DurableLog::<TypeConfig>::inspect_for_node(dir.path(), identity())
        .await
        .unwrap();
    assert_eq!(report["store_id"], before["store_id"]);
    assert_eq!(report["bootstrap_claimed"], true);
    assert_eq!(report["credentials"]["generation"], 2);
    let store = DurableLog::<TypeConfig>::open_with_credentials(dir.path(), identity(), retired())
        .await
        .unwrap();
    assert_eq!(store.credential_policy().await.unwrap(), retired());
}

#[tokio::test]
async fn invalid_transitions_and_stale_recovery_leave_all_bytes_untouched() {
    let dir = tempfile::tempdir().unwrap();
    drop(DurableLog::<TypeConfig>::create_for_node(dir.path(), identity()).await.unwrap());
    std::fs::OpenOptions::new()
        .append(true)
        .open(dir.path().join("journal"))
        .unwrap()
        .write_all(b"unacknowledged tail")
        .unwrap();
    std::fs::write(dir.path().join("manifest-00000000000000000000000000000001"), b"orphan")
        .unwrap();
    let before = files(dir.path());
    for mode in 0..8 {
        let mut next = overlap();
        let mut expected = 0;
        match mode {
            0 => expected = 1,
            1 => next = retired(),
            2 => next.generation = 3,
            3 => next.pending.as_mut().unwrap().node_id = 4,
            4 => next.pending.as_mut().unwrap().certificate_sha256 = [1; 32],
            5 => {
                next.current.insert(1, [5; 32]);
            }
            6 => {
                next.current.remove(&3);
            }
            _ => next.pending = None,
        }
        assert!(
            DurableLog::<TypeConfig>::transition_credentials(
                dir.path(),
                identity(),
                expected,
                next
            )
            .await
            .is_err(),
            "{mode}"
        );
        assert_eq!(files(dir.path()), before, "{mode}");
    }
    DurableLog::<TypeConfig>::transition_credentials(dir.path(), identity(), 0, overlap())
        .await
        .unwrap();
    let after = files(dir.path());
    assert_eq!(before["journal"], after["journal"]);
    for policy in [CredentialPolicy::initial(&identity()), retired()] {
        assert!(DurableLog::<TypeConfig>::open_with_credentials(dir.path(), identity(), policy)
            .await
            .is_err());
        assert_eq!(files(dir.path()), after);
    }
    assert!(
        DurableLog::<TypeConfig>::transition_credentials(dir.path(), identity(), 0, overlap())
            .await
            .is_err()
    );
    let mut wrong = retired();
    wrong.current.insert(2, [2; 32]);
    assert!(
        DurableLog::<TypeConfig>::transition_credentials(dir.path(), identity(), 1, wrong)
            .await
            .is_err()
    );
    assert_eq!(files(dir.path()), after);
}

#[tokio::test]
async fn rotation_requires_an_offline_store_and_serializes_competing_operators() {
    let dir = tempfile::tempdir().unwrap();
    let store = DurableLog::<TypeConfig>::create_for_node(dir.path(), identity()).await.unwrap();
    assert!(
        DurableLog::<TypeConfig>::transition_credentials(dir.path(), identity(), 0, overlap())
            .await
            .is_err()
    );
    drop(store);
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let path = dir.path().to_owned();
        tasks.spawn(async move {
            DurableLog::<TypeConfig>::transition_credentials(path, identity(), 0, overlap())
                .await
                .is_ok()
        });
    }
    let mut accepted = 0;
    while let Some(result) = tasks.join_next().await {
        accepted += usize::from(result.unwrap());
    }
    assert_eq!(accepted, 1);
}

#[tokio::test]
async fn malformed_version_four_manifests_cannot_downgrade_authorization() {
    for mode in ["missing", "duplicate", "foreign", "generation", "version"] {
        let dir = tempfile::tempdir().unwrap();
        drop(DurableLog::<TypeConfig>::create_for_node(dir.path(), identity()).await.unwrap());
        DurableLog::<TypeConfig>::transition_credentials(dir.path(), identity(), 0, overlap())
            .await
            .unwrap();
        let path = dir.path().join("durable.meta");
        let frame = std::fs::read(&path).unwrap();
        let mut body: serde_json::Value =
            serde_json::from_slice(&frame[4..frame.len() - 4]).unwrap();
        match mode {
            "missing" => {
                body.as_object_mut().unwrap().remove("credentials");
            }
            "duplicate" => body["credentials"]["current"]["2"] = serde_json::json!(vec![1; 32]),
            "foreign" => body["credentials"]["pending"]["node_id"] = serde_json::json!(4),
            "generation" => body["credentials"]["generation"] = serde_json::json!(2),
            _ => body["version"] = serde_json::json!(3),
        }
        let bytes = serde_json::to_vec(&body).unwrap();
        let mut frame = (bytes.len() as u32).to_le_bytes().to_vec();
        frame.extend(bytes);
        frame.extend(crc32fast::hash(&frame).to_le_bytes());
        std::fs::write(path, frame).unwrap();
        let before = files(dir.path());
        assert!(
            DurableLog::<TypeConfig>::open_with_credentials(dir.path(), identity(), overlap())
                .await
                .is_err(),
            "{mode}"
        );
        assert!(
            DurableLog::<TypeConfig>::inspect_for_node(dir.path(), identity())
                .await
                .is_err(),
            "{mode}"
        );
        assert_eq!(files(dir.path()), before);
    }
}
