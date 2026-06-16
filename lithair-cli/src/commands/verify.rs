//! `lithair verify <data-dir>` — offline event-store integrity check.
//!
//! Opens the event store at `<data-dir>` and walks its hash chain via
//! `EventStore::verify_chain()`, printing a human-readable summary. An
//! operator runs this against a *restored* backup before starting the server,
//! so a bad restore is caught before it becomes the live source of truth.
//!
//! Exit codes (scriptable):
//! - `0` — chain is valid (no tampered hashes, no broken links).
//! - `1` — chain is INVALID (tamper/corruption detected).
//! - `2` — the store could not be opened (bad path, unreadable files).
//!
//! Binary-mode logs (`LT_ENABLE_BINARY=true`) are detected from the same env
//! var the server uses, so the same invocation works for either log format.

use std::path::Path;

use lithair_core::engine::events::EventStore;

/// Run `lithair verify <data_dir>`. Returns the process exit code.
pub fn run(data_dir: &Path) -> i32 {
    let path = data_dir.to_string_lossy();

    if !data_dir.exists() {
        eprintln!("verify: data directory does not exist: {}", path);
        return 2;
    }

    let store = match EventStore::new(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("verify: failed to open event store at {}: {}", path, e);
            return 2;
        }
    };

    let result = match store.verify_chain() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("verify: chain verification failed to run at {}: {}", path, e);
            return 2;
        }
    };

    // Human summary.
    println!("Event store: {}", path);
    println!("  total events:    {}", result.total_events);
    println!("  verified:        {}", result.verified_events);
    if result.legacy_events > 0 {
        println!("  legacy (no hash):{}", result.legacy_events);
    }
    println!("  invalid hashes:  {}", result.invalid_hashes.len());
    println!("  broken links:    {}", result.broken_links.len());

    if result.is_valid {
        println!("Result: OK — {}", result.summary());
        0
    } else {
        // Report the first offending event so the operator has a starting
        // point. `invalid_hashes` and `broken_links` are ordered by index.
        if let Some(first) = result.invalid_hashes.first() {
            eprintln!(
                "  first bad hash:  index {} (event_id {})",
                first.event_index, first.event_id
            );
        }
        if let Some(first) = result.broken_links.first() {
            eprintln!(
                "  first broken link: index {} (event_id {})",
                first.event_index, first.event_id
            );
        }
        eprintln!("Result: INVALID — {}", result.summary());
        1
    }
}
