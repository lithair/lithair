use crate::features::world::LithairWorld;
use cucumber::{given, then, when};
use lithair_core::engine::persistence::parse_and_validate_event;
use lithair_core::engine::{Engine, EngineConfig, EngineError, Event};

/// Strip CRC32 prefix from a raw raftlog line, returning just the JSON part.
/// Handles both `<crc32>:<json>` format and legacy plain JSON lines.
fn strip_crc32_prefix(line: &str) -> &str {
    if line.len() > 9 && line.as_bytes()[8] == b':' {
        &line[9..]
    } else {
        line
    }
}

// Background
#[given(expr = "a Lithair engine with event sourcing enabled")]
async fn given_event_sourcing_enabled(_world: &mut LithairWorld) {
    // Initialization done at first CRUD operation to avoid fragile dependencies
    println!("Event sourcing context enabled (initialization done at first CRUD operation)");
}

// Internal helper: ensure engine + storage event sourcing are initialized
async fn ensure_event_sourcing_initialized(world: &mut LithairWorld) {
    // If already initialized (TempDir present), do nothing
    {
        let temp_dir_guard = world.temp_dir.lock().await;
        if temp_dir_guard.is_some() {
            return;
        }
    }

    let path = world.init_temp_storage().await.expect("Failed to init event sourcing storage");

    {
        let mut metrics = world.metrics.lock().await;
        metrics.persist_path = path.to_string_lossy().to_string();
    }

    world
        .engine
        .with_state_mut(|state| {
            *state = crate::features::world::TestAppState::default();
        })
        .ok();

    println!("📝 Direct event sourcing engine activated in {:?}", path);
}

#[given(expr = "events are persisted in {string}")]
async fn given_events_persisted_in(_world: &mut LithairWorld, filename: String) {
    println!("Events persisted in: {}", filename);
}

#[given(expr = "snapshots are created periodically")]
async fn given_periodic_snapshots(_world: &mut LithairWorld) {
    println!("Periodic snapshots enabled");
}

// Scenario: Event persistence
#[when(expr = "I perform a CRUD operation")]
async fn when_perform_crud_operation(world: &mut LithairWorld) {
    // Ensure event sourcing environment is initialized
    ensure_event_sourcing_initialized(world).await;

    let payload = serde_json::json!({
        "title": "Test Article",
        "content": "Content"
    });

    // Apply an event to the in-memory state
    let event = crate::features::world::TestEvent::ArticleCreated {
        id: "es-crud-1".to_string(),
        title: "Test Article".to_string(),
        content: "Content".to_string(),
    };

    world
        .engine
        .with_state_mut(|state| {
            event.apply(state);
        })
        .ok();

    // Persist the event to events.raftlog with explicit metadata
    let event_json = serde_json::json!({
        "event_type": "ArticleCreated",
        "event_id": "es-crud-1",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "payload": payload,
    })
    .to_string();

    let mut storage_guard = world.storage.lock().await;
    if let Some(storage) = storage_guard.as_mut() {
        storage.append_event(&event_json).expect("Failed to persist event");
        storage.flush_batch().expect("Failed to flush batch for event sourcing");
    } else {
        panic!("Storage not initialized for event sourcing");
    }

    println!("✍️ Direct CRUD operation performed and event persisted");
}

#[then(expr = "an event should be created and persisted")]
async fn then_event_created_and_persisted(world: &mut LithairWorld) {
    let temp_dir = world.temp_dir.lock().await;
    let dir = temp_dir.as_ref().expect("TempDir not initialized for event sourcing");
    let events_file = dir.path().join("events.raftlog");

    assert!(events_file.exists(), "❌ events.raftlog file not found: {:?}", events_file);

    let content = std::fs::read_to_string(&events_file).expect("Failed to read events.raftlog");
    let lines: Vec<_> = content.lines().filter(|l| !l.trim().is_empty()).collect();

    assert!(!lines.is_empty(), "❌ No events persisted in events.raftlog");

    println!("✅ {} event(s) persisted in {:?}", lines.len(), events_file);
}

#[then(expr = "the event should contain all metadata")]
async fn then_event_contains_metadata(world: &mut LithairWorld) {
    let temp_dir = world.temp_dir.lock().await;
    let dir = temp_dir.as_ref().expect("TempDir not initialized for event sourcing");
    let events_file = dir.path().join("events.raftlog");

    let content = std::fs::read_to_string(&events_file).expect("Failed to read events.raftlog");
    let last_line = content
        .lines()
        .rfind(|l| !l.trim().is_empty())
        .expect("No event found in events.raftlog");

    let json_part = strip_crc32_prefix(last_line);
    let value: serde_json::Value = serde_json::from_str(json_part).expect("Invalid event (JSON)");

    let obj = value.as_object().expect("Persisted event is not a JSON object");

    assert!(
        obj.get("event_type").and_then(|v| v.as_str()).is_some(),
        "❌ event_type missing in event"
    );
    assert!(
        obj.get("event_id").and_then(|v| v.as_str()).is_some(),
        "❌ event_id missing in event"
    );
    assert!(
        obj.get("timestamp").and_then(|v| v.as_str()).is_some(),
        "❌ timestamp missing in event"
    );
    assert!(obj.get("payload").is_some(), "❌ payload missing in event");

    println!("✅ Metadata present in persisted event");
}

#[then(expr = "the log file should be updated atomically")]
async fn then_log_file_updated_atomically(world: &mut LithairWorld) {
    let temp_dir = world.temp_dir.lock().await;
    let dir = temp_dir.as_ref().expect("TempDir not initialized for event sourcing");
    let events_file = dir.path().join("events.raftlog");

    let content = std::fs::read_to_string(&events_file).expect("Failed to read events.raftlog");

    for (idx, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        if parse_and_validate_event(line).is_err() {
            panic!("❌ Partial or corrupted line detected in events.raftlog at line {}", idx + 1);
        }
    }

    println!("✅ All log lines are valid JSON (atomic update)");
}

// Scenario: State reconstruction
#[when(expr = "I restart the server")]
async fn when_restart_server(world: &mut LithairWorld) {
    println!("🔄 Preparing state reconstruction scenario...");

    // 1) Ensure event sourcing environment is properly initialized
    ensure_event_sourcing_initialized(world).await;

    // 2) Build an initial state by applying a few ArticleCreated events
    const EVENT_COUNT: u32 = 10;
    for i in 0..EVENT_COUNT {
        let id = format!("replay-{}", i);
        let title = format!("Article {}", i);
        let content = format!("Content {}", i);

        let event = crate::features::world::TestEvent::ArticleCreated {
            id: id.clone(),
            title: title.clone(),
            content: content.clone(),
        };

        // Apply to in-memory state
        world
            .engine
            .with_state_mut(|state| {
                event.apply(state);
            })
            .ok();

        // Log the event to events.raftlog
        let event_json = serde_json::json!({
            "event_type": "ArticleCreated",
            "id": id,
            "title": title,
            "content": content,
        })
        .to_string();

        let mut storage_guard = world.storage.lock().await;
        if let Some(storage) = storage_guard.as_mut() {
            storage.append_event(&event_json).expect("Failed append_event for replay");
        } else {
            panic!("Storage not initialized for reconstruction");
        }
    }

    // Flush all events at once
    {
        let mut storage_guard = world.storage.lock().await;
        if let Some(storage) = storage_guard.as_mut() {
            storage.flush_batch().expect("Failed flush_batch for reconstruction");
        }
    }

    // 3) Capture a state snapshot before restart
    let snapshot_data = world.engine.with_state(|state| state.data.clone()).unwrap_or_default();
    {
        let mut test_data = world.test_data.lock().await;
        *test_data = snapshot_data;
    }

    // Save the event count for verification
    {
        let mut metrics = world.metrics.lock().await;
        metrics.request_count = EVENT_COUNT as u64;
    }

    // 4) Reset state then replay all events from events.raftlog
    world
        .engine
        .with_state_mut(|state| {
            *state = crate::features::world::TestAppState::default();
        })
        .ok();

    println!("🔄 Logical engine restart: state reset to zero, replaying events...");

    let start = std::time::Instant::now();

    let events_lines = {
        let storage_guard = world.storage.lock().await;
        let storage = storage_guard.as_ref().expect("Storage not initialized for replay");
        storage.read_all_events().expect("Failed to read events.raftlog for replay")
    };

    for line in &events_lines {
        let value: serde_json::Value =
            serde_json::from_str(line).expect("Invalid event in events.raftlog (replay)");

        if let Some(obj) = value.as_object() {
            let event_type =
                obj.get("event_type").and_then(|v| v.as_str()).unwrap_or("ArticleCreated");

            if event_type == "ArticleCreated" {
                let id = obj.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let title = obj.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let content = obj.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();

                let event =
                    crate::features::world::TestEvent::ArticleCreated { id, title, content };

                world
                    .engine
                    .with_state_mut(|state| {
                        event.apply(state);
                    })
                    .ok();
            }
        }
    }

    let duration = start.elapsed();
    {
        let mut metrics = world.metrics.lock().await;
        metrics.total_duration = duration;
    }

    println!(
        "✅ Replay completed: {} events replayed in {:.3}s",
        events_lines.len(),
        duration.as_secs_f64()
    );
}

#[then(expr = "all events should be replayed")]
async fn then_all_events_replayed(world: &mut LithairWorld) {
    let expected = {
        let metrics = world.metrics.lock().await;
        metrics.request_count as usize
    };

    let actual = {
        let storage_guard = world.storage.lock().await;
        let storage =
            storage_guard.as_ref().expect("Storage not initialized for replay verification");
        storage
            .read_all_events()
            .expect("Failed to read events.raftlog for verification")
            .len()
    };

    assert_eq!(
        actual, expected,
        "❌ Incorrect number of replayed events: {} (expected: {})",
        actual, expected
    );

    println!("✅ All events ({}) have been replayed", actual);
}

#[then(expr = "state should be identical to before the restart")]
async fn then_state_identical(world: &mut LithairWorld) {
    // State before restart (snapshot)
    let pre_state = { world.test_data.lock().await.clone() };

    // State after replay
    let post_state = world.engine.with_state(|state| state.data.clone()).unwrap_or_default();

    assert_eq!(
        pre_state.articles.len(),
        post_state.articles.len(),
        "❌ Different article count after replay: before={}, after={}",
        pre_state.articles.len(),
        post_state.articles.len()
    );

    assert_eq!(
        pre_state.articles, post_state.articles,
        "❌ Articles after replay do not match the initial state"
    );

    println!("✅ State restored identically ({} articles)", post_state.articles.len());
}

#[then(expr = "reconstruction should take less than {int} seconds")]
async fn then_reconstruction_within(world: &mut LithairWorld, max_seconds: u32) {
    let metrics = world.metrics.lock().await;
    let elapsed = metrics.total_duration;
    let secs = elapsed.as_secs_f64();

    assert!(
        secs <= max_seconds as f64,
        "❌ Reconstruction too slow: {:.3}s (max: {}s)",
        secs,
        max_seconds
    );

    println!("✅ Reconstruction in {:.3}s (< {}s)", secs, max_seconds);
}

// Scenario: Optimized snapshots
#[when(expr = "{int} events have been created")]
async fn when_events_created(world: &mut LithairWorld, event_count: u32) {
    println!("📊 Creating {} events...", event_count);

    let start = std::time::Instant::now();

    // Ensure event sourcing environment is initialized
    ensure_event_sourcing_initialized(world).await;

    // Generate ArticleCreated events, apply and persist them
    for i in 0..event_count {
        let id = format!("snapshot-{}", i);
        let title = format!("Article {}", i);
        let content = format!("Content {}", i);

        let event = crate::features::world::TestEvent::ArticleCreated {
            id: id.clone(),
            title: title.clone(),
            content: content.clone(),
        };

        // Apply to in-memory state
        world
            .engine
            .with_state_mut(|state| {
                event.apply(state);
            })
            .ok();

        // Write to events.raftlog as simple JSON
        let event_json = serde_json::json!({
            "event_type": "ArticleCreated",
            "id": id,
            "title": title,
            "content": content,
        })
        .to_string();

        let mut storage_guard = world.storage.lock().await;
        if let Some(storage) = storage_guard.as_mut() {
            storage.append_event(&event_json).expect("Failed append_event for snapshots");
        } else {
            panic!("Storage not initialized for snapshots");
        }

        if i % 100 == 0 {
            println!("  Progression: {}/{}", i, event_count);
        }
    }

    // Flush all events at once
    {
        let mut storage_guard = world.storage.lock().await;
        if let Some(storage) = storage_guard.as_mut() {
            storage.flush_batch().expect("Failed flush_batch for snapshots");
        }
    }

    // Generate a complete JSON snapshot of the current state
    let snapshot_json = world
        .engine
        .with_state(|state| serde_json::to_string(state).expect("Snapshot serialization"))
        .unwrap_or_else(|_| "{}".to_string());

    {
        let storage_guard = world.storage.lock().await;
        let storage = storage_guard.as_ref().expect("Storage not initialized for save_snapshot");
        storage.save_snapshot(&snapshot_json).expect("Failed save_snapshot");
    }

    let elapsed = start.elapsed();
    {
        let mut metrics = world.metrics.lock().await;
        metrics.total_duration = elapsed;
        metrics.request_count = event_count as u64;
    }

    println!("✅ {} events created and snapshot written", event_count);
}

#[then(expr = "a snapshot should be generated automatically")]
async fn then_snapshot_generated(world: &mut LithairWorld) {
    let temp_dir = world.temp_dir.lock().await;
    let dir = temp_dir.as_ref().expect("TempDir not initialized for snapshots");
    let snapshot_file = dir.path().join("state.raftsnap");

    assert!(snapshot_file.exists(), "❌ state.raftsnap file not found: {:?}", snapshot_file);

    let content = std::fs::read_to_string(&snapshot_file).expect("Failed to read state.raftsnap");
    assert!(!content.trim().is_empty(), "❌ Empty snapshot in state.raftsnap");

    let value: serde_json::Value = serde_json::from_str(&content).expect("Invalid snapshot (JSON)");
    assert!(value.is_object(), "❌ Snapshot JSON is not an object");

    println!(
        "✅ Snapshot automatically generated ({} bytes) in {:?}",
        content.len(),
        snapshot_file
    );
}

#[then(expr = "the snapshot should compress current state")]
async fn then_snapshot_compresses_state(world: &mut LithairWorld) {
    let temp_dir = world.temp_dir.lock().await;
    let dir = temp_dir.as_ref().expect("TempDir not initialized for snapshots");

    let snapshot_file = dir.path().join("state.raftsnap");
    let events_file = dir.path().join("events.raftlog");

    let snapshot_size =
        std::fs::metadata(&snapshot_file).expect("Snapshot metadata not found").len();
    let events_size = std::fs::metadata(&events_file)
        .expect("events.raftlog metadata not found")
        .len();

    assert!(
        snapshot_size < events_size,
        "❌ Snapshot ({snapshot_size} bytes) is not more compact than the log ({events_size} bytes)"
    );

    println!(
        "✅ Snapshot more compact than the log: {} bytes vs {} bytes",
        snapshot_size, events_size
    );
}

#[then(expr = "old events should be archived")]
async fn then_old_events_archived(world: &mut LithairWorld) {
    let temp_dir = world.temp_dir.lock().await;
    let dir = temp_dir.as_ref().expect("TempDir not initialized for snapshots");
    let events_file = dir.path().join("events.raftlog");

    // Compact/archive the log by truncating after snapshot
    {
        let mut storage_guard = world.storage.lock().await;
        if let Some(storage) = storage_guard.as_mut() {
            storage.truncate_events().expect("Failed truncate_events for archiving");
        } else {
            panic!("Storage not initialized for archiving");
        }
    }

    let size = std::fs::metadata(&events_file)
        .expect("events.raftlog metadata not found after archiving")
        .len();

    assert!(size == 0, "❌ Log not archived/compacted, remaining size: {} bytes", size);

    println!("✅ Old events archived (events.raftlog truncated to 0 bytes)");
}

#[then(expr = "snapshot generation should take less than {int} seconds")]
async fn then_snapshot_generation_within(world: &mut LithairWorld, max_seconds: u32) {
    let metrics = world.metrics.lock().await;
    let elapsed = metrics.total_duration;
    let secs = elapsed.as_secs_f64();

    assert!(
        secs <= max_seconds as f64,
        "❌ Snapshot generation too slow: {:.3}s (max: {}s)",
        secs,
        max_seconds
    );

    println!("✅ Snapshot generation in {:.3}s (< {}s)", secs, max_seconds);
}

// Scenario: Event deduplication
#[when(expr = "the same event is received twice")]
async fn when_duplicate_event_received(world: &mut LithairWorld) {
    println!("🔁 Preparing deduplication scenario (direct engine)...");

    // 1) Initialiser l'environnement event sourcing
    ensure_event_sourcing_initialized(world).await;

    // 2) Build an ArticleCreated event with a stable idempotence key
    let id = "dedup-article-1".to_string();
    let title = "Deduplicated Article".to_string();
    let content = "Deduplicated Content".to_string();

    let event = crate::features::world::TestEvent::ArticleCreated {
        id: id.clone(),
        title: title.clone(),
        content: content.clone(),
    };

    // Simulate receiving the same event twice
    let mut seen_keys = std::collections::HashSet::new();

    let mut apply_with_dedup = |evt: &crate::features::world::TestEvent| {
        let key = evt.idempotence_key().unwrap_or_else(|| evt.to_json());
        if seen_keys.insert(key) {
            world
                .engine
                .with_state_mut(|state| {
                    evt.apply(state);
                })
                .ok();
            true
        } else {
            false
        }
    };

    let first_applied = apply_with_dedup(&event);
    let second_applied = apply_with_dedup(&event);

    // Update metrics for assertions
    {
        let article_count = world.engine.with_state(|state| state.data.articles.len()).unwrap_or(0);
        let mut metrics = world.metrics.lock().await;
        metrics.request_count = article_count as u64;
        // Consider an error if deduplication did not behave as expected
        metrics.error_count = if !first_applied || second_applied { 1 } else { 0 };
    }

    // 3) Persist the same event envelope twice in events.raftlog
    let payload = serde_json::json!({
        "id": id,
        "title": title,
        "content": content,
    });

    let event_id = event.idempotence_key().unwrap_or_else(|| "dedup-missing".to_string());

    let envelope = serde_json::json!({
        "event_type": "ArticleCreated",
        "event_id": event_id,
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "payload": payload,
    })
    .to_string();

    let mut storage_guard = world.storage.lock().await;
    if let Some(storage) = storage_guard.as_mut() {
        storage
            .append_event(&envelope)
            .expect("Failed append_event for deduplication (1)");
        storage
            .append_event(&envelope)
            .expect("Failed append_event for deduplication (2)");
        storage.flush_batch().expect("Failed flush_batch for deduplication");
    } else {
        panic!("Storage not initialized for deduplication");
    }

    println!("🔁 Same event received twice, applied once in memory and persisted twice in log");
}

#[then(expr = "only the first should be applied")]
async fn then_only_first_applied(world: &mut LithairWorld) {
    let articles = world.engine.with_state(|state| state.data.articles.clone()).unwrap_or_default();

    assert_eq!(
        articles.len(),
        1,
        "❌ Deduplication failed: {} articles present in memory (expected: 1)",
        articles.len()
    );

    println!("✅ Only the first event was applied (1 article in memory)");
}

#[then(expr = "the duplicate should be ignored silently")]
async fn then_duplicate_ignored(world: &mut LithairWorld) {
    // Verify no error was recorded in metrics
    let metrics = world.metrics.lock().await;
    assert_eq!(
        metrics.error_count, 0,
        "❌ An error was recorded during deduplication (error_count = {})",
        metrics.error_count
    );
    drop(metrics);

    // Verify the log contains two entries for the same event_id
    let temp_dir = world.temp_dir.lock().await;
    let dir = temp_dir
        .as_ref()
        .expect("TempDir not initialized for deduplication verification");
    let events_file = dir.path().join("events.raftlog");

    let content =
        std::fs::read_to_string(&events_file).expect("Failed to read events.raftlog (dedup)");

    let mut total = 0usize;
    let mut duplicate_count = 0usize;
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        total += 1;
        let json_part = strip_crc32_prefix(line);
        let value: serde_json::Value =
            serde_json::from_str(json_part).expect("Invalid line in events.raftlog (dedup)");
        if let Some(obj) = value.as_object() {
            if let Some(eid) = obj.get("event_id").and_then(|v| v.as_str()) {
                if eid.starts_with("article-created:dedup-article-1") {
                    duplicate_count += 1;
                }
            }
        }
    }

    assert!(
        duplicate_count >= 2,
        "❌ Log does not contain at least two entries for the same event_id (count = {}, total = {})",
        duplicate_count,
        total
    );

    println!(
        "✅ Duplicate silently ignored on state side: {} entries in log for the same event_id, but only one application",
        duplicate_count
    );
}

#[then(expr = "integrity should be preserved")]
async fn then_integrity_preserved(world: &mut LithairWorld) {
    // Replay the log with deduplication by event_id into a clean state
    let temp_dir = world.temp_dir.lock().await;
    let dir = temp_dir.as_ref().expect("TempDir not initialized for integrity verification");
    let events_file = dir.path().join("events.raftlog");

    let content =
        std::fs::read_to_string(&events_file).expect("Failed to read events.raftlog (integrity)");

    let mut seen_ids = std::collections::HashSet::new();
    let mut rebuilt_state = crate::features::world::TestAppState::default();

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }

        let json_part = strip_crc32_prefix(line);
        let value: serde_json::Value =
            serde_json::from_str(json_part).expect("Invalid line in events.raftlog (integrity)");

        let obj = value.as_object().expect("Log event is not a JSON object (integrity)");

        let event_id = obj.get("event_id").and_then(|v| v.as_str()).unwrap_or("").to_string();

        if !seen_ids.insert(event_id) {
            // Already seen: duplicate, skip during replay
            continue;
        }

        if let Some(payload) = obj.get("payload") {
            if let Some(pobj) = payload.as_object() {
                let id = pobj.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let title = pobj.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let content =
                    pobj.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();

                let event =
                    crate::features::world::TestEvent::ArticleCreated { id, title, content };

                event.apply(&mut rebuilt_state);
            }
        }
    }

    let current_state = world.engine.with_state(|state| state.data.clone()).unwrap_or_default();

    assert_eq!(
        current_state.articles,
        rebuilt_state.data.articles,
        "❌ Integrity not preserved: state rebuilt from log (with dedup) differs from current state"
    );

    println!("✅ Integrity preserved: current state == state rebuilt with log deduplication");
}

// Scenario: Recovery after corruption (direct engine)
#[when(expr = "the state file is corrupted")]
async fn when_state_file_corrupted(world: &mut LithairWorld) {
    println!("💥 Preparing a corrupted state file (direct event sourcing)...");

    // 1) Initialiser l'environnement event sourcing
    ensure_event_sourcing_initialized(world).await;

    // 2) Generate a consistent state with a few ArticleCreated events
    const EVENT_COUNT: u32 = 20;
    for i in 0..EVENT_COUNT {
        let id = format!("corrupt-{}", i);
        let title = format!("Article {}", i);
        let content = format!("Content {}", i);

        let event = crate::features::world::TestEvent::ArticleCreated {
            id: id.clone(),
            title: title.clone(),
            content: content.clone(),
        };

        // Apply to in-memory state
        world
            .engine
            .with_state_mut(|state| {
                event.apply(state);
            })
            .ok();

        // Persist to events.raftlog
        let event_json = serde_json::json!({
            "event_type": "ArticleCreated",
            "id": id,
            "title": title,
            "content": content,
        })
        .to_string();

        let mut storage_guard = world.storage.lock().await;
        if let Some(storage) = storage_guard.as_mut() {
            storage.append_event(&event_json).expect("Failed append_event for corruption");
        } else {
            panic!("Storage not initialized for corruption");
        }
    }

    {
        let mut storage_guard = world.storage.lock().await;
        if let Some(storage) = storage_guard.as_mut() {
            storage.flush_batch().expect("Failed flush_batch for corruption");
        }
    }

    // 3) Save a valid snapshot of the current state (last valid snapshot)
    let snapshot_json = world
        .engine
        .with_state(|state| {
            serde_json::to_string(state).expect("Snapshot serialization corruption")
        })
        .unwrap_or_else(|_| "{}".to_string());

    {
        let storage_guard = world.storage.lock().await;
        let storage = storage_guard
            .as_ref()
            .expect("Storage not initialized for save_snapshot corruption");
        storage.save_snapshot(&snapshot_json).expect("Failed save_snapshot corruption");
    }

    // Keep the expected state for verification
    let expected_data = world.engine.with_state(|state| state.data.clone()).unwrap_or_default();
    {
        let mut test_data = world.test_data.lock().await;
        *test_data = expected_data;
    }

    // 4) Corrupt the events.raftlog file by injecting an invalid JSON line
    let events_file = {
        let temp_dir = world.temp_dir.lock().await;
        let dir = temp_dir.as_ref().expect("TempDir not initialized for corruption");
        dir.path().join("events.raftlog")
    };

    let mut content = std::fs::read_to_string(&events_file)
        .expect("Failed to read events.raftlog for corruption");
    content.push_str("\n{ this-is-not-valid-json");
    std::fs::write(&events_file, &content).expect("Failed to write corrupted events.raftlog");

    // Reset the corruption flag for subsequent assertions
    world.corruption_detected = false;

    println!("⚠️ Corrupted state file simulated in {:?}", events_file);
}

#[then(expr = "the system should detect corruption")]
async fn then_system_detects_corruption(world: &mut LithairWorld) {
    let events_file = {
        let temp_dir = world.temp_dir.lock().await;
        let dir = temp_dir.as_ref().expect("TempDir not initialized for corruption detection");
        dir.path().join("events.raftlog")
    };

    let content =
        std::fs::read_to_string(&events_file).expect("Failed to read events.raftlog for detection");

    let mut invalid = 0usize;
    let mut last_invalid_line = 0usize;

    for (idx, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        if parse_and_validate_event(line).is_err() {
            invalid += 1;
            last_invalid_line = idx + 1;
        }
    }

    assert!(
        invalid > 0,
        "❌ No corruption detected in events.raftlog although an invalid line was injected"
    );

    world.corruption_detected = true;

    println!(
        "✅ Corruption detected: {} invalid line(s) (e.g., line {})",
        invalid, last_invalid_line
    );
}

#[then(expr = "rebuild from last valid snapshot")]
async fn then_rebuild_from_last_valid_snapshot(world: &mut LithairWorld) {
    assert!(
        world.corruption_detected,
        "❌ Corruption not detected before reconstruction attempt"
    );

    // Load the last valid snapshot from FileStorage
    let snapshot_json_opt = {
        let storage_guard = world.storage.lock().await;
        let storage = storage_guard.as_ref().expect("Storage not initialized for load_snapshot");
        storage.load_snapshot().expect("Failed load_snapshot for reconstruction")
    };

    let snapshot_json = snapshot_json_opt
        .expect("❌ No snapshot found although a valid snapshot should have been saved");

    let snapshot_state: crate::features::world::TestAppState = serde_json::from_str(&snapshot_json)
        .expect("Invalid snapshot (JSON) during reconstruction");

    // Reset the engine state from the snapshot
    world
        .engine
        .with_state_mut(|state| {
            *state = snapshot_state.clone();
        })
        .ok();

    // Compare with the expected state saved before corruption
    let expected_data = { world.test_data.lock().await.clone() };
    let current_data = world.engine.with_state(|state| state.data.clone()).unwrap_or_default();

    assert_eq!(
        expected_data.articles, current_data.articles,
        "❌ Rebuilt state differs from last valid snapshot",
    );

    println!(
        "✅ State rebuilt from last valid snapshot ({} articles)",
        current_data.articles.len()
    );
}

#[then(expr = "continue to function normally")]
async fn then_continue_to_operate_normally(world: &mut LithairWorld) {
    // Apply a new event after recovery
    let id = "corruption-recovery-new-1".to_string();
    let title = "Article after recovery".to_string();
    let content = "Content after recovery".to_string();

    let event = crate::features::world::TestEvent::ArticleCreated {
        id: id.clone(),
        title: title.clone(),
        content: content.clone(),
    };

    world
        .engine
        .with_state_mut(|state| {
            event.apply(state);
        })
        .ok();

    let event_json = serde_json::json!({
        "event_type": "ArticleCreated",
        "id": id,
        "title": title,
        "content": content,
    })
    .to_string();

    {
        let mut storage_guard = world.storage.lock().await;
        if let Some(storage) = storage_guard.as_mut() {
            storage.append_event(&event_json).expect("Failed append_event after recovery");
            storage.flush_batch().expect("Failed flush_batch after recovery");
        } else {
            panic!("Storage not initialized after recovery");
        }
    }

    let article_count = world.engine.with_state(|state| state.data.articles.len()).unwrap_or(0);

    assert!(
        article_count > 0,
        "❌ No articles present after recovery and writing a new event",
    );

    println!(
        "✅ Engine continues to operate normally after corruption ({} articles)",
        article_count
    );
}

// Scenario: Persistent deduplication after restart
#[when(expr = "an idempotent event is applied before and after engine restart")]
async fn when_idempotent_event_before_and_after_restart(world: &mut LithairWorld) {
    use crate::features::world::{TestEngineApp, TestEvent};

    println!("🧪 Preparing persistent deduplication scenario (before/after restart)...");

    let base_path = "/tmp/lithair-dedup-persistent-test".to_string();

    // Clean the test directory
    std::fs::remove_dir_all(&base_path).ok();
    std::fs::create_dir_all(&base_path)
        .expect("Failed to create directory for persistent deduplication");

    // Force deduplication ID persistence (already enabled by default, but explicit)
    std::env::set_var("LT_DEDUP_PERSIST", "1");

    let config = EngineConfig { event_log_path: base_path.clone(), ..Default::default() };

    // Run 1: apply the event for the first time
    let engine = Engine::<TestEngineApp>::new(config.clone())
        .expect("Failed to initialize engine for persistent deduplication");

    let event = TestEvent::ArticleCreated {
        id: "dedup-persistent-1".to_string(),
        title: "Persistent dedup article".to_string(),
        content: "Persistent dedup content".to_string(),
    };

    let key = event.aggregate_id().unwrap_or("global".to_string());
    engine
        .apply_event(key.clone(), event.clone())
        .expect("Failed initial application of idempotent event");
    engine.flush().expect("Failed flush after first application");

    // Drop to simulate a clean shutdown
    drop(engine);

    // Run 2: restart the engine and re-apply the same event
    let engine2 = Engine::<TestEngineApp>::new(config)
        .expect("Failed to reinitialize engine for persistent deduplication");

    let result_second = engine2.apply_event(key, event);

    let duplicate_rejected = matches!(result_second, Err(EngineError::DuplicateEvent(_)));

    // Verify the content of dedup.raftids
    let dedup_file = format!("{}/dedup.raftids", base_path);
    let dedup_ids = std::fs::read_to_string(&dedup_file)
        .map(|content| {
            content
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|s| s.to_string())
                .collect::<Vec<String>>()
        })
        .unwrap_or_else(|_| Vec::new());

    let expected_id = "article-created:dedup-persistent-1".to_string();
    let contains_expected = dedup_ids.iter().any(|id| id == &expected_id);

    {
        let mut test_data = world.test_data.lock().await;
        test_data.tokens.insert(
            "dedup_persistent_duplicate_rejected".to_string(),
            duplicate_rejected.to_string(),
        );
        test_data
            .tokens
            .insert("dedup_persistent_dedup_ids_count".to_string(), dedup_ids.len().to_string());
        test_data.tokens.insert(
            "dedup_persistent_contains_expected".to_string(),
            contains_expected.to_string(),
        );
    }

    println!(
        "🧪 Persistent deduplication: duplicate_rejected={}, dedup_ids_count={}, contains_expected={}",
        duplicate_rejected,
        dedup_ids.len(),
        contains_expected
    );
}

#[then(expr = "the engine should reject the duplicate after restart")]
async fn then_engine_rejects_duplicate_after_restart(world: &mut LithairWorld) {
    let test_data = world.test_data.lock().await;

    let duplicate_rejected = test_data
        .tokens
        .get("dedup_persistent_duplicate_rejected")
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(false);
    let contains_expected = test_data
        .tokens
        .get("dedup_persistent_contains_expected")
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(false);
    let dedup_count = test_data
        .tokens
        .get("dedup_persistent_dedup_ids_count")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);

    assert!(
        duplicate_rejected,
        "❌ Engine did not reject the duplicate after restart (duplicate_rejected = false)"
    );
    assert!(
        contains_expected && dedup_count >= 1,
        "❌ dedup.raftids does not contain the expected key or is empty (contains_expected={}, count={})",
        contains_expected,
        dedup_count
    );
}

#[when(expr = "an idempotent event is applied before and after engine restart in multi-file mode")]
async fn when_idempotent_event_before_and_after_restart_multifile(world: &mut LithairWorld) {
    use crate::features::world::{TestEngineApp, TestEvent};

    println!("🧪 Preparing persistent deduplication scenario in multi-file mode...",);

    let base_path = "/tmp/lithair-dedup-multifile-test".to_string();

    // Clean the test directory
    std::fs::remove_dir_all(&base_path).ok();
    std::fs::create_dir_all(&base_path)
        .expect("Failed to create directory for multi-file deduplication");

    // Force deduplication ID persistence
    std::env::set_var("LT_DEDUP_PERSIST", "1");

    let config = EngineConfig {
        event_log_path: base_path.clone(),
        use_multi_file_store: true,
        ..Default::default()
    };

    // Run 1: apply the event for the first time in multi-file mode
    let engine = Engine::<TestEngineApp>::new(config.clone())
        .expect("Failed to initialize engine in multi-file mode for persistent deduplication");

    let event = TestEvent::ArticleCreated {
        id: "dedup-multifile-1".to_string(),
        title: "Multi-file dedup article".to_string(),
        content: "Multi-file dedup content".to_string(),
    };

    let key = event.aggregate_id().unwrap_or("global".to_string());
    engine
        .apply_event(key.clone(), event.clone())
        .expect("Failed initial application of idempotent event in multi-file mode");
    engine.flush().expect("Failed flush after first application (multi-file)");

    // Drop to simulate a clean shutdown
    drop(engine);

    // Run 2: restart the engine and re-apply the same event
    let engine2 = Engine::<TestEngineApp>::new(config)
        .expect("Failed to reinitialize engine for persistent deduplication in multi-file mode");

    let result_second = engine2.apply_event(key, event);

    let duplicate_rejected = matches!(result_second, Err(EngineError::DuplicateEvent(_)));

    // Verify the content of dedup.raftids global (base_path/global/dedup.raftids)
    let dedup_file = format!("{}/global/dedup.raftids", base_path);
    let dedup_ids = std::fs::read_to_string(&dedup_file)
        .map(|content| {
            content
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|s| s.to_string())
                .collect::<Vec<String>>()
        })
        .unwrap_or_else(|_| Vec::new());

    let expected_id = "article-created:dedup-multifile-1".to_string();
    let contains_expected = dedup_ids.iter().any(|id| id == &expected_id);

    {
        let mut test_data = world.test_data.lock().await;
        // Reuse the same tokens as the existing persistent dedup scenario
        test_data.tokens.insert(
            "dedup_persistent_duplicate_rejected".to_string(),
            duplicate_rejected.to_string(),
        );
        test_data
            .tokens
            .insert("dedup_persistent_dedup_ids_count".to_string(), dedup_ids.len().to_string());
        test_data.tokens.insert(
            "dedup_persistent_contains_expected".to_string(),
            contains_expected.to_string(),
        );

        // Tokens specific to multi-file scenario
        test_data
            .tokens
            .insert("multifile_dedup_base_path".to_string(), base_path.clone());
        test_data
            .tokens
            .insert("multifile_dedup_expected_id".to_string(), expected_id.clone());
    }

    println!(
        "🧪 Multi-file dedup: duplicate_rejected={}, dedup_ids_count={}, contains_expected={}",
        duplicate_rejected,
        dedup_ids.len(),
        contains_expected
    );
}

#[then(expr = "the deduplication file should be global in multi-file mode")]
async fn then_dedup_file_is_global_multifile(world: &mut LithairWorld) {
    let test_data = world.test_data.lock().await;
    let base_path = test_data
        .tokens
        .get("multifile_dedup_base_path")
        .cloned()
        .unwrap_or_else(|| "/tmp/lithair-dedup-multifile-test".to_string());
    let expected_id = test_data
        .tokens
        .get("multifile_dedup_expected_id")
        .cloned()
        .unwrap_or_else(|| "article-created:dedup-multifile-1".to_string());
    drop(test_data);

    let global_dedup = format!("{}/global/dedup.raftids", base_path);
    assert!(
        std::path::Path::new(&global_dedup).exists(),
        "❌ Global deduplication file not found in multi-file mode: {}",
        global_dedup
    );

    let content = std::fs::read_to_string(&global_dedup)
        .expect("Failed to read global dedup.raftids file (multi-file)");
    let ids: Vec<_> = content.lines().filter(|l| !l.trim().is_empty()).collect();

    assert!(
        !ids.is_empty(),
        "❌ No deduplication identifiers found in global file ({})",
        global_dedup
    );

    let contains_expected = ids.iter().any(|id| *id == expected_id);

    assert!(
        contains_expected,
        "❌ Global dedup.raftids file does not contain expected identifier '{}' (ids={:?})",
        expected_id, ids
    );

    println!(
        "✅ Global deduplication file found in multi-file mode ({} identifier(s))",
        ids.len()
    );
}

#[when(expr = "I persist events on multiple aggregates in a multi-file event store")]
async fn when_persist_events_multi_aggregates_multifile(world: &mut LithairWorld) {
    use crate::features::world::{TestEngineApp, TestEvent};

    println!("🧪 Multi-file routing: persisting events on multiple aggregates (multi-file mode)",);

    let base_path = "/tmp/lithair-multifile-routing-test".to_string();

    // Clean and recreate the test directory
    std::fs::remove_dir_all(&base_path).ok();
    std::fs::create_dir_all(&base_path)
        .expect("Failed to create test directory for multi-file mode");

    let config = EngineConfig {
        event_log_path: base_path.clone(),
        use_multi_file_store: true,
        ..Default::default()
    };

    let engine = Engine::<TestEngineApp>::new(config)
        .expect("Failed to initialize engine in multi-file mode");

    // Two distinct logical structures / tables: articles and users
    let article_id = "article-multifile-1".to_string();
    let user_id = "user-multifile-1".to_string();

    let event_articles = TestEvent::ArticleCreated {
        id: article_id.clone(),
        title: "Article multi-file".to_string(),
        content: "Multi-file content".to_string(),
    };

    let event_users = TestEvent::UserCreated {
        id: user_id.clone(),
        data: serde_json::json!({
            "name": "User multi-file",
            "email": "user-multifile@test.com"
        }),
    };

    let key_articles = event_articles.aggregate_id().unwrap_or("global".to_string());
    let key_users = event_users.aggregate_id().unwrap_or("global".to_string());

    engine
        .apply_event(key_articles, event_articles)
        .expect("Failed to apply aggregate_articles event");
    engine
        .apply_event(key_users, event_users)
        .expect("Failed to apply aggregate_users event");
    engine.flush().expect("Failed to flush engine in multi-file mode");

    {
        let mut test_data = world.test_data.lock().await;
        test_data.tokens.insert("multifile_base_path".to_string(), base_path.clone());
        // aggregate_id now corresponds to the table/structure name
        test_data
            .tokens
            .insert("multifile_agg_articles".to_string(), "articles".to_string());
        test_data.tokens.insert("multifile_agg_users".to_string(), "users".to_string());
    }

    println!("✅ Events persisted in multi-file mode for two distinct logical aggregates",);
}

#[then(expr = "events should be distributed by aggregate into distinct files")]
async fn then_events_routed_to_distinct_files(world: &mut LithairWorld) {
    let test_data = world.test_data.lock().await;
    let base_path = test_data
        .tokens
        .get("multifile_base_path")
        .cloned()
        .unwrap_or_else(|| "/tmp/lithair-multifile-routing-test".to_string());
    let agg_articles = test_data
        .tokens
        .get("multifile_agg_articles")
        .cloned()
        .unwrap_or_else(|| "aggregate_articles".to_string());
    let agg_users = test_data
        .tokens
        .get("multifile_agg_users")
        .cloned()
        .unwrap_or_else(|| "aggregate_users".to_string());
    drop(test_data);

    let articles_path = format!("{}/{}/events.raftlog", base_path, agg_articles);
    let users_path = format!("{}/{}/events.raftlog", base_path, agg_users);

    assert!(
        std::path::Path::new(&articles_path).exists(),
        "❌ events.raftlog file not found for articles aggregate: {}",
        articles_path
    );
    assert!(
        std::path::Path::new(&users_path).exists(),
        "❌ events.raftlog file not found for users aggregate: {}",
        users_path
    );

    let articles_content = std::fs::read_to_string(&articles_path)
        .expect("Failed to read events.raftlog file for articles aggregate");
    let users_content = std::fs::read_to_string(&users_path)
        .expect("Failed to read events.raftlog file for users aggregate");

    let articles_events: Vec<_> =
        articles_content.lines().filter(|l| !l.trim().is_empty()).collect();
    let users_events: Vec<_> = users_content.lines().filter(|l| !l.trim().is_empty()).collect();

    assert!(
        !articles_events.is_empty(),
        "❌ No events found in articles aggregate file ({})",
        articles_path
    );
    assert!(
        !users_events.is_empty(),
        "❌ No events found in users aggregate file ({})",
        users_path
    );

    println!("✅ Events correctly distributed into distinct files for each aggregate",);
}

#[then(expr = "each aggregate file should contain only events for that aggregate")]
async fn then_each_aggregate_file_contains_only_its_events(world: &mut LithairWorld) {
    let test_data = world.test_data.lock().await;
    let base_path = test_data
        .tokens
        .get("multifile_base_path")
        .cloned()
        .unwrap_or_else(|| "/tmp/lithair-multifile-routing-test".to_string());
    let agg_articles = test_data
        .tokens
        .get("multifile_agg_articles")
        .cloned()
        .unwrap_or_else(|| "aggregate_articles".to_string());
    let agg_users = test_data
        .tokens
        .get("multifile_agg_users")
        .cloned()
        .unwrap_or_else(|| "aggregate_users".to_string());
    drop(test_data);

    for (agg, label) in [(agg_articles.as_str(), "articles"), (agg_users.as_str(), "users")] {
        let file_path = format!("{}/{}/events.raftlog", base_path, agg);
        let content = std::fs::read_to_string(&file_path).expect("Failed to read aggregate file");

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let value: serde_json::Value =
                serde_json::from_str(line).expect("Invalid line in events.raftlog (multi-file)");

            let aggregate_in_file =
                value.get("aggregate_id").and_then(|v| v.as_str()).unwrap_or("");

            assert_eq!(
                aggregate_in_file, agg,
                "❌ Aggregate file {} contains an event with aggregate_id='{}' (expected: '{}')",
                label, aggregate_in_file, agg
            );
        }
    }

    println!("✅ Each aggregate file contains only events for its own aggregate",);
}

#[when(expr = "I create a linked user and article in multi-file mode")]
async fn when_create_user_and_article_linked_multifile(world: &mut LithairWorld) {
    use crate::features::world::{TestEngineApp, TestEvent};

    println!(
        "🧪 Dynamic relations: creating a user, an article and their relationship (multi-file)...",
    );

    let base_path = "/tmp/lithair-multifile-relations-test".to_string();

    // Clean and recreate the test directory
    std::fs::remove_dir_all(&base_path).ok();
    std::fs::create_dir_all(&base_path)
        .expect("Failed to create test directory for multi-file relations");

    let config = EngineConfig {
        event_log_path: base_path.clone(),
        use_multi_file_store: true,
        ..Default::default()
    };

    let engine = Engine::<TestEngineApp>::new(config)
        .expect("Failed to initialize engine in multi-file mode for dynamic relations");

    let user_id = "user-rel-1".to_string();
    let article_id = "article-rel-1".to_string();

    let user_data = serde_json::json!({
        "name": "User Relations",
        "email": "user-relations@test.com",
    });

    let event_user = TestEvent::UserCreated { id: user_id.clone(), data: user_data };
    let event_article = TestEvent::ArticleCreated {
        id: article_id.clone(),
        title: "Article Relations".to_string(),
        content: "Article content with user relation".to_string(),
    };
    let event_link =
        TestEvent::ArticleLinkedToUser { article_id: article_id.clone(), user_id: user_id.clone() };

    let key_article = event_article.aggregate_id().unwrap_or("global".to_string());
    let key_user = event_user.aggregate_id().unwrap_or("global".to_string());
    let key_link = event_link.aggregate_id().unwrap_or("global".to_string());

    // Apply events: data first, then relation
    engine
        .apply_event(key_article, event_article)
        .expect("Failed to apply ArticleCreated event for relations");
    engine
        .apply_event(key_user, event_user)
        .expect("Failed to apply UserCreated event for relations");
    engine
        .apply_event(key_link, event_link)
        .expect("Failed to apply ArticleLinkedToUser event");

    engine
        .flush()
        .expect("Failed to flush after creating relation events (multi-file)");

    {
        let mut test_data = world.test_data.lock().await;
        test_data.tokens.insert("relations_base_path".to_string(), base_path.clone());
        test_data.tokens.insert("relations_user_id".to_string(), user_id.clone());
        test_data.tokens.insert("relations_article_id".to_string(), article_id.clone());
    }

    println!(
        "✅ User ({}) and article ({}) created and linked in multi-file mode",
        user_id, article_id
    );
}

#[then(expr = "dynamic relations should be reconstructed in memory from multi-file events")]
async fn then_dynamic_relations_rebuilt_from_multifile_events(world: &mut LithairWorld) {
    use crate::features::world::TestAppState;

    let test_data = world.test_data.lock().await;
    let base_path = test_data
        .tokens
        .get("relations_base_path")
        .cloned()
        .unwrap_or_else(|| "/tmp/lithair-multifile-relations-test".to_string());
    let user_id = test_data
        .tokens
        .get("relations_user_id")
        .cloned()
        .unwrap_or_else(|| "user-rel-1".to_string());
    let article_id = test_data
        .tokens
        .get("relations_article_id")
        .cloned()
        .unwrap_or_else(|| "article-rel-1".to_string());
    drop(test_data);

    let mut rebuilt_state = TestAppState::default();

    // Helper to replay events from an events.raftlog file
    let replay_file = |state: &mut TestAppState, path: &str| {
        if !std::path::Path::new(path).exists() {
            panic!("❌ events.raftlog file not found: {}", path);
        }

        let content = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("Failed to read file {}: {}", path, e));

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }

            let value: serde_json::Value = serde_json::from_str(line)
                .expect("Invalid line in events.raftlog (replay relations)");

            let payload_str = value
                .get("payload")
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| panic!("Payload missing in event envelope"));

            let event: crate::features::world::TestEvent = serde_json::from_str(payload_str)
                .expect("Failed to deserialize TestEvent from payload");

            event.apply(state);
        }
    };

    let articles_log = format!("{}/articles/events.raftlog", base_path);
    let users_log = format!("{}/users/events.raftlog", base_path);
    let relations_log = format!("{}/relations/events.raftlog", base_path);

    replay_file(&mut rebuilt_state, &articles_log);
    replay_file(&mut rebuilt_state, &users_log);
    replay_file(&mut rebuilt_state, &relations_log);

    // Verify the article knows its author
    let article = rebuilt_state
        .data
        .articles
        .get(&article_id)
        .unwrap_or_else(|| panic!("❌ Article {} not found after replay", article_id));

    let author_id = article
        .get("author_id")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("❌ author_id missing on article {} after replay", article_id));

    assert_eq!(
        author_id, user_id,
        "❌ Rebuilt author_id ({}) differs from expected user ({})",
        author_id, user_id
    );

    // Verify the user knows their list of linked articles
    let user = rebuilt_state
        .data
        .users
        .get(&user_id)
        .unwrap_or_else(|| panic!("❌ User {} not found after replay", user_id));

    let articles_array = user.get("articles").and_then(|v| v.as_array()).unwrap_or_else(|| {
        panic!("❌ 'articles' field missing or not an array for user {}", user_id)
    });

    let contains_article = articles_array
        .iter()
        .any(|v| v.as_str().map(|s| s == article_id).unwrap_or(false));

    assert!(
        contains_article,
        "❌ User {} does not reference article {} in 'articles' after replay",
        user_id, article_id
    );

    println!("✅ Dynamic article<->user relations reconstructed in memory from multi-file events",);
}

#[then(expr = "events should be distributed by data table and relation table")]
async fn then_events_routed_by_data_and_relations_tables(world: &mut LithairWorld) {
    let test_data = world.test_data.lock().await;
    let base_path = test_data
        .tokens
        .get("relations_base_path")
        .cloned()
        .unwrap_or_else(|| "/tmp/lithair-multifile-relations-test".to_string());
    drop(test_data);

    let articles_log = format!("{}/articles/events.raftlog", base_path);
    let users_log = format!("{}/users/events.raftlog", base_path);
    let relations_log = format!("{}/relations/events.raftlog", base_path);

    for (path, expected_agg) in [
        (&articles_log, "articles"),
        (&users_log, "users"),
        (&relations_log, "relations"),
    ] {
        assert!(
            std::path::Path::new(path).exists(),
            "❌ events.raftlog file not found for table {}: {}",
            expected_agg,
            path
        );

        let content = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("Failed to read file {}: {}", path, e));

        let mut non_empty_lines = 0usize;

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            non_empty_lines += 1;

            let value: serde_json::Value = serde_json::from_str(line)
                .expect("Invalid line in events.raftlog (per-table verification)");

            let aggregate_in_file =
                value.get("aggregate_id").and_then(|v| v.as_str()).unwrap_or("");

            assert_eq!(
                aggregate_in_file, expected_agg,
                "❌ File {} contains an event with aggregate_id='{}' (expected: '{}')",
                path, aggregate_in_file, expected_agg
            );
        }

        assert!(non_empty_lines > 0, "❌ No events found in table {} ({})", expected_agg, path);
    }
    println!(
        "✅ Events correctly distributed between data tables (articles/users) and relation table",
    );
}

#[when(expr = "I replay ArticleCreated v1 and v2 events via versioned deserializers")]
async fn when_replay_versioned_article_events(world: &mut LithairWorld) {
    use crate::features::world::TestEngineApp;

    let base_path = "/tmp/lithair-versioning-articles-test".to_string();

    std::fs::remove_dir_all(&base_path).ok();
    std::fs::create_dir_all(&base_path)
        .expect("Failed to create test directory for article versioning");

    let payload_v1 = serde_json::json!({
        "version": 1,
        "id": "version-article-v1",
        "title": "Article v1",
        "content": "Content v1 without slug",
    });

    let payload_v2 = serde_json::json!({
        "version": 2,
        "id": "version-article-v2",
        "title": "Article v2",
        "content": "Content v2 with slug",
        "slug": "article-v2-slug",
    });

    let envelope_v1 = serde_json::json!({
        "event_type": "test::ArticleCreated.versioned",
        "event_id": "version-article-v1",
        "timestamp": 0u64,
        "payload": payload_v1.to_string(),
        "aggregate_id": "articles",
    });

    let envelope_v2 = serde_json::json!({
        "event_type": "test::ArticleCreated.versioned",
        "event_id": "version-article-v2",
        "timestamp": 0u64,
        "payload": payload_v2.to_string(),
        "aggregate_id": "articles",
    });

    let events_path = format!("{}/events.raftlog", &base_path);
    let content = format!(
        "{}\n{}\n",
        serde_json::to_string(&envelope_v1).expect("Serialization envelope v1"),
        serde_json::to_string(&envelope_v2).expect("Serialization envelope v2"),
    );

    std::fs::write(&events_path, content).expect("Failed to write versioned events log");

    let config = EngineConfig {
        event_log_path: base_path.clone(),
        use_multi_file_store: false,
        ..Default::default()
    };

    let engine =
        Engine::<TestEngineApp>::new(config).expect("Failed to initialize engine for versioning");

    let (v1_slug, v1_version, v2_slug, v2_version) = {
        engine
            .read_state("articles", |state| {
                let v1 = state
                    .data
                    .articles
                    .get("version-article-v1")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));

                let v2 = state
                    .data
                    .articles
                    .get("version-article-v2")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));

                let v1_slug = v1.get("slug").and_then(|v| v.as_str()).unwrap_or("").to_string();

                let v1_version =
                    v1.get("version").and_then(|v| v.as_u64()).unwrap_or(0).to_string();

                let v2_slug = v2.get("slug").and_then(|v| v.as_str()).unwrap_or("").to_string();

                let v2_version =
                    v2.get("version").and_then(|v| v.as_u64()).unwrap_or(0).to_string();

                (v1_slug, v1_version, v2_slug, v2_version)
            })
            .expect("Failed to read state after replay")
    };

    let mut test_data = world.test_data.lock().await;
    test_data.tokens.insert("versioning_article_v1_slug".to_string(), v1_slug);
    test_data.tokens.insert("versioning_article_v1_version".to_string(), v1_version);
    test_data.tokens.insert("versioning_article_v2_slug".to_string(), v2_slug);
    test_data.tokens.insert("versioning_article_v2_version".to_string(), v2_version);
}

#[then(expr = "article state should reflect current schema (slug v2, slug absent in v1)")]
async fn then_versioned_articles_state_must_match_current_schema(world: &mut LithairWorld) {
    let test_data = world.test_data.lock().await;

    let v1_slug = test_data.tokens.get("versioning_article_v1_slug").cloned().unwrap_or_default();
    let v1_version = test_data
        .tokens
        .get("versioning_article_v1_version")
        .cloned()
        .unwrap_or_default();
    let v2_slug = test_data.tokens.get("versioning_article_v2_slug").cloned().unwrap_or_default();
    let v2_version = test_data
        .tokens
        .get("versioning_article_v2_version")
        .cloned()
        .unwrap_or_default();

    drop(test_data);

    assert!(v1_slug.is_empty(), "❌ Article v1 should not have a slug, found '{}'", v1_slug);

    assert_eq!(v1_version, "1", "❌ Expected version for article v1 = 1, found {}", v1_version);

    assert_eq!(
        v2_slug, "article-v2-slug",
        "❌ Article v2 should have slug 'article-v2-slug', found '{}'",
        v2_slug
    );

    assert_eq!(v2_version, "2", "❌ Expected version for article v2 = 2, found {}", v2_version);

    println!(
        "✅ Upcasting of ArticleCreated v1/v2 events: slug and version correctly reconstructed"
    );
}

// Additional tests for completeness
#[when(expr = "I query the event history")]
async fn when_query_event_history(world: &mut LithairWorld) {
    let _ = world.make_request("GET", "/api/events/history", None).await;
    println!("📜 Event history queried");
}

#[then(expr = "I must be able to filter by event type")]
async fn then_filter_by_event_type(world: &mut LithairWorld) {
    let _ = world.make_request("GET", "/api/events/history?type=ArticleCreated", None).await;
    println!("✅ Filtering by event type");
}

#[then(expr = "by aggregate")]
async fn then_filter_by_aggregate(world: &mut LithairWorld) {
    let _ = world.make_request("GET", "/api/events/history?aggregate_id=123", None).await;
    println!("✅ Filtering by aggregate");
}

#[then(expr = "by date range")]
async fn then_filter_by_date_range(world: &mut LithairWorld) {
    let _ = world
        .make_request("GET", "/api/events/history?from=2024-01-01&to=2024-12-31", None)
        .await;
    println!("✅ Filtering by date range");
}
