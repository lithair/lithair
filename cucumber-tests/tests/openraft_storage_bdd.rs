// Test the private implementation without introducing an application-facing API.
#[path = "../../lithair-core/src/cluster/durable_log/mod.rs"]
#[allow(dead_code)]
mod durable_log;

use cucumber::{given, then, when, World};
use durable_log::DurableLog;
use openraft::storage::{Adaptor, RaftLogStorage, RaftLogStorageExt};
use openraft::{CommittedLeaderId, Entry, EntryPayload, LogId, RaftLogReader, Vote};
use std::io::{Seek, SeekFrom, Write};

openraft::declare_raft_types!(Config: D = String, R = (), SnapshotData = std::io::Cursor<Vec<u8>>);

#[derive(Default, World)]
struct LogWorld {
    store: Option<Adaptor<Config, DurableLog<Config>>>,
    dir: Option<tempfile::TempDir>,
}
impl std::fmt::Debug for LogWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogWorld").finish_non_exhaustive()
    }
}
fn id(term: u64, index: u64) -> LogId<u64> {
    LogId::new(CommittedLeaderId::new(term, 1), index)
}
fn entry(term: u64, index: u64) -> Entry<Config> {
    Entry { log_id: id(term, index), payload: EntryPayload::Blank }
}

#[given("an isolated OpenRaft log store")]
async fn isolated(world: &mut LogWorld) {
    world.dir = Some(tempfile::tempdir().unwrap());
    world.store = Some(
        DurableLog::create(world.dir.as_ref().unwrap().path())
            .await
            .unwrap()
            .into_log_store(),
    );
}
#[when("I reopen the OpenRaft log store")]
async fn reopen(world: &mut LogWorld) {
    world.store = None;
    world.store = Some(
        DurableLog::open(world.dir.as_ref().unwrap().path())
            .await
            .unwrap()
            .into_log_store(),
    );
}
#[when("I persist a vote and three log entries")]
async fn persist(world: &mut LogWorld) {
    let store = world.store.as_mut().unwrap();
    store.save_vote(&Vote::new_committed(3, 1)).await.unwrap();
    store.blocking_append([entry(3, 0), entry(3, 1), entry(3, 2)]).await.unwrap();
}
#[then("the vote and three log entries are intact")]
async fn intact(world: &mut LogWorld) {
    let store = world.store.as_mut().unwrap();
    assert_eq!(store.read_vote().await.unwrap(), Some(Vote::new_committed(3, 1)));
    assert_eq!(
        store
            .try_get_log_entries(..)
            .await
            .unwrap()
            .iter()
            .map(|e| e.log_id)
            .collect::<Vec<_>>(),
        [id(3, 0), id(3, 1), id(3, 2)]
    );
}
#[when("I replace the conflicting suffix and purge the prefix")]
async fn replace(world: &mut LogWorld) {
    let store = world.store.as_mut().unwrap();
    store.truncate(id(3, 1)).await.unwrap();
    store.blocking_append([entry(4, 1)]).await.unwrap();
    store.purge(id(3, 0)).await.unwrap();
}
#[then("only the replacement suffix and purge watermark remain")]
async fn replaced(world: &mut LogWorld) {
    let store = world.store.as_mut().unwrap();
    assert_eq!(store.get_log_state().await.unwrap().last_purged_log_id, Some(id(3, 0)));
    assert_eq!(
        store
            .try_get_log_entries(..)
            .await
            .unwrap()
            .iter()
            .map(|e| e.log_id)
            .collect::<Vec<_>>(),
        [id(4, 1)]
    );
}
#[when("I damage the durable journal")]
async fn damage(world: &mut LogWorld) {
    world.store = None;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(world.dir.as_ref().unwrap().path().join("journal"))
        .unwrap();
    file.seek(SeekFrom::Start(24)).unwrap();
    file.write_all(&u32::MAX.to_le_bytes()).unwrap();
    file.sync_all().unwrap();
}
#[then("reopening the OpenRaft log store fails")]
async fn rejected(world: &mut LogWorld) {
    assert!(DurableLog::<Config>::open(world.dir.as_ref().unwrap().path()).await.is_err());
}
#[when("the stored journal and metadata disappear")]
async fn missing(world: &mut LogWorld) {
    world.store = None;
    for name in ["journal", "durable.meta"] {
        std::fs::remove_file(world.dir.as_ref().unwrap().path().join(name)).unwrap();
    }
}
#[tokio::main]
async fn main() {
    LogWorld::cucumber()
        .fail_on_skipped()
        .run_and_exit("features/persistence/openraft_storage.feature")
        .await;
}
