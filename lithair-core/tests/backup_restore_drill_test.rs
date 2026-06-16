//! End-to-end backup/restore drill (issue #133, v1.0 gate G7).
//!
//! Proves the procedure documented in `docs/operations/backup-restore.md`
//! actually works — specifically **Strategy B**, the physical event-store
//! backup, in its cold/consistent posture. An untested backup is a guess;
//! these tests turn the runbook's claims into executed assertions.
//!
//! The headline drill (`physical_backup_restore_drill_*`) performs the exact
//! sequence an operator runs:
//!
//!   1. write N records to data dir A,
//!   2. **force a deterministic flush** so the on-disk log is complete (the
//!      runbook's "quiescent, internally consistent set" — see the flush note
//!      below),
//!   3. copy every data-dir file from A to backup dir B (the `aws s3 sync`
//!      / `tar` step),
//!   4. drop the handler and **wipe** A (the `rm -rf /var/lib/lithair/*`
//!      step),
//!   5. copy B back into A (the restore `s3 sync` / `tar xzf` step),
//!   6. reopen with `new_with_replay(A)` (the "start the server, let replay
//!      run" step) and assert field-level identity, chain validity, and that
//!      a post-restore write continues the chain cleanly.
//!
//! ## The flush-determinism point
//!
//! A live `LithairServer` runs a background flusher on a timer
//! (`flush_events()` every `LT_FLUSH_INTERVAL_MS`, default 100ms). A
//! deterministic test must NOT `sleep` and hope the timer fired — it forces
//! the flush itself by taking the event-store write lock and calling
//! `flush_events()`. Operationally this corresponds to the runbook's cold
//! posture: stop/drain the server (which flushes on the way down) before
//! copying. The drill makes that step explicit and the doc was updated to
//! spell it out for an operator copying a *running* store.
//!
//! Both JSON and binary (`LT_ENABLE_BINARY`) log modes are covered, because
//! the runbook claims torn-tail tolerance for both. Mode is selected by
//! constructing the handler under a process-wide env lock (env vars are
//! process-global; the lock serializes the JSON and binary drills so they
//! cannot observe each other's `LT_ENABLE_BINARY`).

use std::collections::HashMap;
use std::path::Path;

use lithair_core::engine::events::EventStore;
use lithair_core::engine::persistence::calculate_crc32;
use lithair_core::http::DeclarativeHttpHandler;
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};
use tempfile::TempDir;
use tokio::sync::Mutex;

/// Serializes tests that toggle the process-global `LT_ENABLE_BINARY` env
/// var so the JSON and binary drills never race on it. A `tokio::sync::Mutex`
/// (not `std::sync::Mutex`) so the guard can be held across `.await` without
/// the `await_holding_lock` hazard, and so a panicking drill does not poison
/// the lock for the others.
static ENV_LOCK: Mutex<()> = Mutex::const_new(());

/// A model with deliberately varied field types so the drill proves
/// field-level identity across the full serde round-trip, not just counts:
/// a String, an i64 money field (cents — the "decimal-ish" amount), a
/// timestamp (epoch millis), an `Option`, and a `Vec`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, DeclarativeModel)]
struct Invoice {
    #[http(expose)]
    id: String,
    #[http(expose)]
    customer: String,
    /// Money as integer cents — exact, no float rounding.
    #[http(expose)]
    amount_cents: i64,
    /// Issue time as epoch milliseconds (DateTime-ish, but dependency-free).
    #[http(expose)]
    issued_at_ms: i64,
    /// Optional memo — exercises Some/None round-trip.
    #[http(expose)]
    memo: Option<String>,
    /// Line-item SKUs — exercises Vec round-trip.
    #[http(expose)]
    line_items: Vec<String>,
}

/// Deterministic record generator: record `i` has stable, checkable content.
fn make_invoice(i: usize) -> Invoice {
    Invoice {
        id: format!("inv-{:04}", i),
        customer: format!("customer-{}", i % 7),
        // Spread amounts so two records rarely collide; keep them exact.
        amount_cents: 1_000 + (i as i64) * 137,
        issued_at_ms: 1_700_000_000_000 + (i as i64) * 86_400_000,
        // Every third invoice has no memo — covers None and Some.
        memo: if i.is_multiple_of(3) { None } else { Some(format!("memo for invoice {}", i)) },
        // Variable-length Vec including an empty one.
        line_items: (0..(i % 4)).map(|k| format!("sku-{}-{}", i, k)).collect(),
    }
}

/// Copy every regular file in `from` into `to` (non-recursive — the event
/// store keeps all its files flat in the data dir). Mirrors `aws s3 sync` /
/// `tar` of the whole directory.
fn copy_dir_files(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("create dest dir");
    for entry in std::fs::read_dir(from).expect("read source dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.is_file() {
            let dest = to.join(entry.file_name());
            std::fs::copy(&path, &dest).expect("copy file");
        }
    }
}

/// Wipe the contents of a directory (keep the directory itself). Mirrors the
/// runbook's `sudo rm -rf /var/lib/lithair/*`.
fn wipe_dir_contents(dir: &Path) {
    for entry in std::fs::read_dir(dir).expect("read dir to wipe") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.is_file() {
            std::fs::remove_file(&path).expect("remove file");
        } else if path.is_dir() {
            std::fs::remove_dir_all(&path).expect("remove subdir");
        }
    }
}

/// List the data-dir files present (for asserting a backup actually captured
/// the matched set described in the runbook).
fn file_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("read dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    names
}

/// The full drill, parameterized over log mode. `binary` selects the mode the
/// handler is built under; the caller holds `ENV_LOCK` for the whole call.
async fn run_drill(binary: bool) {
    const N: usize = 50;

    let prior_env = std::env::var("LT_ENABLE_BINARY").ok();
    if binary {
        std::env::set_var("LT_ENABLE_BINARY", "true");
    } else {
        std::env::remove_var("LT_ENABLE_BINARY");
    }

    // Restore the env var on the way out so we never leak mode into a
    // sibling test even if an assertion panics mid-drill.
    struct EnvGuard(Option<String>);
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.0 {
                Some(v) => std::env::set_var("LT_ENABLE_BINARY", v),
                None => std::env::remove_var("LT_ENABLE_BINARY"),
            }
        }
    }
    let _env_guard = EnvGuard(prior_env);

    let root = TempDir::new().expect("tempdir");
    let dir_a = root.path().join("live"); // the live data dir
    let dir_b = root.path().join("backup"); // off-site backup copy
    std::fs::create_dir_all(&dir_a).expect("create live dir");
    let path_a = dir_a.to_string_lossy().to_string();

    // ---- 1. Write N records with known content ----------------------------
    let expected: Vec<Invoice> = (0..N).map(make_invoice).collect();
    {
        let handler = DeclarativeHttpHandler::<Invoice>::new_with_replay(&path_a)
            .await
            .expect("open live handler");
        for inv in &expected {
            handler.apply_replicated_item(inv.clone()).await.expect("write record");
        }
        assert_eq!(handler.get_all_items().await.len(), N, "all N records in memory pre-backup");

        // ---- 2. Force a deterministic flush (NOT a sleep) -----------------
        // Take the event-store write lock and flush so the on-disk log holds
        // every event before we copy. This is the test analogue of the
        // runbook's cold posture (stop/drain flushes before the dir is copied).
        {
            let mut store = handler.get_event_store().write().await;
            store.flush_events().expect("force flush before backup");
        }

        // Sanity: a valid chain BEFORE we ever touch the files, so any later
        // failure is attributable to backup/restore, not to the write path.
        {
            let store = handler.get_event_store().read().await;
            let pre = store.verify_chain().expect("verify pre-backup");
            assert!(pre.is_valid, "chain valid before backup: {}", pre.summary());
            assert_eq!(pre.total_events, N, "exactly N events on disk pre-backup");
        }

        // ---- 3. Back up: copy all data-dir files A -> B -------------------
        copy_dir_files(&dir_a, &dir_b);

        // 4 (part). Drop the handler to release file handles, simulating a
        // stopped service before the destructive wipe.
        drop(handler);
    }

    // The backup must contain the append-only log at minimum. (Index/meta
    // files are present too; we assert the log is there since it is the
    // source of truth.)
    let backed_up = file_names(&dir_b);
    assert!(
        backed_up.iter().any(|f| f == "events.raftlog"),
        "backup captured the event log; got {:?}",
        backed_up
    );

    // ---- 4. Wipe the live dir A ------------------------------------------
    wipe_dir_contents(&dir_a);
    assert!(file_names(&dir_a).is_empty(), "live dir wiped clean before restore");

    // ---- 5. Restore: copy B back into A ----------------------------------
    copy_dir_files(&dir_b, &dir_a);

    // ---- 6. Reopen and assert --------------------------------------------
    let restored = DeclarativeHttpHandler::<Invoice>::new_with_replay(&path_a)
        .await
        .expect("reopen restored handler");

    let items = restored.get_all_items().await;
    assert_eq!(
        items.len(),
        N,
        "record count survives backup -> wipe -> restore (mode binary={binary})"
    );

    // Field-level identity for EVERY record, not just count. Index by id and
    // compare the whole struct (derive PartialEq) against what was written.
    let by_id: HashMap<String, Invoice> = items.into_iter().map(|i| (i.id.clone(), i)).collect();
    for want in &expected {
        let got = by_id
            .get(&want.id)
            .unwrap_or_else(|| panic!("record {} missing after restore", want.id));
        assert_eq!(got, want, "field-level identity for {} (mode binary={binary})", want.id);
    }

    // Chain integrity on the RESTORED store.
    {
        let store = restored.get_event_store().read().await;
        let result = store.verify_chain().expect("verify_chain on restored store");
        assert!(result.is_valid, "restored chain valid: {}", result.summary());
        assert_eq!(result.total_events, N, "restored store has exactly N events");
        assert!(result.invalid_hashes.is_empty(), "no tampered hashes after restore");
        assert!(result.broken_links.is_empty(), "no broken links after restore");
    }

    // A write AFTER restore must succeed and keep the chain valid — proves
    // the restore did not corrupt the index/offset/last-hash continuity.
    let post = make_invoice(N + 1);
    restored.apply_replicated_item(post.clone()).await.expect("post-restore write");
    {
        let mut store = restored.get_event_store().write().await;
        store.flush_events().expect("flush post-restore write");
    }
    {
        let store = restored.get_event_store().read().await;
        let result = store.verify_chain().expect("verify_chain after post-restore write");
        assert!(
            result.is_valid,
            "chain still valid after post-restore write: {}",
            result.summary()
        );
        assert_eq!(result.total_events, N + 1, "post-restore write appended one event");
    }
    assert_eq!(
        restored.get_by_id(&post.id).await.as_ref(),
        Some(&post),
        "post-restore record is readable and identical"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn physical_backup_restore_drill_json() {
    // Ignore poisoning: a panic in one drill must not cascade into the
    // others as a misleading "env lock" failure. The env guard in each drill
    // restores `LT_ENABLE_BINARY` on unwind regardless.
    let _g = ENV_LOCK.lock().await;
    run_drill(false).await;
}

#[tokio::test(flavor = "current_thread")]
async fn physical_backup_restore_drill_binary() {
    // Ignore poisoning: a panic in one drill must not cascade into the
    // others as a misleading "env lock" failure. The env guard in each drill
    // restores `LT_ENABLE_BINARY` on unwind regardless.
    let _g = ENV_LOCK.lock().await;
    run_drill(true).await;
}

// ---------------------------------------------------------------------------
// Torn-tail case — the runbook (Strategy B, "Hot / live backup") claims that
// a copy capturing a torn/partially-written FINAL record replays the intact
// prefix and drops only that last incomplete event, for both JSON and binary
// modes. These tests prove it: write N, back up, truncate the last record's
// bytes in the backup, restore, and assert count == N-1 with a structurally
// valid chain over the survivors.
// ---------------------------------------------------------------------------

/// Shared torn-tail drill. `binary` picks the mode; the caller holds the lock.
async fn run_torn_tail(binary: bool) {
    const N: usize = 20;

    let prior_env = std::env::var("LT_ENABLE_BINARY").ok();
    if binary {
        std::env::set_var("LT_ENABLE_BINARY", "true");
    } else {
        std::env::remove_var("LT_ENABLE_BINARY");
    }
    struct EnvGuard(Option<String>);
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.0 {
                Some(v) => std::env::set_var("LT_ENABLE_BINARY", v),
                None => std::env::remove_var("LT_ENABLE_BINARY"),
            }
        }
    }
    let _env_guard = EnvGuard(prior_env);

    let root = TempDir::new().expect("tempdir");
    let dir_a = root.path().join("live");
    let dir_b = root.path().join("backup");
    std::fs::create_dir_all(&dir_a).expect("create live dir");
    let path_a = dir_a.to_string_lossy().to_string();

    let expected: Vec<Invoice> = (0..N).map(make_invoice).collect();
    {
        let handler = DeclarativeHttpHandler::<Invoice>::new_with_replay(&path_a)
            .await
            .expect("open live handler");
        for inv in &expected {
            handler.apply_replicated_item(inv.clone()).await.expect("write record");
        }
        {
            let mut store = handler.get_event_store().write().await;
            store.flush_events().expect("force flush");
        }
        copy_dir_files(&dir_a, &dir_b);
        drop(handler);
    }

    // Corrupt the FINAL record in the backup copy by truncating the log file
    // by a handful of bytes. JSON mode: the last CRC32-prefixed line's
    // checksum no longer matches its (now-shortened) payload -> rejected.
    // Binary mode: the last length-prefixed frame's payload runs past EOF ->
    // reader stops cleanly at it. Either way the intact prefix survives.
    let log_b = dir_b.join("events.raftlog");
    let len = std::fs::metadata(&log_b).expect("log metadata").len();
    assert!(len > 16, "log must be non-trivial to truncate meaningfully");
    // Truncate enough to damage the last record without eating into the
    // second-to-last one. Records here are well over 12 bytes each.
    {
        let f = std::fs::OpenOptions::new()
            .write(true)
            .open(&log_b)
            .expect("open log for truncate");
        f.set_len(len - 12).expect("truncate last record's tail");
    }

    // Restore the torn backup into a clean live dir.
    wipe_dir_contents(&dir_a);
    copy_dir_files(&dir_b, &dir_a);

    let restored = DeclarativeHttpHandler::<Invoice>::new_with_replay(&path_a)
        .await
        .expect("reopen after torn-tail restore");

    let items = restored.get_all_items().await;
    assert_eq!(
        items.len(),
        N - 1,
        "torn final record dropped, intact prefix survives (mode binary={binary})"
    );

    // The surviving records are exactly the first N-1, byte-identical.
    let by_id: HashMap<String, Invoice> = items.into_iter().map(|i| (i.id.clone(), i)).collect();
    for want in expected.iter().take(N - 1) {
        let got = by_id.get(&want.id).unwrap_or_else(|| panic!("survivor {} missing", want.id));
        assert_eq!(got, want, "survivor {} is field-identical", want.id);
    }
    assert!(
        !by_id.contains_key(&expected[N - 1].id),
        "the torn final record ({}) must NOT have replayed",
        expected[N - 1].id
    );

    // The chain over the survivors is still structurally valid (no spurious
    // tampering/break reported for dropping a torn tail).
    {
        let store = restored.get_event_store().read().await;
        let result = store.verify_chain().expect("verify_chain over survivors");
        assert!(
            result.is_valid,
            "chain over the intact prefix is valid after dropping the torn tail: {}",
            result.summary()
        );
        assert_eq!(result.total_events, N - 1, "verify sees exactly the survivors");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn torn_tail_tolerated_json() {
    // Ignore poisoning: a panic in one drill must not cascade into the
    // others as a misleading "env lock" failure. The env guard in each drill
    // restores `LT_ENABLE_BINARY` on unwind regardless.
    let _g = ENV_LOCK.lock().await;
    run_torn_tail(false).await;
}

#[tokio::test(flavor = "current_thread")]
async fn torn_tail_tolerated_binary() {
    // Ignore poisoning: a panic in one drill must not cascade into the
    // others as a misleading "env lock" failure. The env guard in each drill
    // restores `LT_ENABLE_BINARY` on unwind regardless.
    let _g = ENV_LOCK.lock().await;
    run_torn_tail(true).await;
}

// ---------------------------------------------------------------------------
// Offline verify primitive — the same `verify_chain()` the `lithair verify`
// CLI exposes. Proves that an operator opening a RESTORED store with a plain
// `EventStore::new(<data-dir>)` (no handler, no server) can confirm integrity,
// and that a tampered store reports is_valid = false. This is the library
// contract the CLI wraps.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn offline_event_store_verifies_restored_dir() {
    // Ignore poisoning: a panic in one drill must not cascade into the
    // others as a misleading "env lock" failure. The env guard in each drill
    // restores `LT_ENABLE_BINARY` on unwind regardless.
    let _g = ENV_LOCK.lock().await;
    std::env::remove_var("LT_ENABLE_BINARY");

    let root = TempDir::new().expect("tempdir");
    let dir = root.path().join("data");
    std::fs::create_dir_all(&dir).expect("create data dir");
    let path = dir.to_string_lossy().to_string();

    {
        let handler = DeclarativeHttpHandler::<Invoice>::new_with_replay(&path)
            .await
            .expect("open handler");
        for i in 0..10 {
            handler.apply_replicated_item(make_invoice(i)).await.expect("write");
        }
        let mut store = handler.get_event_store().write().await;
        store.flush_events().expect("flush");
    }

    // Operator's offline check: open the dir directly and verify.
    let good = EventStore::new(&path).expect("open store offline");
    let result = good.verify_chain().expect("verify good store");
    assert!(result.is_valid, "freshly restored store verifies clean: {}", result.summary());
    assert_eq!(result.total_events, 10);

    // Tamper with the log the way a malicious actor would: change a record's
    // payload AND recompute its CRC32 prefix so the storage-layer checksum
    // still passes (a naive flip would just be dropped at read time). The
    // hash chain is what catches this — the stored `event_hash` was computed
    // over the original payload, so it no longer matches `compute_hash()` of
    // the altered payload, and `verify_chain()` must report `is_valid=false`.
    let log = dir.join("events.raftlog");
    let original = std::fs::read_to_string(&log).expect("read log as text");
    let mut lines: Vec<String> = original.lines().map(|l| l.to_string()).collect();
    assert!(!lines.is_empty(), "log must have at least one line to tamper");
    // Tamper the FIRST record: strip its CRC prefix, mutate the JSON payload
    // text, then re-prefix with a fresh, matching CRC32.
    let first = &lines[0];
    assert!(first.len() > 9 && first.as_bytes()[8] == b':', "expected CRC32-prefixed line");
    let json = &first[9..];
    // "customer-0" -> "customer-X": same length, valid JSON, but the stored
    // event_hash (over the original) will no longer match.
    let tampered_json = json.replacen("customer-0", "customer-X", 1);
    assert_ne!(tampered_json, json, "tamper must actually change the payload");
    let new_crc = calculate_crc32(tampered_json.as_bytes());
    lines[0] = format!("{:08x}:{}", new_crc, tampered_json);
    let mut rebuilt = lines.join("\n");
    rebuilt.push('\n');
    std::fs::write(&log, rebuilt.as_bytes()).expect("write tampered log");

    let tampered = EventStore::new(&path).expect("open tampered store offline");
    let result = tampered.verify_chain().expect("verify tampered store");
    assert!(
        !result.is_valid,
        "tampered store must report is_valid=false; got summary: {}",
        result.summary()
    );
}
