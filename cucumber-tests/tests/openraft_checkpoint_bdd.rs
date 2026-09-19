#[path = "../../lithair-core/src/cluster/durable_log/mod.rs"]
#[allow(dead_code)]
mod durable_log;
use cucumber::{given, then, when, World};
use durable_log::DurableLog;
use openraft::{
    storage::{RaftLogStorage, RaftLogStorageExt},
    CommittedLeaderId, Entry, EntryPayload, LogId, RaftLogReader, SnapshotMeta,
};

openraft::declare_raft_types!(Config:D=String,R=(),SnapshotData=std::io::Cursor<Vec<u8>>);
#[derive(Default, World)]
struct CheckpointWorld {
    dir: Option<tempfile::TempDir>,
    store: Option<DurableLog<Config>>,
}
impl std::fmt::Debug for CheckpointWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CheckpointWorld").finish_non_exhaustive()
    }
}
fn id(index: u64) -> LogId<u64> {
    LogId::new(CommittedLeaderId::new(1, 1), index)
}
#[given("a durable OpenRaft log with committed commands")]
async fn seed(world: &mut CheckpointWorld) {
    let dir = tempfile::tempdir().unwrap();
    let store = DurableLog::<Config>::create(dir.path()).await.unwrap();
    let mut log = store.clone().into_log_store();
    log.blocking_append((0..3).map(|index| Entry {
        log_id: id(index),
        payload: EntryPayload::Normal(format!("command-{index}")),
    }))
    .await
    .unwrap();
    log.save_committed(Some(id(2))).await.unwrap();
    world.dir = Some(dir);
    world.store = Some(store);
}
#[when("I publish a snapshot and purge its covered prefix")]
async fn snapshot(world: &mut CheckpointWorld) {
    let store = world.store.as_ref().unwrap();
    store
        .save_snapshot(
            SnapshotMeta {
                last_log_id: Some(id(1)),
                last_membership: Default::default(),
                snapshot_id: "bdd".into(),
            },
            b"checkpoint".to_vec(),
        )
        .await
        .unwrap();
    store.clone().into_log_store().purge(id(1)).await.unwrap();
}
#[when("I compact and reopen the OpenRaft journal")]
async fn reopen(world: &mut CheckpointWorld) {
    world.store.take().unwrap().compact().await.unwrap();
    world.store = Some(DurableLog::open(world.dir.as_ref().unwrap().path()).await.unwrap());
}
#[then("the snapshot and retained commands are intact")]
async fn intact(world: &mut CheckpointWorld) {
    let store = world.store.as_ref().unwrap();
    let snapshot = store.snapshot().await.unwrap().unwrap();
    assert_eq!(snapshot.meta.last_log_id, Some(id(1)));
    assert_eq!(snapshot.data, b"checkpoint"[..]);
    let mut log = store.clone().into_log_store();
    assert_eq!(log.read_committed().await.unwrap(), Some(id(2)));
    assert_eq!(
        log.try_get_log_entries(..)
            .await
            .unwrap()
            .iter()
            .map(|e| e.log_id)
            .collect::<Vec<_>>(),
        vec![id(2)]
    );
}
#[then("obsolete journal generations have been reclaimed")]
async fn reclaimed(world: &mut CheckpointWorld) {
    let path = world.dir.as_ref().unwrap().path();
    assert!(!path.join("journal").exists());
    assert_eq!(std::fs::read_dir(path).unwrap().count(), 4);
}
#[when("I damage the active snapshot generation")]
async fn damage(world: &mut CheckpointWorld) {
    world.store = None;
    let entry = std::fs::read_dir(world.dir.as_ref().unwrap().path())
        .unwrap()
        .map(Result::unwrap)
        .find(|e| e.file_name().to_string_lossy().starts_with("snapshot-"))
        .unwrap();
    std::fs::write(entry.path(), b"corrupt").unwrap();
}
#[then("OpenRaft checkpoint recovery fails")]
async fn rejected(world: &mut CheckpointWorld) {
    assert!(DurableLog::<Config>::open(world.dir.as_ref().unwrap().path()).await.is_err());
}
#[tokio::main]
async fn main() {
    CheckpointWorld::cucumber()
        .fail_on_skipped()
        .run_and_exit("features/persistence/openraft_checkpoint.feature")
        .await;
}
