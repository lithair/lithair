//! Native in-process concurrency, journal ordering and failure acknowledgements.
use lithair_core::engine::{AsyncWriter, Event, EventStore, Scc2Engine, Scc2EngineConfig};
use lithair_core::model::{FieldPolicy, ModelSpec};
use lithair_core::model_inspect::Inspectable;
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, RwLock,
};

#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq)]
struct Record {
    name: String,
    values: Vec<usize>,
}

impl Inspectable for Record {
    fn get_field_value(&self, field: &str) -> Option<serde_json::Value> {
        match field {
            "name" => Some(serde_json::json!(self.name)),
            "group" => Some(serde_json::json!("all")),
            _ => None,
        }
    }
}
impl ModelSpec for Record {
    fn get_policy(&self, field: &str) -> Option<FieldPolicy> {
        match field {
            "name" => Some(FieldPolicy { unique: true, indexed: true, ..Default::default() }),
            "group" => Some(FieldPolicy { indexed: true, ..Default::default() }),
            _ => None,
        }
    }
    fn get_all_fields(&self) -> Vec<String> {
        vec!["name".into(), "group".into()]
    }
}

#[derive(Serialize, Deserialize)]
struct Change {
    name: String,
    value: usize,
    #[serde(skip)]
    applications: Option<Arc<AtomicUsize>>,
}
impl Event for Change {
    type State = Record;
    fn apply(&self, state: &mut Record) {
        if let Some(count) = &self.applications {
            count.fetch_add(1, Ordering::SeqCst);
        }
        state.name = self.name.clone();
        state.values.push(self.value);
    }
    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap()
    }
}
fn change(name: &str, value: usize) -> Change {
    Change { name: name.into(), value, applications: None }
}
fn engine(path: &std::path::Path, immediate: bool) -> Arc<Scc2Engine<Record>> {
    Arc::new(
        Scc2Engine::new(
            Arc::new(RwLock::new(EventStore::new(path.to_str().unwrap()).unwrap())),
            Scc2EngineConfig {
                verbose_logging: false,
                enable_snapshots: true,
                snapshot_interval: 1000,
                enable_deduplication: false,
                auto_persist_writes: true,
                force_immediate_persistence: immediate,
            },
        )
        .unwrap(),
    )
}

#[tokio::test]
async fn simultaneous_readers_never_turn_contention_into_absence() {
    let data = tempfile::tempdir().unwrap();
    let engine = engine(data.path(), false);
    engine.insert_sync("key".into(), Record::default());
    // The first callback holds its read access until the second has completed.
    // A timeout is only a deadlock bound, never an assertion about performance.
    let (entered, ready) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let other = engine.clone();
    let reader = std::thread::spawn(move || {
        other.read("key", |_| {
            entered.send(()).unwrap();
            released.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        })
    });
    ready.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
    let result = engine.read("key", |s| s.clone());
    release.send(()).unwrap();
    reader.join().unwrap();
    assert_eq!(result, Some(Record::default()));
}

#[tokio::test]
async fn event_is_applied_once_and_index_removal_is_complete() {
    let data = tempfile::tempdir().unwrap();
    let engine = engine(data.path(), false);
    let count = Arc::new(AtomicUsize::new(0));
    engine
        .apply_event(
            "key".into(),
            Change { applications: Some(count.clone()), ..change("unique", 1) },
            false,
        )
        .await
        .unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(engine.get_indexed_values("name", "unique"), vec!["key"]);
    engine.remove_sync("key");
    assert!(engine.get_indexed_values("name", "unique").is_empty());
    engine
        .apply_event("replacement".into(), change("unique", 2), false)
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_writes_have_one_order_in_memory_and_after_reopen() {
    for immediate in [false, true] {
        let data = tempfile::tempdir().unwrap();
        let current = engine(data.path(), immediate);
        let start = Arc::new(tokio::sync::Barrier::new(8));
        let mut writers = tokio::task::JoinSet::new();
        for writer in 0..8 {
            let current = current.clone();
            let start = start.clone();
            writers.spawn(async move {
                start.wait().await;
                for offset in 0..16 {
                    current
                        .apply_event(
                            "counter".into(),
                            change("counter", writer * 16 + offset),
                            true,
                        )
                        .await
                        .unwrap();
                }
            });
        }
        while let Some(result) = writers.join_next().await {
            result.unwrap();
        }
        current.flush().await.unwrap();
        let before = current.read("counter", Clone::clone).unwrap();
        assert_eq!(before.values.len(), 128);
        let mut sorted = before.values.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..128).collect::<Vec<_>>());
        drop(current);
        let reopened = engine(data.path(), immediate);
        reopened.replay_events::<Change>().unwrap();
        assert_eq!(reopened.read("counter", Clone::clone), Some(before));
        assert_eq!(reopened.get_indexed_values("name", "counter"), vec!["counter"]);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn competing_unique_values_have_exactly_one_winner() {
    let data = tempfile::tempdir().unwrap();
    let engine = engine(data.path(), false);
    let start = Arc::new(tokio::sync::Barrier::new(32));
    let mut writers = tokio::task::JoinSet::new();
    for id in 0..32 {
        let engine = engine.clone();
        let start = start.clone();
        writers.spawn(async move {
            start.wait().await;
            engine.apply_event(id.to_string(), change("shared", id), false).await.is_ok()
        });
    }
    let mut winners = 0;
    while let Some(result) = writers.join_next().await {
        winners += usize::from(result.unwrap());
    }
    assert_eq!(winners, 1);
    assert_eq!(engine.total_count(), 1);
    assert_eq!(engine.get_indexed_values("name", "shared").len(), 1);
}

#[tokio::test]
async fn failed_flush_is_reported_and_writer_remains_failed() {
    let data = tempfile::tempdir().unwrap();
    let store = Arc::new(RwLock::new(EventStore::new(data.path().to_str().unwrap()).unwrap()));
    let writer = AsyncWriter::new(store, 1000);
    std::fs::create_dir(data.path().join("events.raftlog")).unwrap();
    writer.write("{}".into()).unwrap();
    assert!(writer.flush().await.is_err());
    std::fs::remove_dir(data.path().join("events.raftlog")).unwrap();
    assert!(writer.write("{}".into()).is_err());
    assert!(writer.flush().await.is_err());
    writer.shutdown().await;
}

#[tokio::test]
async fn immediate_failure_does_not_publish_state_or_indexes() {
    let data = tempfile::tempdir().unwrap();
    let engine = engine(data.path(), true);
    engine.insert_sync("key".into(), Record { name: "before".into(), values: vec![0] });
    std::fs::create_dir(data.path().join("events.raftlog")).unwrap();
    assert!(engine.apply_event("key".into(), change("after", 1), true).await.is_err());
    assert_eq!(engine.read("key", |s| s.name.clone()), Some("before".into()));
    assert_eq!(engine.get_indexed_values("name", "before"), vec!["key"]);
    assert!(engine.get_indexed_values("name", "after").is_empty());
    assert!(engine.flush().await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shared_secondary_index_keeps_every_concurrent_record() {
    let data = tempfile::tempdir().unwrap();
    let current = engine(data.path(), false);
    let mut writers = tokio::task::JoinSet::new();
    for id in 0..64 {
        let current = current.clone();
        writers.spawn(async move {
            current
                .apply_event(id.to_string(), change(&id.to_string(), id), true)
                .await
                .unwrap();
        });
    }
    while let Some(result) = writers.join_next().await {
        result.unwrap();
    }
    let mut expected: Vec<_> = (0..64).map(|id| id.to_string()).collect();
    expected.sort();
    let mut actual = current.get_indexed_values("group", "all");
    actual.sort();
    assert_eq!(actual, expected);
    current.flush().await.unwrap();
    drop(current);
    let reopened = engine(data.path(), false);
    reopened.replay_events::<Change>().unwrap();
    let mut actual = reopened.get_indexed_values("group", "all");
    actual.sort();
    assert_eq!(actual, expected);
    for id in 0..64 {
        assert_eq!(reopened.read(&id.to_string(), |s| s.values.clone()), Some(vec![id]));
    }
}

#[tokio::test]
async fn legacy_raw_and_enveloped_events_remain_readable() {
    use lithair_core::engine::EventEnvelope;
    let data = tempfile::tempdir().unwrap();
    {
        let mut store = EventStore::new(data.path().to_str().unwrap()).unwrap();
        // Historical raw events without aggregate_id belong to "global".
        store
            .append_raw_line(&serde_json::to_string(&change("legacy", 1)).unwrap())
            .unwrap();
        store
            .append_envelope(&EventEnvelope::new(
                "old_crate::Change".into(),
                "legacy-2".into(),
                1,
                serde_json::to_string(&change("legacy", 2)).unwrap(),
                Some("global".into()),
                None,
            ))
            .unwrap();
        store.force_flush().unwrap();
    }
    let current = engine(data.path(), true);
    current.replay_events::<Change>().unwrap();
    current.apply_event("global".into(), change("legacy", 3), true).await.unwrap();
    drop(current);
    let reopened = engine(data.path(), true);
    reopened.replay_events::<Change>().unwrap();
    assert_eq!(reopened.read("global", |s| s.values.clone()), Some(vec![1, 2, 3]));
}

#[tokio::test]
async fn append_failure_rejects_every_flush_barrier() {
    let data = tempfile::tempdir().unwrap();
    let store = Arc::new(RwLock::new(EventStore::new(data.path().to_str().unwrap()).unwrap()));
    let writer = AsyncWriter::new(store, 1); // append itself flushes the full batch
    std::fs::create_dir(data.path().join("events.raftlog")).unwrap();
    writer.write("{}".into()).unwrap();
    let (first, second, third) = tokio::join!(writer.flush(), writer.flush(), writer.flush());
    assert!(first.is_err());
    assert_eq!(first, second);
    assert_eq!(first, third);
    writer.shutdown().await;
}

#[tokio::test]
async fn poisoned_store_rejects_flush_without_hanging() {
    let data = tempfile::tempdir().unwrap();
    let store = Arc::new(RwLock::new(EventStore::new(data.path().to_str().unwrap()).unwrap()));
    let writer = AsyncWriter::new(store.clone(), 1000);
    assert!(std::thread::spawn(move || {
        let _guard = store.write().unwrap();
        panic!("injected store failure");
    })
    .join()
    .is_err());
    writer.write("{}".into()).unwrap();
    let result = tokio::time::timeout(std::time::Duration::from_secs(10), writer.flush())
        .await
        .unwrap();
    assert!(result.is_err());
    writer.shutdown().await;
}

#[tokio::test]
async fn retention_runs_after_releasing_the_published_bucket() {
    use lithair_core::lifecycle::RetentionConfig;
    let data = tempfile::tempdir().unwrap();
    let mut current = engine(data.path(), false);
    Arc::get_mut(&mut current).unwrap().enable_retention(
        RetentionConfig { memory_count: Some(1), ..Default::default() },
        vec!["name".into()],
    );
    current.insert_sync("one".into(), Record { name: "one".into(), values: vec![1] });
    current.write("one", |record| record.values.push(2)).unwrap();
    current.insert_sync("two".into(), Record { name: "two".into(), values: vec![3] });
    assert_eq!(current.internal_map().len(), 1);
    assert_eq!(current.total_count(), 2);
    assert!(current.retention_layer().unwrap().is_evicted("one"));
    assert_eq!(current.get_indexed_values("name", "one"), vec!["one"]);
    current.remove_sync("one");
    assert_eq!(current.total_count(), 1);
    assert!(current.get_indexed_values("name", "one").is_empty());
    current.apply_event("three".into(), change("one", 3), false).await.unwrap();
}

#[tokio::test]
async fn replacing_an_evicted_record_removes_its_previous_index_values() {
    use lithair_core::lifecycle::RetentionConfig;
    let data = tempfile::tempdir().unwrap();
    let mut current = engine(data.path(), false);
    Arc::get_mut(&mut current).unwrap().enable_retention(
        RetentionConfig { memory_count: Some(1), ..Default::default() },
        vec!["name".into()],
    );
    current.insert_sync("one".into(), Record { name: "before".into(), values: vec![1] });
    current.insert_sync("two".into(), Record { name: "other".into(), values: vec![2] });
    assert!(current.retention_layer().unwrap().is_evicted("one"));
    current.insert_sync("one".into(), Record { name: "after".into(), values: vec![3] });
    assert!(current.get_indexed_values("name", "before").is_empty());
    assert_eq!(current.get_indexed_values("name", "after"), vec!["one"]);
    assert_eq!(current.total_count(), 2);
    assert!(!current.retention_layer().unwrap().is_evicted("one"));
}
