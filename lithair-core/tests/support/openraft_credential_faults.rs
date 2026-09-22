use super::*;
// Also included by harness=false Gherkin binaries, which omit test bodies.
#[allow(unused_imports)]
use openraft_memstore::TypeConfig;

fn identity() -> NodeIdentity {
    NodeIdentity {
        cluster_id: "rotation-faults".into(),
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
const STAGES: [Stage; 5] = [
    Stage::MetadataWrite,
    Stage::MetadataSync,
    Stage::MetadataRename,
    Stage::DirectorySync,
    Stage::Complete,
];

#[tokio::test]
async fn rotation_crash_child() {
    let Some(path) = std::env::var_os("LITHAIR_ROTATION_CRASH_DIR") else {
        return;
    };
    let index: usize = std::env::var("LITHAIR_ROTATION_CRASH_STAGE").unwrap().parse().unwrap();
    let mut inner =
        Inner::<TypeConfig>::load(path.into(), false, Some(identity()), None, false).unwrap();
    inner.fault = Some((STAGES[index], true));
    inner.transition_credentials(0, overlap()).unwrap();
    panic!("fault did not terminate process");
}

#[tokio::test]
async fn process_death_selects_exactly_one_complete_policy() {
    for index in 0..STAGES.len() {
        let dir = tempfile::tempdir().unwrap();
        drop(DurableLog::<TypeConfig>::create_for_node(dir.path(), identity()).await.unwrap());
        let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
            .arg("rotation_crash_child")
            .env("LITHAIR_ROTATION_CRASH_DIR", dir.path())
            .env("LITHAIR_ROTATION_CRASH_STAGE", index.to_string())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(15), child.wait())
                .await
                .unwrap()
                .unwrap()
                .code(),
            Some(86)
        );
        let selected = if index < 3 { CredentialPolicy::initial(&identity()) } else { overlap() };
        let store = DurableLog::<TypeConfig>::open_with_credentials(
            dir.path(),
            identity(),
            selected.clone(),
        )
        .await
        .unwrap();
        assert_eq!(store.credential_policy().await.unwrap(), selected);
    }
}

#[tokio::test]
async fn io_failure_never_partially_updates_credentials_or_rewrites_journal() {
    for (index, stage) in STAGES.iter().enumerate() {
        let dir = tempfile::tempdir().unwrap();
        drop(DurableLog::<TypeConfig>::create_for_node(dir.path(), identity()).await.unwrap());
        let before = std::fs::read(dir.path().join("journal")).unwrap();
        let mut inner =
            Inner::<TypeConfig>::load(dir.path().into(), false, Some(identity()), None, false)
                .unwrap();
        inner.fault = Some((*stage, false));
        assert!(inner.transition_credentials(0, overlap()).is_err());
        assert!(inner.failed);
        drop(inner);
        assert_eq!(std::fs::read(dir.path().join("journal")).unwrap(), before);
        let selected = if index < 3 { CredentialPolicy::initial(&identity()) } else { overlap() };
        let store = DurableLog::<TypeConfig>::open_with_credentials(
            dir.path(),
            identity(),
            selected.clone(),
        )
        .await
        .unwrap();
        assert_eq!(store.credential_policy().await.unwrap(), selected);
    }
}
