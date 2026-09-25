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
    engine_mode(path, immediate, false)
}
fn engine_mode(path: &std::path::Path, immediate: bool, binary: bool) -> Arc<Scc2Engine<Record>> {
    Arc::new(
        Scc2Engine::new(
            Arc::new(RwLock::new(
                EventStore::new_with_options(path.to_str().unwrap(), false, binary).unwrap(),
            )),
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

fn retain_one(engine: &mut Arc<Scc2Engine<Record>>) {
    Arc::get_mut(engine).unwrap().enable_retention(
        lithair_core::lifecycle::RetentionConfig { memory_count: Some(1), ..Default::default() },
        vec!["name".into()],
    );
}

#[tokio::test]
async fn warm_mutation_preserves_full_state_and_version_in_every_journal_mode() {
    for immediate in [false, true] {
        for binary in [false, true] {
            let data = tempfile::tempdir().unwrap();
            let mut current = engine_mode(data.path(), immediate, binary);
            current.replay_events::<Change>().unwrap();
            retain_one(&mut current);
            current.apply_event("one".into(), change("before", 1), true).await.unwrap();
            current.apply_event("two".into(), change("other", 9), true).await.unwrap();
            assert!(current.retention_layer().unwrap().is_evicted("one"));
            // No flush: recovery must include accepted queued writes itself.
            current.apply_event("one".into(), change("after", 2), true).await.unwrap();
            let expected = Record { name: "after".into(), values: vec![1, 2] };
            assert_eq!(current.read("one", Clone::clone), Some(expected.clone()));
            assert_eq!(current.internal_map().read_sync("one", |_, v| v.version), Some(2));
            assert!(current.get_indexed_values("name", "before").is_empty());
            assert_eq!(current.get_indexed_values("name", "after"), vec!["one"]);
            current.flush().await.unwrap();
            drop(current);
            let restored = engine_mode(data.path(), immediate, binary);
            restored.replay_events::<Change>().unwrap();
            assert_eq!(restored.read("one", Clone::clone), Some(expected));
        }
    }
}

#[tokio::test]
async fn warm_volatile_callback_starts_from_full_state() {
    let data = tempfile::tempdir().unwrap();
    let mut current = engine(data.path(), false);
    current.replay_events::<Change>().unwrap();
    retain_one(&mut current);
    current.apply_event("one".into(), change("one", 1), true).await.unwrap();
    current.apply_event("two".into(), change("two", 9), true).await.unwrap();
    assert_eq!(
        current.update_entry_volatile("one", |r| {
            r.values.push(2);
            r.values.len()
        }),
        Some(2)
    );
    assert_eq!(
        current.read("one", Clone::clone),
        Some(Record { name: "one".into(), values: vec![1, 2] })
    );
}

#[tokio::test]
async fn warm_mutations_recover_checkpoint_and_suffix_without_resetting_versions() {
    for immediate in [false, true] {
        for binary in [false, true] {
            let data = tempfile::tempdir().unwrap();
            let mut current = engine_mode(data.path(), immediate, binary);
            current.replay_events::<Change>().unwrap();
            current.apply_event("one".into(), change("one", 1), true).await.unwrap();
            current.snapshot().unwrap();
            current.truncate_log().unwrap();
            retain_one(&mut current);
            for value in 2..=4 {
                current.apply_event("two".into(), change("two", value), true).await.unwrap();
                current.apply_event("one".into(), change("one", value), true).await.unwrap();
            }
            assert_eq!(current.read("one", |r| r.values.clone()), Some(vec![1, 2, 3, 4]));
            assert_eq!(current.internal_map().read_sync("one", |_, r| r.version), Some(4));
            current.flush().await.unwrap();
            drop(current);
            let restored = engine_mode(data.path(), immediate, binary);
            restored.replay_events::<Change>().unwrap();
            assert_eq!(restored.read("one", |r| r.values.clone()), Some(vec![1, 2, 3, 4]));
            assert_eq!(restored.read("two", |r| r.values.clone()), Some(vec![2, 3, 4]));
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_warm_mutations_preserve_acknowledged_order_after_restart() {
    for immediate in [false, true] {
        for binary in [false, true] {
            let data = tempfile::tempdir().unwrap();
            let mut current = engine_mode(data.path(), immediate, binary);
            current.replay_events::<Change>().unwrap();
            retain_one(&mut current);
            let start = Arc::new(tokio::sync::Barrier::new(8));
            let mut jobs = tokio::task::JoinSet::new();
            for client in 0..8 {
                let current = current.clone();
                let start = start.clone();
                jobs.spawn(async move {
                    start.wait().await;
                    for value in 0..4 {
                        for key in ["one", "two"] {
                            current
                                .apply_event(key.into(), change(key, client * 4 + value), true)
                                .await
                                .unwrap();
                        }
                    }
                });
            }
            while let Some(result) = jobs.join_next().await {
                result.unwrap();
            }
            let before: Vec<_> = ["one", "two"]
                .into_iter()
                .map(|key| {
                    let record = current.read_or_load::<_, _, Change>(key, Clone::clone).unwrap();
                    let mut values = record.values.clone();
                    values.sort_unstable();
                    assert_eq!(values, (0..32).collect::<Vec<_>>());
                    record
                })
                .collect();
            current.flush().await.unwrap();
            drop(current);
            let restored = engine_mode(data.path(), immediate, binary);
            restored.replay_events::<Change>().unwrap();
            for (key, expected) in ["one", "two"].into_iter().zip(before) {
                assert_eq!(restored.read(key, Clone::clone), Some(expected));
            }
        }
    }
}

#[tokio::test]
async fn warm_recovery_failure_does_not_run_callback_publish_or_append() {
    for damage in ["no-decoder", "unknown-event", "missing-history", "io-error", "volatile"] {
        let data = tempfile::tempdir().unwrap();
        let mut current = engine(data.path(), true);
        if damage != "no-decoder" {
            current.replay_events::<Change>().unwrap();
        }
        retain_one(&mut current);
        current.apply_event("one".into(), change("one", 1), true).await.unwrap();
        if damage == "volatile" {
            current.write("one", |r| r.values.push(99)).unwrap();
        }
        current.apply_event("two".into(), change("two", 2), true).await.unwrap();
        match damage {
            "unknown-event" => {
                let store = current.event_store();
                let mut store = store.write().unwrap();
                let mut envelope = lithair_core::engine::EventEnvelope::new(
                    "Unknown".into(),
                    "unknown".into(),
                    0,
                    "{}".into(),
                    Some("one".into()),
                    None,
                );
                envelope.event_hash = None;
                store.append_envelope(&envelope).unwrap();
                store.flush().unwrap();
            }
            "missing-history" => {
                std::fs::write(data.path().join("events.raftlog"), "").unwrap();
            }
            "io-error" => {
                std::fs::remove_file(data.path().join("events.raftlog")).unwrap();
                std::fs::create_dir(data.path().join("events.raftlog")).unwrap();
            }
            _ => {}
        }
        let journal = data.path().join("events.raftlog");
        let bytes_before = std::fs::read(&journal).ok();
        let warm_before = current.retention_layer().unwrap().read_warm("one").unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let mutation = Change { applications: Some(calls.clone()), ..change("after", 3) };
        assert!(current.apply_event("one".into(), mutation, true).await.is_err(), "{damage}");
        assert_eq!(
            current.update_entry_volatile("one", |_| {
                calls.fetch_add(1, Ordering::SeqCst);
            }),
            None
        );
        assert!(current
            .write("one", |_| {
                calls.fetch_add(1, Ordering::SeqCst);
            })
            .is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0, "{damage}");
        assert!(current.read("one", Clone::clone).is_none());
        assert_eq!(current.read("two", |r| r.values.clone()), Some(vec![2]));
        let warm_after = current.retention_layer().unwrap().read_warm("one").unwrap();
        assert_eq!(warm_before.version, warm_after.version);
        assert_eq!(warm_before.pinned_data, warm_after.pinned_data);
        assert_eq!(current.get_indexed_values("name", "one"), vec!["one"]);
        assert!(current.get_indexed_values("name", "after").is_empty());
        assert_eq!(std::fs::read(&journal).ok(), bytes_before);
        if damage != "no-decoder" {
            assert!(current.read_or_load::<_, _, Change>("one", Clone::clone).is_none());
        }
    }
}

#[tokio::test]
async fn warm_unique_conflict_leaves_recovered_candidate_unpublished() {
    let data = tempfile::tempdir().unwrap();
    let mut current = engine(data.path(), true);
    current.replay_events::<Change>().unwrap();
    retain_one(&mut current);
    current.apply_event("one".into(), change("one", 1), true).await.unwrap();
    current.apply_event("two".into(), change("taken", 2), true).await.unwrap();
    assert!(current
        .apply_event("one".into(), change("taken", 3), true)
        .await
        .unwrap_err()
        .to_string()
        .contains("Unique"));
    assert!(current.retention_layer().unwrap().is_evicted("one"));
    assert_eq!(current.read_or_load::<_, _, Change>("one", |r| r.values.clone()), Some(vec![1]));
    assert_eq!(current.get_indexed_values("name", "one"), vec!["one"]);
    current.apply_event("one".into(), change("renamed", 4), true).await.unwrap();
    assert_eq!(current.read("one", |r| r.values.clone()), Some(vec![1, 4]));
}

#[tokio::test]
async fn warm_volatile_state_is_recoverable_only_after_a_full_checkpoint() {
    for checkpoint in [false, true] {
        let data = tempfile::tempdir().unwrap();
        let mut current = engine(data.path(), false);
        current.replay_events::<Change>().unwrap();
        retain_one(&mut current);
        current.apply_event("one".into(), change("one", 1), false).await.unwrap();
        if checkpoint {
            current.snapshot().unwrap();
            current.truncate_log().unwrap();
        }
        current.apply_event("two".into(), change("two", 2), true).await.unwrap();
        let result = current.apply_event("one".into(), change("one", 3), true).await;
        assert_eq!(result.is_ok(), checkpoint);
        if checkpoint {
            assert_eq!(current.read("one", |r| r.values.clone()), Some(vec![1, 3]));
            current.flush().await.unwrap();
            drop(current);
            let restored = engine(data.path(), false);
            restored.replay_events::<Change>().unwrap();
            assert_eq!(restored.read("one", |r| r.values.clone()), Some(vec![1, 3]));
        }
    }
}

#[tokio::test]
async fn warm_mutation_with_zero_resident_capacity_keeps_the_complete_history() {
    let data = tempfile::tempdir().unwrap();
    let mut current = engine(data.path(), false);
    current.replay_events::<Change>().unwrap();
    Arc::get_mut(&mut current).unwrap().enable_retention(
        lithair_core::lifecycle::RetentionConfig { memory_count: Some(0), ..Default::default() },
        vec![],
    );
    for value in 0..3 {
        current.apply_event("one".into(), change("one", value), true).await.unwrap();
    }
    assert_eq!(current.internal_map().len(), 0);
    assert_eq!(
        current.read_or_load::<_, _, Change>("one", |r| r.values.clone()),
        Some(vec![0, 1, 2])
    );
    assert_eq!(current.retention_layer().unwrap().read_warm("one").unwrap().version, 3);
}

#[tokio::test]
async fn warm_mutation_uses_the_application_enum_and_preserves_untouched_fields() {
    #[derive(Serialize, Deserialize)]
    enum ApplicationEvent {
        Rename(String),
        Append(usize),
    }
    impl Event for ApplicationEvent {
        type State = Record;
        fn apply(&self, state: &mut Record) {
            match self {
                Self::Rename(name) => state.name = name.clone(),
                Self::Append(value) => state.values.push(*value),
            }
        }
    }
    let data = tempfile::tempdir().unwrap();
    let mut current = engine(data.path(), false);
    current.replay_events::<ApplicationEvent>().unwrap();
    retain_one(&mut current);
    current
        .apply_event("one".into(), ApplicationEvent::Rename("original".into()), true)
        .await
        .unwrap();
    current
        .apply_event("one".into(), ApplicationEvent::Append(1), true)
        .await
        .unwrap();
    current
        .apply_event("two".into(), ApplicationEvent::Rename("other".into()), true)
        .await
        .unwrap();
    current
        .apply_event("one".into(), ApplicationEvent::Append(2), true)
        .await
        .unwrap();
    let expected = Record { name: "original".into(), values: vec![1, 2] };
    assert_eq!(current.read("one", Clone::clone), Some(expected.clone()));
    assert_eq!(current.get_indexed_values("name", "original"), vec!["one"]);
    current.flush().await.unwrap();
    drop(current);
    let restored = engine(data.path(), false);
    restored.replay_events::<ApplicationEvent>().unwrap();
    assert_eq!(restored.read("one", Clone::clone), Some(expected));
}

#[tokio::test]
async fn warm_legacy_version_zero_requires_an_actual_recovered_record() {
    let data = tempfile::tempdir().unwrap();
    let snapshot = data.path().join("state.raftsnap");
    std::fs::write(
        &snapshot,
        r#"{"one":{"version":0,"last_updated":0,"data":{"name":"one","values":[42]}}}"#,
    )
    .unwrap();
    let mut current = engine(data.path(), true);
    Arc::get_mut(&mut current).unwrap().enable_retention(
        lithair_core::lifecycle::RetentionConfig { memory_count: Some(0), ..Default::default() },
        vec!["name".into()],
    );
    current.replay_events::<Change>().unwrap();
    assert_eq!(
        current.read_or_load::<_, _, Change>("one", |r| r.values.clone()),
        Some(vec![42])
    );
    // A legacy raw snapshot has no checksum. Losing this record must not make
    // an absent history look like a successfully recovered version-zero state.
    std::fs::write(snapshot, "{}").unwrap();
    assert!(current.apply_event("one".into(), change("after", 1), true).await.is_err());
    assert_eq!(current.read_or_load::<_, _, Change>("one", Clone::clone), None);
    assert_eq!(current.get_indexed_values("name", "one"), vec!["one"]);
}
