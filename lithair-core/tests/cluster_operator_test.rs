#![cfg(all(feature = "cluster", feature = "tls"))]
use lithair_core::cluster::operator::{run, OperatorCommand};
#[path = "../src/cluster/durable_log/mod.rs"]
#[allow(dead_code)]
mod durable_log;
#[path = "support/operator_files.rs"]
mod fixture;

#[tokio::test]
async fn check_provision_and_read_only_inspection_use_the_same_identity() {
    let files = fixture::OperatorFiles::new();
    let config = &files.configs[&1];
    let checked = run(OperatorCommand::Check, config).await.unwrap();
    assert!(!files.data(1).exists());
    let provisioned = run(OperatorCommand::Provision, config).await.unwrap();
    assert_eq!(checked["plan_sha256"], provisioned["plan_sha256"]);
    assert!(run(OperatorCommand::Provision, config).await.is_err());
    let inspection = run(OperatorCommand::Inspect, config).await.unwrap();
    assert_eq!(inspection["store"]["bootstrap_claimed"], false);
    assert_eq!(inspection["store"]["pristine"], true);
}

use std::{collections::BTreeMap, io::Write, path::Path};
fn bytes(path: &Path) -> BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(path)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (entry.file_name().to_str().unwrap().into(), std::fs::read(entry.path()).unwrap())
        })
        .collect()
}
#[tokio::test]
async fn inspection_keeps_uncommitted_tail_orphans_and_bootstrap_claim_untouched() {
    let files = fixture::OperatorFiles::new();
    run(OperatorCommand::Provision, &files.configs[&1]).await.unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(files.data(1).join("journal"))
        .unwrap()
        .write_all(b"tail")
        .unwrap();
    std::fs::write(files.data(1).join("manifest-00000000000000000000000000000001"), b"orphan")
        .unwrap();
    let before = bytes(&files.data(1));
    let report = run(OperatorCommand::Inspect, &files.configs[&1]).await.unwrap();
    assert_eq!(report["store"]["ignored_tail_bytes"], 4);
    assert_eq!(bytes(&files.data(1)), before);
    let lock = std::fs::File::open(files.data(1).join("LOCK")).unwrap();
    lock.try_lock().unwrap();
    assert!(run(OperatorCommand::Inspect, &files.configs[&1]).await.is_err());
    lock.unlock().unwrap();
    assert_eq!(bytes(&files.data(1)), before);
    std::fs::write(files.data(1).join("durable.meta"), b"corrupt").unwrap();
    let before = bytes(&files.data(1));
    assert!(run(OperatorCommand::Inspect, &files.configs[&1]).await.is_err());
    assert_eq!(bytes(&files.data(1)), before);
}
#[tokio::test]
async fn shared_digest_excludes_local_node_and_addresses_but_covers_bootstrap_and_enrollment() {
    let files = fixture::OperatorFiles::new();
    let a = run(OperatorCommand::Check, &files.configs[&1]).await.unwrap();
    let path = &files.configs[&2];
    let b = run(OperatorCommand::Check, path).await.unwrap();
    assert_eq!(a["plan_sha256"], b["plan_sha256"]);
    let original = std::fs::read_to_string(path).unwrap();
    std::fs::write(path, original.replace("127.0.0.1:20001", "127.0.0.1:29999")).unwrap();
    assert_eq!(
        a["plan_sha256"],
        run(OperatorCommand::Check, path).await.unwrap()["plan_sha256"]
    );
    std::fs::write(path, original.replace("bootstrap_node = 1", "bootstrap_node = 2")).unwrap();
    assert_ne!(
        a["plan_sha256"],
        run(OperatorCommand::Check, path).await.unwrap()["plan_sha256"]
    );
}
#[tokio::test]
async fn malformed_or_oversized_configuration_cannot_create_storage() {
    for mode in ["version", "unknown", "duplicate", "fingerprint", "address", "name", "size"] {
        let files = fixture::OperatorFiles::new();
        let path = &files.configs[&1];
        let original = std::fs::read_to_string(path).unwrap();
        let changed = match mode {
            "version" => original.replace("version = 1", "version = 99"),
            "unknown" => format!("unexpected = 'secret-value'\n{original}"),
            "duplicate" => original.replace("node_id = 3", "node_id = 2"),
            "fingerprint" => {
                original.replace("certificate_sha256 = \"", "certificate_sha256 = \"x")
            }
            "address" => original.replace("127.0.0.1:20001", "0.0.0.0:0"),
            "name" => original.replace("node1.test", "wrong.test"),
            _ => "x".repeat(64 * 1024 + 1),
        };
        std::fs::write(path, changed).unwrap();
        assert!(run(OperatorCommand::Provision, path).await.is_err(), "{mode}");
        assert!(!files.data(1).exists());
    }
}
#[tokio::test]
async fn invalid_tls_material_cannot_create_storage_or_echo_secrets() {
    for mode in [
        "key",
        "key-header",
        "key-size",
        "cert",
        "cert-size",
        "wrong-key",
        "wrong-ca",
        "multiple-keys",
    ] {
        let files = fixture::OperatorFiles::new();
        let root = files.root.path();
        let key = root.join("1/key.pem");
        let cert = root.join("1/cert.pem");
        match mode {
            "key" => std::fs::write(key, b"PRIVATE_TEST_SECRET").unwrap(),
            "key-header" => std::fs::write(key, b"-----BEGIN PRIVATE_TEST_SECRET\n").unwrap(),
            "key-size" => std::fs::write(key, vec![b'x'; 64 * 1024 + 1]).unwrap(),
            "cert" => std::fs::write(cert, b"INVALID_CERTIFICATE").unwrap(),
            "cert-size" => std::fs::write(cert, vec![b'x'; 1024 * 1024 + 1]).unwrap(),
            "wrong-key" => {
                std::fs::copy(root.join("2/key.pem"), key).unwrap();
            }
            "wrong-ca" => {
                std::fs::copy(root.join("2/cert.pem"), root.join("ca.pem")).unwrap();
            }
            _ => {
                let content = std::fs::read_to_string(&key).unwrap();
                std::fs::write(key, content.repeat(2)).unwrap();
            }
        }
        let error = run(OperatorCommand::Provision, &files.configs[&1]).await.unwrap_err();
        assert!(!format!("{error:#}").contains("PRIVATE_TEST_SECRET"));
        assert!(!files.data(1).exists(), "{mode}");
    }
}
#[tokio::test]
async fn offline_recovery_never_adopts_a_different_identity_or_a_missing_store() {
    let files = fixture::OperatorFiles::new();
    assert!(run(OperatorCommand::Inspect, &files.configs[&1]).await.is_err());
    assert!(!files.data(1).exists());
    run(OperatorCommand::Provision, &files.configs[&1]).await.unwrap();
    let before = bytes(&files.data(1));
    let path = &files.configs[&1];
    let original = std::fs::read_to_string(path).unwrap();
    std::fs::write(path, original.replace("operator-test", "another-cluster")).unwrap();
    assert!(run(OperatorCommand::Inspect, path).await.is_err());
    assert_eq!(bytes(&files.data(1)), before);
}

#[tokio::test]
async fn inspection_validates_committed_data_snapshots_and_consumed_bootstrap_without_repair() {
    use durable_log::{DurableLog, NodeIdentity};
    use openraft::{
        storage::{RaftLogStorage, RaftLogStorageExt},
        CommittedLeaderId, Entry, EntryPayload, LogId, SnapshotMeta, Vote,
    };
    openraft::declare_raft_types!(TestConfig: D=String, R=(), Node=String, SnapshotData=std::io::Cursor<Vec<u8>>);
    let files = fixture::OperatorFiles::new();
    let path = &files.configs[&1];
    run(OperatorCommand::Provision, path).await.unwrap();
    let config: toml::Value = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let voters = config["peers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|peer| {
            (
                peer["node_id"].as_integer().unwrap() as u64,
                hex::decode(peer["certificate_sha256"].as_str().unwrap())
                    .unwrap()
                    .try_into()
                    .unwrap(),
            )
        })
        .collect();
    let identity =
        NodeIdentity { cluster_id: "operator-test".into(), node_id: 1, bootstrap_node: 1, voters };
    let store = DurableLog::<TestConfig>::open_for_node(files.data(1), identity).await.unwrap();
    store.claim_bootstrap().await.unwrap();
    let mut log = store.clone().into_log_store();
    let log_id = LogId::new(CommittedLeaderId::new(1, 1), 0);
    log.save_vote(&Vote::new_committed(1, 1)).await.unwrap();
    log.blocking_append([Entry { log_id, payload: EntryPayload::Normal("opaque command".into()) }])
        .await
        .unwrap();
    log.save_committed(Some(log_id)).await.unwrap();
    store
        .save_snapshot(
            SnapshotMeta { last_log_id: Some(log_id), ..Default::default() },
            b"opaque state".to_vec(),
        )
        .await
        .unwrap();
    store.compact().await.unwrap();
    drop(log);
    drop(store);
    let before = bytes(&files.data(1));
    let report = run(OperatorCommand::Inspect, path).await.unwrap();
    assert_eq!(report["store"]["bootstrap_claimed"], true);
    assert_eq!(report["store"]["pristine"], false);
    assert_eq!(report["store"]["retained_entries"], 1);
    assert_eq!(report["store"]["commit_index"], 0);
    assert_eq!(report["store"]["snapshot_index"], 0);
    assert_eq!(bytes(&files.data(1)), before);
    for prefix in ["journal-", "snapshot-"] {
        let (name, original) = before.iter().find(|(name, _)| name.starts_with(prefix)).unwrap();
        let mut damaged = original.clone();
        let last = damaged.len() - 1;
        damaged[last] ^= 1;
        std::fs::write(files.data(1).join(name), &damaged).unwrap();
        let damaged_files = bytes(&files.data(1));
        assert!(run(OperatorCommand::Inspect, path).await.is_err(), "{prefix}");
        assert_eq!(bytes(&files.data(1)), damaged_files);
        std::fs::write(files.data(1).join(name), original).unwrap();
    }
}
