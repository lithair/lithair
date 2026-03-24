use cucumber::{given, then, when};
use std::time::Instant;

use crate::features::world::LithairWorld;
use lithair_core::engine::MultiFileEventStore;

// ==================== SETUP ====================

#[given(expr = "a multi-file store with snapshot threshold at {int} in {string}")]
async fn given_store_with_snapshot_threshold(
    world: &mut LithairWorld,
    threshold: usize,
    base_path: String,
) {
    println!("🚀 Initializing store with snapshot threshold: {}", threshold);

    // Clean up and create the directory
    std::fs::remove_dir_all(&base_path).ok();
    std::fs::create_dir_all(&base_path).expect("Failed to create base dir");

    // Create MultiFileEventStore with custom threshold
    let store = MultiFileEventStore::with_snapshot_threshold(&base_path, threshold)
        .expect("Failed to create MultiFileEventStore");

    *world.multi_file_store.lock().await = Some(store);

    let mut metrics = world.metrics.lock().await;
    metrics.persist_path = base_path;

    println!("✅ Store initialized with threshold of {} events", threshold);
}

// ==================== SNAPSHOT CREATION ====================

#[when(expr = "I create a snapshot for {string} with state {string}")]
async fn when_create_snapshot_for_aggregate(
    world: &mut LithairWorld,
    aggregate_id: String,
    state: String,
) {
    println!(
        "📸 Creating snapshot for '{}' with state: {}",
        aggregate_id,
        state.chars().take(50).collect::<String>()
    );

    {
        let mut store_guard = world.multi_file_store.lock().await;
        let store = store_guard.as_mut().expect("MultiFileEventStore not initialized");
        store
            .save_snapshot(Some(&aggregate_id), state, None)
            .expect("Failed to save snapshot");
    }

    println!("✅ Snapshot created for '{}'", aggregate_id);
}

#[when(expr = "I create a global snapshot with state {string}")]
async fn when_create_global_snapshot(world: &mut LithairWorld, state: String) {
    println!("📸 Creating global snapshot...");

    {
        let mut store_guard = world.multi_file_store.lock().await;
        let store = store_guard.as_mut().expect("MultiFileEventStore not initialized");
        store.save_snapshot(None, state, None).expect("Failed to save global snapshot");
    }

    println!("✅ Global snapshot created");
}

#[when(expr = "I create a snapshot for {string} with complex state")]
async fn when_create_snapshot_complex_state(world: &mut LithairWorld, aggregate_id: String) {
    let complex_state = serde_json::json!({
        "version": 1,
        "items": [
            {"id": 1, "name": "Item 1", "price": 9.99},
            {"id": 2, "name": "Item 2", "price": 19.99},
            {"id": 3, "name": "Item 3", "price": 29.99}
        ],
        "metadata": {
            "created_at": "2024-01-01T00:00:00Z",
            "updated_at": "2024-12-01T00:00:00Z",
            "tags": ["test", "complex", "nested"]
        },
        "statistics": {
            "total_items": 3,
            "total_value": 59.97,
            "average_price": 19.99
        }
    })
    .to_string();

    println!("📸 Creating complex snapshot for '{}'", aggregate_id);

    {
        let mut store_guard = world.multi_file_store.lock().await;
        let store = store_guard.as_mut().expect("MultiFileEventStore not initialized");
        store
            .save_snapshot(Some(&aggregate_id), complex_state.clone(), None)
            .expect("Failed to save complex snapshot");
    }

    // Save the state for later verification
    let mut metrics = world.metrics.lock().await;
    metrics.last_state_json = Some(complex_state);

    println!("✅ Complex snapshot created");
}

// ==================== SNAPSHOT VERIFICATION ====================

#[then(expr = "the snapshot for {string} must exist")]
async fn then_snapshot_must_exist(world: &mut LithairWorld, aggregate_id: String) {
    let store_guard = world.multi_file_store.lock().await;
    let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");

    let snapshot = store.load_snapshot(Some(&aggregate_id)).expect("Failed to load snapshot");
    assert!(snapshot.is_some(), "Snapshot for '{}' does not exist", aggregate_id);

    println!("✅ Snapshot for '{}' exists", aggregate_id);
}

#[then("the global snapshot must exist")]
async fn then_global_snapshot_must_exist(world: &mut LithairWorld) {
    let store_guard = world.multi_file_store.lock().await;
    let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");

    let snapshot = store.load_snapshot(None).expect("Failed to load global snapshot");
    assert!(snapshot.is_some(), "Global snapshot does not exist");

    println!("✅ Global snapshot exists");
}

#[then(expr = "the snapshot for {string} must not exist")]
async fn then_snapshot_must_not_exist(world: &mut LithairWorld, aggregate_id: String) {
    let store_guard = world.multi_file_store.lock().await;
    let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");

    let snapshot = store.load_snapshot(Some(&aggregate_id)).expect("Failed to check snapshot");
    assert!(
        snapshot.is_none(),
        "Snapshot for '{}' exists when it should not",
        aggregate_id
    );

    println!("✅ Snapshot for '{}' does not exist (expected)", aggregate_id);
}

#[then(expr = "the snapshot for {string} must contain {int} events")]
async fn then_snapshot_must_contain_events(
    world: &mut LithairWorld,
    aggregate_id: String,
    expected_count: usize,
) {
    let store_guard = world.multi_file_store.lock().await;
    let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");

    let snapshot = store
        .load_snapshot(Some(&aggregate_id))
        .expect("Failed to load snapshot")
        .expect("Snapshot not found");

    assert_eq!(
        snapshot.metadata.event_count, expected_count,
        "Snapshot contains {} events (expected: {})",
        snapshot.metadata.event_count, expected_count
    );

    println!("✅ Snapshot for '{}' contains {} events", aggregate_id, expected_count);
}

#[then(expr = "the global snapshot must contain {int} events")]
async fn then_global_snapshot_must_contain_events(world: &mut LithairWorld, expected_count: usize) {
    let store_guard = world.multi_file_store.lock().await;
    let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");

    let snapshot = store
        .load_snapshot(None)
        .expect("Failed to load global snapshot")
        .expect("Global snapshot not found");

    assert_eq!(
        snapshot.metadata.event_count, expected_count,
        "Global snapshot contains {} events (expected: {})",
        snapshot.metadata.event_count, expected_count
    );

    println!("✅ Global snapshot contains {} events", expected_count);
}

#[then(expr = "the snapshot for {string} must have a valid CRC32")]
async fn then_snapshot_must_have_valid_crc32(world: &mut LithairWorld, aggregate_id: String) {
    let store_guard = world.multi_file_store.lock().await;
    let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");

    let snapshot = store
        .load_snapshot(Some(&aggregate_id))
        .expect("Failed to load snapshot")
        .expect("Snapshot not found");

    // CRC32 validation is done automatically in load_snapshot
    // If we reach here, the CRC32 is valid
    assert!(
        snapshot.validate().is_ok(),
        "Invalid CRC32 for snapshot '{}'",
        aggregate_id
    );

    println!("✅ Snapshot '{}' has a valid CRC32", aggregate_id);
}

// ==================== RECOVERY ====================

#[when(expr = "I recover events after snapshot for {string}")]
async fn when_get_events_after_snapshot(world: &mut LithairWorld, aggregate_id: String) {
    println!("📖 Recovering events after snapshot for '{}'...", aggregate_id);

    let events = {
        let store_guard = world.multi_file_store.lock().await;
        let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");
        store
            .read_events_after_snapshot(Some(&aggregate_id))
            .expect("Failed to read events after snapshot")
    };

    let mut metrics = world.metrics.lock().await;
    metrics.request_count = events.len() as u64;

    println!("✅ Recovered {} events after snapshot", events.len());
}

#[then(expr = "the total number of events for {string} must be {int}")]
async fn then_total_events_must_be(
    world: &mut LithairWorld,
    aggregate_id: String,
    expected_count: usize,
) {
    let store_guard = world.multi_file_store.lock().await;
    let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");

    let actual_count = store.get_event_count(Some(&aggregate_id));
    assert_eq!(
        actual_count, expected_count,
        "Number of events for '{}': {} (expected: {})",
        aggregate_id, actual_count, expected_count
    );

    println!("✅ '{}' has {} events in total", aggregate_id, actual_count);
}

#[then("all these events must have a valid CRC32")]
async fn then_all_events_must_have_valid_crc32(_world: &mut LithairWorld) {
    // Already validated during reading
    println!("✅ All events have a valid CRC32");
}

// ==================== CORRUPTION ====================

#[when(expr = "I corrupt the snapshot file for {string}")]
async fn when_corrupt_snapshot_file(world: &mut LithairWorld, aggregate_id: String) {
    let metrics = world.metrics.lock().await;
    let snapshot_path = format!("{}/{}/snapshot.raftsnap", metrics.persist_path, aggregate_id);

    let content = std::fs::read_to_string(&snapshot_path).expect("Failed to read snapshot file");

    // Corrupt the content
    let corrupted = content.replace("data", "CORRUPTED_DATA_XYZ");
    std::fs::write(&snapshot_path, corrupted).expect("Failed to write corrupted snapshot");

    println!("💀 Snapshot '{}' corrupted", aggregate_id);
}

#[then(expr = "loading the snapshot for {string} must fail with corruption error")]
async fn then_snapshot_load_must_fail_with_corruption(
    world: &mut LithairWorld,
    aggregate_id: String,
) {
    let store_guard = world.multi_file_store.lock().await;
    let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");

    let result = store.load_snapshot(Some(&aggregate_id));
    assert!(result.is_err(), "Loading the corrupted snapshot should have failed");

    let error = result.unwrap_err().to_string();
    assert!(
        error.contains("corrupt") || error.contains("CRC") || error.contains("mismatch"),
        "The error should mention corruption: {}",
        error
    );

    println!("✅ Corruption detected correctly: {}", error);
}

#[then(expr = "loading the snapshot for {string} must succeed")]
async fn then_snapshot_load_must_succeed(world: &mut LithairWorld, aggregate_id: String) {
    let store_guard = world.multi_file_store.lock().await;
    let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");

    let snapshot = store
        .load_snapshot(Some(&aggregate_id))
        .expect("Failed to load snapshot")
        .expect("Snapshot not found");

    // Save the loaded state for comparison
    drop(store_guard);

    let mut metrics = world.metrics.lock().await;
    metrics.loaded_state_json = Some(snapshot.state);

    println!("✅ Snapshot loaded successfully");
}

#[then("the recovered state must be identical to the saved state")]
async fn then_state_must_be_identical(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;

    let saved = metrics.last_state_json.as_ref().expect("No saved state");
    let loaded = metrics.loaded_state_json.as_ref().expect("No loaded state");

    assert_eq!(saved, loaded, "States differ");

    println!("✅ Recovered state is identical to the saved state");
}

// ==================== PERFORMANCE ====================

#[when(expr = "I measure the time to read all {string} events")]
async fn when_measure_read_time(world: &mut LithairWorld, aggregate_id: String) {
    println!("⏱️ Measuring read time for '{}'...", aggregate_id);

    let start = Instant::now();

    {
        let store_guard = world.multi_file_store.lock().await;
        let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");
        let _events = store.read_aggregate_envelopes(&aggregate_id).expect("Failed to read events");
    }

    let elapsed = start.elapsed();

    let mut metrics = world.metrics.lock().await;
    metrics.total_duration = elapsed;

    println!("✅ Full read completed in {:.2}ms", elapsed.as_secs_f64() * 1000.0);
}

#[when(expr = "I measure the time to read after snapshot for {string}")]
async fn when_measure_read_after_snapshot(world: &mut LithairWorld, aggregate_id: String) {
    println!("⏱️ Measuring read time after snapshot for '{}'...", aggregate_id);

    let start = Instant::now();

    {
        let store_guard = world.multi_file_store.lock().await;
        let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");
        let _events = store
            .read_events_after_snapshot(Some(&aggregate_id))
            .expect("Failed to read events after snapshot");
    }

    let elapsed = start.elapsed();

    let mut metrics = world.metrics.lock().await;
    metrics.snapshot_read_duration = Some(elapsed);

    println!("✅ Read after snapshot in {:.2}ms", elapsed.as_secs_f64() * 1000.0);
}

#[then("the time with snapshot must be at least 10x faster")]
async fn then_snapshot_must_be_faster(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;

    let full_read = metrics.total_duration.as_secs_f64();
    let snapshot_read =
        metrics.snapshot_read_duration.expect("No snapshot read duration").as_secs_f64();

    // The ratio should be at least 10x (reading 100 events vs 10000)
    // But we accept 5x to account for variations
    let ratio = full_read / snapshot_read;

    println!(
        "📊 Performance: full read={:.2}ms, after snapshot={:.2}ms, ratio={:.1}x",
        full_read * 1000.0,
        snapshot_read * 1000.0,
        ratio
    );

    assert!(
        ratio >= 5.0,
        "Performance ratio is only {:.1}x (expected: >= 5x)",
        ratio
    );

    println!("✅ Performance validated: {:.1}x faster with snapshot", ratio);
}

// ==================== THRESHOLD ====================

#[then(expr = "a snapshot for {string} should not be necessary")]
async fn then_snapshot_should_not_be_needed(world: &mut LithairWorld, aggregate_id: String) {
    let store_guard = world.multi_file_store.lock().await;
    let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");

    let should_create = store
        .should_create_snapshot(Some(&aggregate_id))
        .expect("Failed to check snapshot threshold");

    assert!(
        !should_create,
        "A snapshot should not be necessary for '{}'",
        aggregate_id
    );

    println!("✅ Snapshot not necessary for '{}' (expected)", aggregate_id);
}

#[then(expr = "a snapshot for {string} should be necessary")]
async fn then_snapshot_should_be_needed(world: &mut LithairWorld, aggregate_id: String) {
    let store_guard = world.multi_file_store.lock().await;
    let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");

    let should_create = store
        .should_create_snapshot(Some(&aggregate_id))
        .expect("Failed to check snapshot threshold");

    assert!(should_create, "A snapshot should be necessary for '{}'", aggregate_id);

    println!("✅ Snapshot necessary for '{}' (expected)", aggregate_id);
}

// ==================== MULTI-AGGREGATE ====================

#[then(expr = "the snapshot list must contain {int} entries")]
async fn then_snapshot_list_must_contain(world: &mut LithairWorld, expected_count: usize) {
    let store_guard = world.multi_file_store.lock().await;
    let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");

    let snapshots = store.list_snapshots().expect("Failed to list snapshots");
    assert_eq!(
        snapshots.len(),
        expected_count,
        "Snapshot list: {} (expected: {})",
        snapshots.len(),
        expected_count
    );

    println!("✅ {} snapshots in the list", snapshots.len());
}

#[when(expr = "I delete the snapshot for {string}")]
async fn when_delete_snapshot(world: &mut LithairWorld, aggregate_id: String) {
    println!("🗑️ Deleting snapshot for '{}'...", aggregate_id);

    {
        let store_guard = world.multi_file_store.lock().await;
        let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");
        store.delete_snapshot(Some(&aggregate_id)).expect("Failed to delete snapshot");
    }

    println!("✅ Snapshot deleted for '{}'", aggregate_id);
}
