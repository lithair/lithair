use lithair_core::engine::{Event, EventStore, Scc2Engine, Scc2EngineConfig};
use lithair_core::model::ModelSpec;
use lithair_core::model_inspect::Inspectable;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Counter {
    pub value: u64,
}
impl Inspectable for Counter {
    fn get_field_value(&self, _: &str) -> Option<serde_json::Value> {
        None
    }
}
impl ModelSpec for Counter {
    fn get_policy(&self, _: &str) -> Option<lithair_core::model::FieldPolicy> {
        None
    }
}
#[derive(Serialize, Deserialize)]
pub struct Increment;
impl Event for Increment {
    type State = Counter;
    fn apply(&self, state: &mut Counter) {
        state.value += 1;
    }
    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap()
    }
}
pub fn engine(path: &std::path::Path) -> Scc2Engine<Counter> {
    engine_mode(path, false, false)
}
pub fn engine_mode(path: &std::path::Path, immediate: bool, binary: bool) -> Scc2Engine<Counter> {
    Scc2Engine::new(
        Arc::new(RwLock::new(
            EventStore::new_with_options(path.to_str().unwrap(), false, binary).unwrap(),
        )),
        Scc2EngineConfig {
            verbose_logging: false,
            enable_snapshots: true,
            snapshot_interval: 0,
            enable_deduplication: false,
            auto_persist_writes: true,
            force_immediate_persistence: immediate,
        },
    )
    .unwrap()
}

pub async fn snapshot_then_replay_applies_only_uncovered_increments() {
    let dir = tempfile::tempdir().unwrap();
    let live = engine(dir.path());
    live.apply_event("key".into(), Increment, true).await.unwrap();
    live.flush().await.unwrap();
    live.snapshot().unwrap();
    live.apply_event("key".into(), Increment, true).await.unwrap();
    live.flush().await.unwrap();
    let reopened = engine(dir.path());
    reopened.replay_events::<Increment>().unwrap();
    assert_eq!(reopened.read("key", |v| v.value), Some(2));
    // Replay is a restore, including on an already populated engine.
    reopened.replay_events::<Increment>().unwrap();
    assert_eq!(reopened.read("key", |v| v.value), Some(2));
}

pub async fn compacted_scc_snapshot_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let live = engine(dir.path());
    live.apply_event("key".into(), Increment, true).await.unwrap();
    live.flush().await.unwrap();
    live.snapshot().unwrap();
    live.truncate_log().unwrap();
    let reopened = engine(dir.path());
    reopened.replay_events::<Increment>().unwrap();
    assert_eq!(reopened.read("key", |v| v.value), Some(1));
}

pub fn truncation_requires_a_current_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = EventStore::new(dir.path().to_str().unwrap()).unwrap();
    store.append_raw_line(r#"{"value":1}"#).unwrap();
    store.flush().unwrap();
    assert!(store.truncate_events().is_err());
    assert_eq!(store.get_all_events().unwrap().len(), 1);
}

#[derive(Clone, Debug, Serialize, Deserialize, lithair_core::DeclarativeModel)]
struct Document {
    #[db(primary_key)]
    id: String,
    value: u64,
}

pub async fn repeated_http_compaction_preserves_updates_and_deletes() {
    use lithair_core::http::DeclarativeHttpHandler;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_str().unwrap();
    let mut handler = DeclarativeHttpHandler::<Document>::new(path).unwrap();
    for cycle in 0..3 {
        handler
            .apply_replicated_item(Document { id: "kept".into(), value: cycle })
            .await
            .unwrap();
        handler
            .apply_replicated_item(Document { id: "deleted".into(), value: cycle })
            .await
            .unwrap();
        handler.compact().await.unwrap();
        handler.apply_replicated_delete("deleted").await.unwrap();
        handler = DeclarativeHttpHandler::new_with_replay(path).await.unwrap();
        assert_eq!(handler.get_by_id("kept").await.unwrap().value, cycle);
        assert!(handler.get_by_id("deleted").await.is_none());
    }
    // A selected corrupt snapshot must never silently turn into an empty model.
    std::fs::write(dir.path().join("state.raftsnap"), "{").unwrap();
    assert!(DeclarativeHttpHandler::<Document>::new_with_replay(path).await.is_err());
}
