//! Regression tests for builder-driven auto-compaction (issue #69).
//!
//! These tests pin down four complementary properties:
//!
//! 1. **Triggers above threshold** — when the number of appended events
//!    exceeds `events_threshold`, the background task calls the handler's
//!    `compact()`, which snapshots state then truncates the log, dropping
//!    `event_count()` back to zero.
//!
//! 2. **Does not trigger below threshold** — when the count stays at or
//!    below `events_threshold`, the background task leaves the log alone
//!    (no spurious compactions, count is preserved).
//!
//! 3. **State survives compaction + restart** — the critical property
//!    that motivated the Gemini review on PR #84. After `compact()`, the
//!    on-disk events log is empty but a snapshot file holds the full
//!    state — reopening the handler against the same data dir must
//!    reconstruct storage from the snapshot.
//!
//! 4. **Config sanity** — `events_threshold = 0` is rejected; default
//!    config aligns with `DEFAULT_SNAPSHOT_THRESHOLD`.
//!
//! The low-level loop body is exercised against a raw `EventStore` (the
//! framework primitives) to avoid spinning up a full HTTP server. The
//! state-survives-restart property is exercised through the real
//! `DeclarativeHttpHandler::compact()` path — the same code the server
//! calls from its background task.

use std::sync::Arc;
use std::time::Duration;

use lithair_core::engine::events::{EventEnvelope, EventStore};
use lithair_core::engine::AutoCompactionConfig;
use lithair_core::http::DeclarativeHttpHandler;
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};
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

/// Mirror of the loop body in `LithairServer::serve()` for tests that
/// don't need the full handler path. See `lithair-core/src/app/mod.rs`,
/// auto-compaction block. Kept in sync manually — if the production loop
/// diverges, both should be updated.
///
/// Note: this helper calls `truncate_events()` directly because the raw
/// `EventStore` has no concept of model state to snapshot. The
/// state-survives-restart property is tested separately via
/// `DeclarativeHttpHandler::compact()` (the real production path).
fn spawn_auto_compaction(
    event_store: Arc<RwLock<EventStore>>,
    cfg: AutoCompactionConfig,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(cfg.check_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
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

// ---------------------------------------------------------------------------
// State-survives-restart tests — addresses the critical Gemini comment on
// PR #84 (data-loss bug). Necessary because the "event count drops to zero"
// assertion above is necessary but not sufficient: a buggy implementation
// could also drop the count to zero by simply wiping the log without
// snapshotting state first. The tests below recreate the handler against
// the same data dir after compaction and assert items are still there.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
struct Widget {
    #[http(expose)]
    id: String,
    #[http(expose)]
    name: String,
    #[http(expose)]
    weight: u64,
}

/// Helper: build a handler against `data_dir` and replay any persisted
/// events + snapshot. Mirrors what `LithairServerBuilder` does at startup.
async fn open_handler(data_dir: &std::path::Path) -> DeclarativeHttpHandler<Widget> {
    let path = data_dir.to_string_lossy().to_string();
    DeclarativeHttpHandler::<Widget>::new_with_replay(&path)
        .await
        .expect("open handler")
}

#[tokio::test(flavor = "current_thread")]
async fn compact_then_reopen_preserves_state() {
    // The headline data-loss regression: write items, compact, drop the
    // handler, reopen against the same data dir, assert items are still
    // visible. With the original PR #84 code (truncate without snapshot)
    // this test would fail — the reopened handler would see an empty
    // events log, no snapshot, and reconstruct an empty storage map.

    let tmp = TempDir::new().expect("tempdir");
    let data_dir = tmp.path().join("widgets");
    std::fs::create_dir_all(&data_dir).expect("create data dir");

    // 1. Open a handler, persist 12 items (so compact triggers above a
    //    threshold of 5). `apply_replicated_item` writes to storage AND
    //    appends to the event store, matching the production code path.
    let handler = open_handler(&data_dir).await;
    for i in 0..12u64 {
        handler
            .apply_replicated_item(Widget {
                id: format!("w-{}", i),
                name: format!("widget-{}", i),
                weight: i * 10,
            })
            .await
            .expect("apply replicated item");
    }
    // Flush any pending batched writes before compacting.
    {
        let mut store = handler.get_event_store().write().await;
        store.flush_events().expect("flush before compact");
    }
    assert_eq!(handler.get_all_items().await.len(), 12, "12 items in storage pre-compact");
    assert!(
        handler.get_event_store().read().await.event_count() > 0,
        "events appended pre-compact"
    );

    // 2. Trigger compaction through the real production path. This is
    //    what the server-side spawn loop calls on each threshold cross.
    handler.compact().await.expect("compact ok");
    assert_eq!(
        handler.get_event_store().read().await.event_count(),
        0,
        "events log truncated post-compact"
    );

    // 3. Drop the handler explicitly to release file handles, then reopen
    //    against the same data dir. This simulates a process restart.
    drop(handler);
    let reopened = open_handler(&data_dir).await;

    // 4. The critical assertion: all 12 items must be visible. If the
    //    snapshot was not written before truncate, this would be empty.
    let items = reopened.get_all_items().await;
    assert_eq!(
        items.len(),
        12,
        "state must survive compaction + restart (data-loss regression: PR #84 Gemini review)"
    );

    // 5. Spot-check item identity and field values — not just count.
    //    Use a set comparison rather than a sorted list (string-sort of
    //    "w-10" lands before "w-2", which is not what we mean by
    //    "all ids present").
    let ids: std::collections::HashSet<String> = items.iter().map(|w| w.id.clone()).collect();
    let expected: std::collections::HashSet<String> =
        (0..12u64).map(|i| format!("w-{}", i)).collect();
    assert_eq!(ids, expected, "all original ids must be present");
    let by_id: std::collections::HashMap<String, &Widget> =
        items.iter().map(|w| (w.id.clone(), w)).collect();
    for i in 0..12u64 {
        let w = by_id.get(&format!("w-{}", i)).expect("item present");
        assert_eq!(w.weight, i * 10, "field values preserved across snapshot round-trip");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn compact_then_append_then_reopen_replays_both() {
    // Defense-in-depth: after compaction, new events should still append
    // to the log, and a restart should reconstruct state from
    // (snapshot + post-snapshot events). Catches a buggy `replay_events`
    // that loads the snapshot but skips events, or vice versa.

    let tmp = TempDir::new().expect("tempdir");
    let data_dir = tmp.path().join("widgets");
    std::fs::create_dir_all(&data_dir).expect("create data dir");

    let handler = open_handler(&data_dir).await;
    // Phase 1: 6 items → snapshot.
    for i in 0..6u64 {
        handler
            .apply_replicated_item(Widget {
                id: format!("pre-{}", i),
                name: format!("pre-{}", i),
                weight: i,
            })
            .await
            .expect("apply pre");
    }
    {
        let mut store = handler.get_event_store().write().await;
        store.flush_events().expect("flush");
    }
    handler.compact().await.expect("compact");

    // Phase 2: 3 more items after the snapshot.
    for i in 0..3u64 {
        handler
            .apply_replicated_item(Widget {
                id: format!("post-{}", i),
                name: format!("post-{}", i),
                weight: 100 + i,
            })
            .await
            .expect("apply post");
    }
    {
        let mut store = handler.get_event_store().write().await;
        store.flush_events().expect("flush");
    }

    drop(handler);
    let reopened = open_handler(&data_dir).await;
    let items = reopened.get_all_items().await;
    assert_eq!(
        items.len(),
        9,
        "snapshot (6) + post-snapshot events (3) = 9 items after restart"
    );
    let ids: std::collections::HashSet<String> = items.iter().map(|w| w.id.clone()).collect();
    for i in 0..6u64 {
        assert!(ids.contains(&format!("pre-{}", i)), "pre-snapshot item {} present", i);
    }
    for i in 0..3u64 {
        assert!(ids.contains(&format!("post-{}", i)), "post-snapshot item {} present", i);
    }
}
