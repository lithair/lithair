use super::*;
use crate::engine::EventStore;
use std::cell::RefCell;

thread_local! { static CRASH: RefCell<Option<String>> = const { RefCell::new(None) }; }
pub(super) fn stage(name: &str) {
    CRASH.with(|target| {
        if target.borrow().as_deref() == Some(name) {
            std::process::exit(86);
        }
    });
}

#[test]
fn crash_child() {
    let Ok(path) = std::env::var("NATIVE_CHECKPOINT_TEST_CHILD_DIR") else {
        return;
    };
    let mut store = EventStore::new(&path).unwrap();
    let crash = std::env::var("NATIVE_CHECKPOINT_TEST_CRASH_STAGE").unwrap();
    if std::env::var("NATIVE_CHECKPOINT_TEST_CRASH_OPERATION").unwrap() == "snapshot" {
        CRASH.with(|v| *v.borrow_mut() = Some(crash));
        store.save_snapshot("2").unwrap();
    } else {
        store.save_snapshot("2").unwrap();
        CRASH.with(|v| *v.borrow_mut() = Some(crash));
        store.truncate_events().unwrap();
    }
    panic!("crash stage was not reached");
}

#[test]
fn process_interruptions_select_snapshot_and_suffix_together() {
    for operation in ["snapshot", "compact"] {
        let extra = if operation == "snapshot" { "journal_synced" } else { "new_journal_synced" };
        for stage in [
            extra,
            "snapshot_written",
            "snapshot_synced",
            "snapshot_renamed",
            "directory_synced",
        ] {
            crash_case(operation, stage);
        }
    }
    crash_case("compact", "reclaimed");
}
fn crash_case(operation: &str, stage: &str) {
    let dir = tempfile::tempdir().unwrap();
    {
        let mut store = EventStore::new(dir.path().to_str().unwrap()).unwrap();
        store.append_raw_line("1").unwrap();
        store.flush().unwrap();
        store.save_snapshot("1").unwrap();
        store.truncate_events().unwrap();
        store.append_raw_line("1").unwrap();
        store.force_flush().unwrap();
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "engine::persistence::checkpoint::tests::crash_child", "--nocapture"])
        .env("NATIVE_CHECKPOINT_TEST_CHILD_DIR", dir.path())
        .env("NATIVE_CHECKPOINT_TEST_CRASH_STAGE", stage)
        .env("NATIVE_CHECKPOINT_TEST_CRASH_OPERATION", operation)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(86),
        "{operation}/{stage}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut recovered = EventStore::new(dir.path().to_str().unwrap()).unwrap();
    let (state, suffix) = recovered.recovery_state().unwrap();
    let value: u64 = serde_json::from_str(&state.unwrap()).unwrap();
    assert_eq!(value + suffix.len() as u64, 2, "{operation}/{stage}");
    recovered.append_raw_line("1").unwrap();
    recovered.flush().unwrap();
    recovered.save_snapshot("3").unwrap();
    recovered.truncate_events().unwrap();
    let reopened = EventStore::new(dir.path().to_str().unwrap()).unwrap();
    assert_eq!(reopened.recovery_state().unwrap(), (Some("3".into()), vec![]));
}

#[test]
fn publication_error_keeps_previous_journal_and_stops_writes() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = EventStore::new(dir.path().to_str().unwrap()).unwrap();
    store.append_raw_line("1").unwrap();
    store.flush().unwrap();
    fs::create_dir(dir.path().join("state.raftsnap")).unwrap();
    assert!(store.save_snapshot("1").is_err());
    assert!(store.append_raw_line("2").is_err());
    assert_eq!(store.get_all_events().unwrap(), vec!["1"]);
    fs::remove_dir(dir.path().join("state.raftsnap")).unwrap();
    let reopened = EventStore::new(dir.path().to_str().unwrap()).unwrap();
    assert_eq!(reopened.get_all_events().unwrap(), vec!["1"]);
}
