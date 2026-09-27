use super::*;
use crate::cluster::durable_log::{NodeIdentity, Stage};
use openraft::{CommittedLeaderId, Entry, EntryPayload, LogId, RaftStorage};

fn models() -> BTreeMap<String, Model> {
    BTreeMap::from([(
        "items".into(),
        Model {
            id: "items".into(),
            path: "/api/items".into(),
            contract: json!({}),
            prepare: |_, _| panic!("committed apply must never execute model callbacks"),
            readable: |_| true,
        },
    )])
}
fn identity() -> NodeIdentity {
    NodeIdentity {
        cluster_id: "native-apply".into(),
        node_id: 1,
        bootstrap_node: 1,
        voters: BTreeMap::from([(1, [1; 32]), (2, [2; 32]), (3, [3; 32])]),
    }
}
fn entry(index: u64) -> Entry<Config> {
    Entry {
        log_id: LogId::new(CommittedLeaderId::new(1, 1), index),
        payload: EntryPayload::Normal(Command {
            version: 1,
            contract: [9; 32],
            model: "items".into(),
            request_id: "once".into(),
            fingerprint: [8; 32],
            revision: 0,
            change: Change::Put { key: "one".into(), value: json!({"id":"one"}) },
            reply: Reply { status: 201, body: Some(json!({"id":"one"})) },
        }),
    }
}
#[tokio::test]
async fn apply_crash_child() {
    let Ok(path) = std::env::var("NATIVE_CLUSTER_APPLY_CHILD") else {
        return;
    };
    let stage =
        stages()[std::env::var("NATIVE_CLUSTER_APPLY_STAGE").unwrap().parse::<usize>().unwrap()];
    let durable = DurableLog::<Config>::open_for_node(&path, identity()).await.unwrap();
    durable.bind_application([9; 32]).await.unwrap();
    let mut machine =
        Machine::restore(durable.clone(), State::new([9; 32], &models())).await.unwrap();
    durable.inject_failure(stage, true).await;
    machine.apply_to_state_machine(&[entry(0)]).await.unwrap();
    panic!("apply crash boundary was not reached");
}
fn stages() -> [Stage; 7] {
    [
        Stage::GenerationWrite,
        Stage::GenerationSync,
        Stage::GenerationDirectorySync,
        Stage::MetadataSync,
        Stage::MetadataRename,
        Stage::DirectorySync,
        Stage::Complete,
    ]
}
#[tokio::test]
async fn committed_apply_process_death_recovers_data_position_and_results_together() {
    for (index, stage) in stages().iter().enumerate() {
        let directory = tempfile::tempdir().unwrap();
        let mut durable = DurableLog::<Config>::create_for_node(directory.path(), identity())
            .await
            .unwrap();
        durable.bind_application([9; 32]).await.unwrap();
        durable.append_to_log([entry(0)]).await.unwrap();
        durable.save_committed(Some(entry(0).log_id)).await.unwrap();
        drop(durable);
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "cluster::native::tests::apply_crash_child", "--nocapture"])
            .env("NATIVE_CLUSTER_APPLY_CHILD", directory.path())
            .env("NATIVE_CLUSTER_APPLY_STAGE", index.to_string())
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(86),
            "{stage:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let durable =
            DurableLog::<Config>::open_for_node(directory.path(), identity()).await.unwrap();
        let mut machine = Machine::restore(durable, State::new([9; 32], &models())).await.unwrap();
        let state = machine.state.read().await;
        assert_eq!(state.applied.is_some(), state.models["items"].contains_key("one"));
        assert_eq!(state.applied.is_some(), state.result("items", "once", &[8; 32]).is_some());
        let applied = state.applied.is_some();
        drop(state);
        if !applied {
            machine.apply_to_state_machine(&[entry(0)]).await.unwrap();
        }
        let result = machine.apply_to_state_machine(&[entry(1)]).await.unwrap();
        assert_eq!(result[0].status, 201);
        assert_eq!(machine.state.read().await.revision, 1);
        assert_eq!(machine.state.read().await.models["items"].len(), 1);
    }
}
#[tokio::test]
async fn failed_apply_never_publishes_or_authorizes_reads() {
    let directory = tempfile::tempdir().unwrap();
    let mut durable = DurableLog::<Config>::create_for_node(directory.path(), identity())
        .await
        .unwrap();
    durable.bind_application([9; 32]).await.unwrap();
    durable.append_to_log([entry(0)]).await.unwrap();
    durable.save_committed(Some(entry(0).log_id)).await.unwrap();
    let mut machine =
        Machine::restore(durable.clone(), State::new([9; 32], &models())).await.unwrap();
    durable.inject_failure(Stage::MetadataRename, false).await;
    assert!(machine.apply_to_state_machine(&[entry(0)]).await.is_err());
    assert!(machine.failed.load(Ordering::Acquire));
    assert!(machine.state.read().await.models["items"].is_empty());
    assert!(machine.apply_to_state_machine(&[entry(0)]).await.is_err());
}
#[test]
fn stale_preparation_conflicting_keys_and_bounded_results_are_deterministic() {
    let mut state = State::new([9; 32], &models());
    let EntryPayload::Normal(command) = entry(0).payload else { unreachable!() };
    assert_eq!(state.execute(&command).unwrap().status, 201);
    let mut stale = command.clone();
    stale.request_id = "stale".into();
    assert_eq!(state.execute(&stale).unwrap().status, 409);
    assert_eq!(state.result("items", "once", &[7; 32]).unwrap().status, 409);
    for index in 0..state::RESULTS {
        stale.request_id = format!("key-{index}");
        stale.revision = state.revision;
        stale.change = Change::None;
        state.execute(&stale).unwrap();
    }
    assert!(state.result("items", "once", &[8; 32]).is_none());
    assert_eq!(state.models["items"].len(), 1);
}
