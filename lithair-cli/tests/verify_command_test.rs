//! Integration tests for `lithair verify <data-dir>` (issue #133).
//!
//! Runs the actual compiled `lithair` binary (via `CARGO_BIN_EXE_lithair`)
//! against a good event store and a tampered one, asserting the human output
//! and the scriptable exit codes (0 valid, 1 invalid, 2 unreadable).
//!
//! The event stores are built directly with `lithair-core` (a normal
//! dependency of the CLI), so the fixtures use the same on-disk format the
//! command reads.

use std::path::Path;
use std::process::Command;

use lithair_core::engine::events::{EventEnvelope, EventStore};
use lithair_core::engine::persistence::calculate_crc32;

/// Build a small, valid event store with `n` hash-chained events in `dir`.
fn make_good_store(dir: &Path, n: u64) {
    std::fs::create_dir_all(dir).expect("create data dir");
    let path = dir.to_string_lossy().to_string();
    let mut store = EventStore::new(&path).expect("open store");
    for i in 0..n {
        // Leave `event_hash`/`previous_hash` unset so `append_envelope`
        // computes the hash chain itself — the same path the real handler
        // uses (`persist_to_event_store`). Using the hash-setting
        // `EventEnvelope::new` here would freeze `previous_hash = None` on
        // every event and produce a deliberately broken chain.
        let env = EventEnvelope {
            event_type: "Article.Created".to_string(),
            event_id: format!("evt-{}", i),
            timestamp: 1_700_000_000 + i,
            payload: format!(r#"{{"id":"art-{}","title":"t{}"}}"#, i, i),
            aggregate_id: Some(format!("art-{}", i)),
            event_hash: None,
            previous_hash: None,
        };
        store.append_envelope(&env).expect("append");
    }
    store.flush_events().expect("flush");
}

/// Tamper the first record in `dir`'s log so it survives CRC32 but fails the
/// hash chain (recompute the CRC over altered payload, leaving the stored
/// event_hash stale). Mirrors the offline-verify drill in lithair-core.
fn tamper_first_record(dir: &Path) {
    let log = dir.join("events.raftlog");
    let original = std::fs::read_to_string(&log).expect("read log");
    let mut lines: Vec<String> = original.lines().map(|s| s.to_string()).collect();
    assert!(!lines.is_empty(), "log has lines to tamper");
    let first = &lines[0];
    assert!(first.len() > 9 && first.as_bytes()[8] == b':', "CRC32-prefixed line");
    let json = &first[9..];
    // The record payload is an *escaped* JSON string inside the envelope, so
    // in the raw log line the inner quotes are backslash-escaped. Match the
    // same-length token "t0" inside it and bump it to "HACKED" (longer is
    // fine — CRC is recomputed below; only the stale event_hash matters).
    let tampered = json.replacen(r#"title\":\"t0\""#, r#"title\":\"HACKED\""#, 1);
    assert_ne!(tampered, json, "tamper changed the payload (line was: {})", json);
    let crc = calculate_crc32(tampered.as_bytes());
    lines[0] = format!("{:08x}:{}", crc, tampered);
    let mut rebuilt = lines.join("\n");
    rebuilt.push('\n');
    std::fs::write(&log, rebuilt.as_bytes()).expect("write tampered log");
}

fn run_verify(data_dir: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_lithair"))
        .arg("verify")
        .arg(data_dir)
        .output()
        .expect("run lithair verify")
}

#[test]
fn verify_good_store_exits_zero() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let dir = tmp.path().join("good");
    make_good_store(&dir, 10);

    let out = run_verify(&dir);
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert_eq!(out.status.code(), Some(0), "valid chain exits 0; stdout:\n{}", stdout);
    assert!(stdout.contains("total events:"), "summary lists total events:\n{}", stdout);
    assert!(stdout.contains("Result: OK"), "reports OK:\n{}", stdout);
    assert!(
        stdout.contains("total events:    10") || stdout.contains("total events:    10\n"),
        "reports the 10 events:\n{}",
        stdout
    );
}

#[test]
fn verify_tampered_store_exits_one() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let dir = tmp.path().join("tampered");
    make_good_store(&dir, 10);
    tamper_first_record(&dir);

    let out = run_verify(&dir);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(
        out.status.code(),
        Some(1),
        "tampered chain exits 1; stdout:\n{}\nstderr:\n{}",
        stdout,
        stderr
    );
    assert!(stdout.contains("INVALID"), "reports INVALID on stdout:\n{}", stdout);
    assert!(
        stdout.contains("first bad hash") || stdout.contains("first broken link"),
        "points at the first offending event on stdout:\n{}",
        stdout
    );
}

#[test]
fn verify_missing_dir_exits_two() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let missing = tmp.path().join("does-not-exist");

    let out = run_verify(&missing);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(out.status.code(), Some(2), "unreadable store exits 2");
    assert!(stderr.contains("does not exist"), "explains the missing dir:\n{}", stderr);
}
