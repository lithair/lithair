//! Regression tests for builder-driven auto-compaction (issue #69).
//!
//! These tests pin down two complementary properties:
//!
//! 1. **Triggers above threshold** — when the number of appended events
//!    exceeds `events_threshold`, the background task calls
//!    `EventStore::truncate_events`, dropping `event_count()` back to zero.
//!
//! 2. **Does not trigger below threshold** — when the count stays at or
//!    below `events_threshold`, the background task leaves the log alone
//!    (no spurious compactions, count is preserved).
//!
//! The tests run the same loop body that lives in
//! `LithairServer::serve()` directly against an `EventStore` (the
//! framework primitives, untouched by this feature) to avoid spinning up
//! a full HTTP server in unit tests. The builder-level `with_auto_compaction`
//! flag is covered by builder-side tests in the same module.

use std::sync::Arc;
use std::time::Duration;

use lithair_core::engine::events::{EventEnvelope, EventStore};
use lithair_core::engine::AutoCompactionConfig;
use tempfile::TempDir;
use tokio::sync::RwLock;

/// Build a fresh, isolated `EventStore` backed by a temp dir.
fn make_store() -> (Arc<RwLock<EventStore>>, TempDir) {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("events.raftlog");
    let store = EventStore::new(path.to_str().unwrap()).expect("new EventStore");
    (Arc::new(RwLock::new(store)), tmp)
}

/// Append `n` synthetic envelopes to `store`. Each envelope is a small
/// full-object dump, matching the shape produced by
/// `DeclarativeHttpHandler::persist_to_event_store` (the call site that
/// motivated issue #69).
async fn append_n(store: &Arc<RwLock<EventStore>>, n: usize) {
    let mut guard = store.write().await;
    for i in 0..n {
        let envelope = EventEnvelope::new(
            "TestModel.Created".to_string(),
            format!("test-{}", i),
            i as u64,
            format!(r#"{{"id":"{}","value":{}}}"#, i, i),
            Some(format!("agg-{}", i)),
            None,
        );
        guard.append_envelope(&envelope).expect("append envelope");
    }
    guard.flush_events().expect("flush");
}

/// Mirror of the loop body in `LithairServer::serve()` (see
/// `lithair-core/src/app/mod.rs`, auto-compaction block). Kept in sync
/// manually — if the production loop diverges, both should be updated.
fn spawn_auto_compaction(
    event_store: Arc<RwLock<EventStore>>,
    cfg: AutoCompactionConfig,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(cfg.check_interval);
        ticker.tick().await; // skip immediate first tick
        loop {
            ticker.tick().await;
            let needs_compaction = {
                let store = event_store.read().await;
                store.event_count() > cfg.events_threshold
            };
            if !needs_compaction {
                continue;
            }
            let mut store = event_store.write().await;
            let _ = store.truncate_events();
        }
    })
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn auto_compaction_triggers_above_threshold() {
    // Threshold = 5 — small for fast tests. Real consumers use 10_000.
    let cfg = AutoCompactionConfig::new(5, Duration::from_millis(100)).expect("valid cfg");
    let (store, _tmp) = make_store();

    // Append 10 events — well above threshold.
    append_n(&store, 10).await;
    assert_eq!(store.read().await.event_count(), 10, "10 events appended");

    // Spawn the loop and let it reach its first `.tick()` (which resolves
    // immediately for `tokio::time::interval` — we consume it inside the
    // task to skip the spurious zero-time tick).
    let handle = spawn_auto_compaction(Arc::clone(&store), cfg);
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }

    // Drive the spawned task through several full intervals. On
    // `current_thread`, the spawned task wakes on the tick, takes the
    // read lock, drops it, then needs another scheduling pass before
    // acquiring the write lock to truncate. Loop a few times instead of
    // betting on a single advance+yield being enough.
    for _ in 0..5 {
        tokio::time::advance(Duration::from_millis(150)).await;
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
        if store.read().await.event_count() == 0 {
            break;
        }
    }

    let count_after = store.read().await.event_count();
    assert_eq!(
        count_after, 0,
        "expected compaction to truncate the log (count_before=10, threshold=5), got {} events remaining",
        count_after
    );

    handle.abort();
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn auto_compaction_does_not_trigger_below_threshold() {
    let cfg = AutoCompactionConfig::new(100, Duration::from_millis(100)).expect("valid cfg");
    let (store, _tmp) = make_store();

    // Append 10 events — well below threshold (100).
    append_n(&store, 10).await;
    assert_eq!(store.read().await.event_count(), 10, "10 events appended");

    let handle = spawn_auto_compaction(Arc::clone(&store), cfg);

    // Advance through several check intervals.
    for _ in 0..5 {
        tokio::time::advance(Duration::from_millis(150)).await;
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
    }

    let count_after = store.read().await.event_count();
    assert_eq!(
        count_after, 10,
        "expected log to be untouched (10 events <= threshold 100), got {} events",
        count_after
    );

    handle.abort();
}

#[test]
fn auto_compaction_config_rejects_zero_threshold() {
    // `events_threshold = 0` would compact on every tick, defeating the
    // point of having a threshold. The constructor must reject it so the
    // builder layer can surface the misconfiguration.
    assert!(AutoCompactionConfig::new(0, Duration::from_secs(1)).is_none());
    assert!(AutoCompactionConfig::new(1, Duration::from_secs(1)).is_some());
}

#[test]
fn auto_compaction_config_default_uses_default_snapshot_threshold() {
    // The default config aligns with `DEFAULT_SNAPSHOT_THRESHOLD` so
    // consumers who just want "the framework's recommendation" can call
    // `AutoCompactionConfig::default()` and get a sensible value.
    let cfg = AutoCompactionConfig::default();
    assert_eq!(cfg.events_threshold, lithair_core::engine::DEFAULT_SNAPSHOT_THRESHOLD);
    assert!(cfg.check_interval >= Duration::from_secs(1));
}
