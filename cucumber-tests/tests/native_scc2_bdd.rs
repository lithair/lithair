use cucumber::{given, then, when, World};
use lithair_core::engine::{Event, EventStore, Scc2Engine, Scc2EngineConfig};
use lithair_core::DeclarativeModel;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};

#[derive(Clone, Debug, Default, Serialize, Deserialize, DeclarativeModel)]
struct Counter {
    #[db(primary_key)]
    id: String,
    #[db(unique)]
    name: String,
    count: u64,
}
#[derive(Serialize, Deserialize)]
struct Increment {
    id: String,
    name: String,
}
impl Event for Increment {
    type State = Counter;
    fn apply(&self, state: &mut Counter) {
        state.id = self.id.clone();
        state.name = self.name.clone();
        state.count += 1;
    }
}
#[derive(Default, World)]
struct NativeWorld {
    engine: Option<Arc<Scc2Engine<Counter>>>,
    dir: Option<tempfile::TempDir>,
    immediate: bool,
    acknowledged: usize,
}
impl std::fmt::Debug for NativeWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeWorld").finish_non_exhaustive()
    }
}
fn open(world: &NativeWorld) -> Arc<Scc2Engine<Counter>> {
    let path = world.dir.as_ref().unwrap().path();
    Arc::new(
        Scc2Engine::new(
            Arc::new(RwLock::new(EventStore::new(path.to_str().unwrap()).unwrap())),
            Scc2EngineConfig {
                verbose_logging: false,
                enable_snapshots: false,
                snapshot_interval: 0,
                enable_deduplication: false,
                auto_persist_writes: true,
                force_immediate_persistence: world.immediate,
            },
        )
        .unwrap(),
    )
}
#[given(expr = "an isolated native engine with {word} acknowledgements")]
async fn isolated(world: &mut NativeWorld, mode: String) {
    world.immediate = match mode.as_str() {
        "durable" => true,
        "queued" => false,
        _ => panic!("unknown mode"),
    };
    world.dir = Some(tempfile::tempdir().unwrap());
    world.engine = Some(open(world));
}
#[when("eight clients increment the same record twenty times")]
async fn increments(world: &mut NativeWorld) {
    let start = Arc::new(tokio::sync::Barrier::new(8));
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let engine = world.engine.as_ref().unwrap().clone();
        let start = start.clone();
        tasks.spawn(async move {
            start.wait().await;
            for _ in 0..20 {
                engine
                    .apply_event(
                        "counter".into(),
                        Increment { id: "counter".into(), name: "counter".into() },
                        true,
                    )
                    .await
                    .unwrap();
            }
        });
    }
    while let Some(result) = tasks.join_next().await {
        result.unwrap();
    }
    world.engine.as_ref().unwrap().flush().await.unwrap();
}
#[then("all 160 increments are present before and after reopening")]
async fn retained(world: &mut NativeWorld) {
    assert_eq!(world.engine.as_ref().unwrap().read("counter", |s| s.count), Some(160));
    world.engine = None;
    let engine = open(world);
    engine.replay_events::<Increment>().unwrap();
    assert_eq!(engine.read("counter", |s| s.count), Some(160));
    world.engine = Some(engine);
}
#[when("sixteen clients claim the same unique name")]
async fn unique(world: &mut NativeWorld) {
    let mut tasks = tokio::task::JoinSet::new();
    for id in 0..16 {
        let engine = world.engine.as_ref().unwrap().clone();
        tasks.spawn(async move {
            engine
                .apply_event(
                    id.to_string(),
                    Increment { id: id.to_string(), name: "shared".into() },
                    true,
                )
                .await
                .is_ok()
        });
    }
    while let Some(result) = tasks.join_next().await {
        world.acknowledged += usize::from(result.unwrap());
    }
    world.engine.as_ref().unwrap().flush().await.unwrap();
}
#[then("exactly one record owns the name after reopening")]
async fn one_owner(world: &mut NativeWorld) {
    assert_eq!(world.acknowledged, 1);
    world.engine = None;
    let engine = open(world);
    engine.replay_events::<Increment>().unwrap();
    assert_eq!(engine.total_count(), 1);
    assert_eq!(engine.get_indexed_values("name", "shared").len(), 1);
    world.engine = Some(engine);
}
#[when("the journal cannot be opened for writing")]
async fn fail(world: &mut NativeWorld) {
    std::fs::create_dir(world.dir.as_ref().unwrap().path().join("events.raftlog")).unwrap();
}
#[then("the mutation fails without publishing a record and subsequent flushes fail")]
async fn refused(world: &mut NativeWorld) {
    let engine = world.engine.as_ref().unwrap();
    assert!(engine
        .apply_event(
            "counter".into(),
            Increment { id: "counter".into(), name: "counter".into() },
            true
        )
        .await
        .is_err());
    assert!(engine.read("counter", |s| s.count).is_none());
    assert!(engine.get_indexed_values("name", "counter").is_empty());
    assert!(engine.flush().await.is_err());
    assert!(engine.flush().await.is_err());
}
#[given("the native cache retains one full record")]
async fn retain_one(world: &mut NativeWorld) {
    let engine = Arc::get_mut(world.engine.as_mut().unwrap()).unwrap();
    engine.replay_events::<Increment>().unwrap();
    engine.enable_retention(
        lithair_core::lifecycle::RetentionConfig { memory_count: Some(1), ..Default::default() },
        vec!["name".into()],
    );
}
async fn increment_key(world: &NativeWorld, key: &str) {
    world
        .engine
        .as_ref()
        .unwrap()
        .apply_event(key.into(), Increment { id: key.into(), name: key.into() }, true)
        .await
        .unwrap();
}
#[when("a second record evicts the first incremented record")]
async fn evict_first(world: &mut NativeWorld) {
    increment_key(world, "first").await;
    increment_key(world, "second").await;
    assert!(world.engine.as_ref().unwrap().retention_layer().unwrap().is_evicted("first"));
}
#[when("the first record is incremented again")]
async fn increment_first(world: &mut NativeWorld) {
    increment_key(world, "first").await;
}
#[then("both increments of the first record survive reopening")]
async fn warm_reopened(world: &mut NativeWorld) {
    let engine = world.engine.as_ref().unwrap();
    assert_eq!(engine.read("first", |s| s.count), Some(2));
    engine.flush().await.unwrap();
    world.engine = None;
    let engine = open(world);
    engine.replay_events::<Increment>().unwrap();
    assert_eq!(engine.read("first", |s| s.count), Some(2));
    assert_eq!(engine.read("second", |s| s.count), Some(1));
    world.engine = Some(engine);
}
#[when("a volatile change to the first record is evicted")]
async fn volatile_evicted(world: &mut NativeWorld) {
    increment_key(world, "first").await;
    world.engine.as_ref().unwrap().write("first", |s| s.count = 42).unwrap();
    increment_key(world, "second").await;
}
#[then("updating the first record is refused without using its older durable value")]
async fn volatile_refused(world: &mut NativeWorld) {
    let engine = world.engine.as_ref().unwrap();
    assert!(engine
        .apply_event("first".into(), Increment { id: "first".into(), name: "first".into() }, true,)
        .await
        .unwrap_err()
        .to_string()
        .contains("volatile"));
    assert_eq!(
        engine.update_entry_volatile("first", |_| panic!("incomplete state")),
        None::<()>
    );
    assert_eq!(engine.read_or_load::<_, _, Increment>("first", |s| s.count), None);
    assert_eq!(engine.retention_layer().unwrap().read_warm("first").unwrap().version, 2);
    assert_eq!(engine.get_indexed_values("name", "first"), vec!["first"]);
    assert_eq!(engine.read("second", |s| s.count), Some(1));
}
#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    NativeWorld::cucumber().run_and_exit("features/core/native_scc2.feature").await;
}
