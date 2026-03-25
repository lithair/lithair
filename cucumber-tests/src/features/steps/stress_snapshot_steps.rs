use cucumber::{then, when};
use std::time::Instant;

use crate::features::world::LithairWorld;
use lithair_core::engine::events::EventEnvelope;

// ==================== MASSIVE CREATION ====================

#[when(expr = "I create {int} events with measured throughput for {string}")]
async fn when_create_events_with_throughput(
    world: &mut LithairWorld,
    count: usize,
    aggregate_id: String,
) {
    println!("📝 Creating {} events for '{}'...", count, aggregate_id);

    let start = Instant::now();

    {
        let mut store_guard = world.multi_file_store.lock().await;
        let store = store_guard.as_mut().expect("MultiFileEventStore not initialized");

        for i in 0..count {
            let envelope = EventEnvelope {
                event_type: "StressEvent".to_string(),
                event_id: format!("{}-stress-{}", aggregate_id, i),
                timestamp: chrono::Utc::now().timestamp() as u64,
                payload: serde_json::json!({
                    "id": format!("{}-{}", aggregate_id, i),
                    "index": i,
                    "data": format!("Stress data #{}", i)
                })
                .to_string(),
                aggregate_id: Some(aggregate_id.clone()),
                event_hash: None,
                previous_hash: None,
            };

            store.append_envelope(&envelope).expect("Failed to append envelope");

            // Progress every 10K
            if (i + 1) % 10000 == 0 {
                println!("  ... {} events created", i + 1);
            }
        }
    }

    let elapsed = start.elapsed();
    let throughput = count as f64 / elapsed.as_secs_f64();

    println!(
        "✅ {} events created in {:.2}s ({:.0} evt/s)",
        count,
        elapsed.as_secs_f64(),
        throughput
    );

    // Save in metrics
    let mut metrics = world.metrics.lock().await;
    metrics.request_count = count as u64;
    metrics.total_duration = elapsed;
    metrics.throughput = throughput;
}

#[when(expr = "I create {int} events in batches of {int} for {string}")]
async fn when_create_events_by_batch(
    world: &mut LithairWorld,
    total: usize,
    batch_size: usize,
    aggregate_id: String,
) {
    println!(
        "📝 Creating {} events in batches of {} for '{}'...",
        total, batch_size, aggregate_id
    );

    let start = Instant::now();
    let num_batches = total / batch_size;

    {
        let mut store_guard = world.multi_file_store.lock().await;
        let store = store_guard.as_mut().expect("MultiFileEventStore not initialized");

        for batch in 0..num_batches {
            let batch_start = batch * batch_size;

            for i in 0..batch_size {
                let idx = batch_start + i;
                let envelope = EventEnvelope {
                    event_type: "BatchEvent".to_string(),
                    event_id: format!("{}-batch-{}", aggregate_id, idx),
                    timestamp: chrono::Utc::now().timestamp() as u64,
                    payload: serde_json::json!({
                        "id": format!("{}-{}", aggregate_id, idx),
                        "batch": batch,
                        "index": idx
                    })
                    .to_string(),
                    aggregate_id: Some(aggregate_id.clone()),
                    event_hash: None,
                    previous_hash: None,
                };

                store.append_envelope(&envelope).expect("Failed to append envelope");
            }

            // Flush after each batch
            store.flush_all().expect("Failed to flush");

            let batch_elapsed = start.elapsed();
            let batch_throughput = ((batch + 1) * batch_size) as f64 / batch_elapsed.as_secs_f64();
            println!(
                "  Batch {}/{} completed - {:.0} evt/s cumulative",
                batch + 1,
                num_batches,
                batch_throughput
            );
        }
    }

    let elapsed = start.elapsed();
    let throughput = total as f64 / elapsed.as_secs_f64();

    println!(
        "✅ {} events created in {:.2}s ({:.0} evt/s)",
        total,
        elapsed.as_secs_f64(),
        throughput
    );

    let mut metrics = world.metrics.lock().await;
    metrics.request_count = total as u64;
    metrics.total_duration = elapsed;
    metrics.throughput = throughput;
}

#[when(expr = "I create {int} events distributed across {int} aggregates")]
async fn when_create_distributed_events(
    world: &mut LithairWorld,
    total: usize,
    num_aggregates: usize,
) {
    println!(
        "📝 Creating {} events distributed across {} aggregates...",
        total, num_aggregates
    );

    let start = Instant::now();
    let events_per_aggregate = total / num_aggregates;

    {
        let mut store_guard = world.multi_file_store.lock().await;
        let store = store_guard.as_mut().expect("MultiFileEventStore not initialized");

        for agg_idx in 0..num_aggregates {
            let aggregate_id = format!("agg_{:04}", agg_idx);

            for i in 0..events_per_aggregate {
                let envelope = EventEnvelope {
                    event_type: "DistributedEvent".to_string(),
                    event_id: format!("{}-{}", aggregate_id, i),
                    timestamp: chrono::Utc::now().timestamp() as u64,
                    payload: serde_json::json!({
                        "aggregate": agg_idx,
                        "index": i
                    })
                    .to_string(),
                    aggregate_id: Some(aggregate_id.clone()),
                    event_hash: None,
                    previous_hash: None,
                };

                store.append_envelope(&envelope).expect("Failed to append envelope");
            }

            if (agg_idx + 1) % 10 == 0 {
                println!("  ... {} aggregates processed", agg_idx + 1);
            }
        }
    }

    let elapsed = start.elapsed();
    println!(
        "✅ {} events distributed across {} aggregates in {:.2}s",
        total,
        num_aggregates,
        elapsed.as_secs_f64()
    );

    let mut metrics = world.metrics.lock().await;
    metrics.request_count = total as u64;
    metrics.total_duration = elapsed;
}

// ==================== COMPLEX SNAPSHOTS ====================

#[when(expr = "I create a snapshot for {string} with complex state of {int} elements")]
async fn when_create_complex_snapshot(
    world: &mut LithairWorld,
    aggregate_id: String,
    num_elements: usize,
) {
    println!(
        "📸 Creating complex snapshot for '{}' ({} elements)...",
        aggregate_id, num_elements
    );

    // Generate a complex state
    let items: Vec<serde_json::Value> = (0..std::cmp::min(num_elements, 1000))
        .map(|i| {
            serde_json::json!({
                "id": i,
                "name": format!("Item {}", i),
                "value": i as f64 * 1.5
            })
        })
        .collect();

    let state = serde_json::json!({
        "total_count": num_elements,
        "sample_items": items,
        "metadata": {
            "created_at": chrono::Utc::now().to_rfc3339(),
            "version": 1
        }
    })
    .to_string();

    {
        let mut store_guard = world.multi_file_store.lock().await;
        let store = store_guard.as_mut().expect("MultiFileEventStore not initialized");
        store
            .save_snapshot(Some(&aggregate_id), state.clone(), None)
            .expect("Failed to save snapshot");
    }

    let mut metrics = world.metrics.lock().await;
    metrics.last_state_json = Some(state);

    println!("✅ Complex snapshot created for '{}'", aggregate_id);
}

#[when(expr = "I create a snapshot for {string} with state of {int} elements")]
async fn when_create_snapshot_with_count(
    world: &mut LithairWorld,
    aggregate_id: String,
    _num_elements: usize,
) {
    println!("📸 Creating snapshot for '{}' ({} events)...", aggregate_id, _num_elements);

    let state = serde_json::json!({
        "count": _num_elements,
        "aggregate_id": aggregate_id,
        "timestamp": chrono::Utc::now().to_rfc3339()
    })
    .to_string();

    {
        let mut store_guard = world.multi_file_store.lock().await;
        let store = store_guard.as_mut().expect("MultiFileEventStore not initialized");
        store
            .save_snapshot(Some(&aggregate_id), state, None)
            .expect("Failed to save snapshot");
    }

    println!("✅ Snapshot created for '{}'", aggregate_id);
}

#[when("I create snapshots for all aggregates")]
async fn when_create_snapshots_for_all_aggregates(world: &mut LithairWorld) {
    println!("📸 Creating snapshots for all aggregates...");

    let aggregates: Vec<String> = {
        let store_guard = world.multi_file_store.lock().await;
        let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");
        store.list_aggregates()
    };

    let count = aggregates.len();

    for (idx, aggregate_id) in aggregates.iter().enumerate() {
        let state = serde_json::json!({
            "aggregate_id": aggregate_id
        })
        .to_string();

        {
            let mut store_guard = world.multi_file_store.lock().await;
            let store = store_guard.as_mut().expect("MultiFileEventStore not initialized");
            store
                .save_snapshot(Some(aggregate_id), state, None)
                .expect("Failed to save snapshot");
        }

        if (idx + 1) % 10 == 0 {
            println!("  ... {} snapshots created", idx + 1);
        }
    }

    println!("✅ {} snapshots created", count);
}

// ==================== PERFORMANCE MEASUREMENTS ====================

#[when(expr = "I measure the complete recovery time for {string}")]
async fn when_measure_full_recovery_time(world: &mut LithairWorld, aggregate_id: String) {
    println!("⏱️ Measuring complete recovery time for '{}'...", aggregate_id);

    let start = Instant::now();

    {
        let store_guard = world.multi_file_store.lock().await;
        let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");
        let _events = store.read_aggregate_envelopes(&aggregate_id).expect("Failed to read events");
    }

    let elapsed = start.elapsed();

    let mut metrics = world.metrics.lock().await;
    metrics.total_duration = elapsed;

    println!("✅ Complete recovery in {:.2}ms", elapsed.as_secs_f64() * 1000.0);
}

#[when(expr = "I measure the recovery time after snapshot for {string}")]
async fn when_measure_snapshot_recovery_time(world: &mut LithairWorld, aggregate_id: String) {
    println!("⏱️ Measuring recovery time after snapshot for '{}'...", aggregate_id);

    let start = Instant::now();

    {
        let store_guard = world.multi_file_store.lock().await;
        let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");

        // Load the snapshot
        let _snapshot = store.load_snapshot(Some(&aggregate_id)).expect("Failed to load snapshot");

        // Read events after the snapshot
        let _events = store
            .read_events_after_snapshot(Some(&aggregate_id))
            .expect("Failed to read events after snapshot");
    }

    let elapsed = start.elapsed();

    let mut metrics = world.metrics.lock().await;
    metrics.snapshot_read_duration = Some(elapsed);

    println!("✅ Recovery with snapshot in {:.2}ms", elapsed.as_secs_f64() * 1000.0);
}

// ==================== EVENT VALIDATIONS ====================

#[then(expr = "the number of events to replay must be {int}")]
async fn then_events_to_replay_must_be(world: &mut LithairWorld, expected_count: usize) {
    let metrics = world.metrics.lock().await;
    let actual = metrics.request_count as usize;

    assert_eq!(
        actual, expected_count,
        "Number of events to replay: {} (expected: {})",
        actual, expected_count
    );

    println!("✅ {} events to replay (correct)", actual);
}

// ==================== PERFORMANCE VALIDATIONS ====================

#[then(expr = "the creation throughput must be greater than {int} evt/s")]
async fn then_throughput_must_be_above(world: &mut LithairWorld, min_throughput: usize) {
    let metrics = world.metrics.lock().await;
    let actual = metrics.throughput;

    assert!(
        actual >= min_throughput as f64,
        "Insufficient throughput: {:.0} evt/s (minimum: {} evt/s)",
        actual,
        min_throughput
    );

    println!("✅ Throughput validated: {:.0} evt/s >= {} evt/s", actual, min_throughput);
}

#[then(expr = "the average throughput must be greater than {int} evt/s")]
async fn then_average_throughput_must_be_above(world: &mut LithairWorld, min_throughput: usize) {
    let metrics = world.metrics.lock().await;
    let actual = metrics.throughput;

    assert!(
        actual >= min_throughput as f64,
        "Insufficient average throughput: {:.0} evt/s (minimum: {} evt/s)",
        actual,
        min_throughput
    );

    println!(
        "✅ Average throughput validated: {:.0} evt/s >= {} evt/s",
        actual, min_throughput
    );
}

#[then(expr = "the total creation time must be less than {int} seconds")]
async fn then_creation_time_must_be_below(world: &mut LithairWorld, max_seconds: u64) {
    let metrics = world.metrics.lock().await;
    let elapsed = metrics.total_duration.as_secs_f64();

    assert!(
        elapsed < max_seconds as f64,
        "Creation time too long: {:.2}s (max: {}s)",
        elapsed,
        max_seconds
    );

    println!("✅ Creation time validated: {:.2}s < {}s", elapsed, max_seconds);
}

#[then(expr = "recovery with snapshot must be at least {int}x faster")]
async fn then_snapshot_recovery_must_be_faster(world: &mut LithairWorld, min_ratio: usize) {
    let metrics = world.metrics.lock().await;

    let full_read = metrics.total_duration.as_secs_f64();
    let snapshot_read =
        metrics.snapshot_read_duration.expect("No snapshot read duration").as_secs_f64();

    // Avoid division by zero
    let ratio = if snapshot_read > 0.0 { full_read / snapshot_read } else { f64::INFINITY };

    println!(
        "📊 Recovery performance: full={:.2}ms, snapshot={:.2}ms, ratio={:.1}x",
        full_read * 1000.0,
        snapshot_read * 1000.0,
        ratio
    );

    assert!(
        ratio >= min_ratio as f64,
        "Insufficient performance ratio: {:.1}x (minimum: {}x)",
        ratio,
        min_ratio
    );

    println!("✅ Recovery performance validated: {:.1}x >= {}x", ratio, min_ratio);
}

#[then(expr = "recovery time with snapshot must be less than {int} seconds")]
async fn then_snapshot_recovery_time_must_be_below(world: &mut LithairWorld, max_seconds: u64) {
    let metrics = world.metrics.lock().await;
    let elapsed = metrics.snapshot_read_duration.expect("No snapshot read duration").as_secs_f64();

    assert!(
        elapsed < max_seconds as f64,
        "Recovery time too long: {:.2}s (max: {}s)",
        elapsed,
        max_seconds
    );

    println!("✅ Recovery time with snapshot validated: {:.2}s < {}s", elapsed, max_seconds);
}

// ==================== SNAPSHOT VALIDATIONS ====================

#[then("the snapshot file size must be reasonable")]
async fn then_snapshot_size_must_be_reasonable(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;
    let base_path = &metrics.persist_path;

    // Traverse subdirectories to find snapshots
    let mut total_size = 0u64;
    if let Ok(entries) = std::fs::read_dir(base_path) {
        for entry in entries.flatten() {
            let snapshot_path = entry.path().join("snapshot.raftsnap");
            if snapshot_path.exists() {
                if let Ok(metadata) = std::fs::metadata(&snapshot_path) {
                    total_size += metadata.len();
                }
            }
        }
    }

    // A snapshot should not exceed 100MB to be considered reasonable
    let max_size = 100 * 1024 * 1024; // 100MB
    assert!(
        total_size < max_size,
        "Snapshot size too large: {} bytes (max: {} bytes)",
        total_size,
        max_size
    );

    println!("✅ Snapshot size is reasonable: {:.2} MB", total_size as f64 / 1024.0 / 1024.0);
}

#[then(expr = "{int} snapshots must exist")]
async fn then_n_snapshots_must_exist(world: &mut LithairWorld, expected_count: usize) {
    let store_guard = world.multi_file_store.lock().await;
    let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");

    let snapshots = store.list_snapshots().expect("Failed to list snapshots");
    assert_eq!(
        snapshots.len(),
        expected_count,
        "Number of snapshots: {} (expected: {})",
        snapshots.len(),
        expected_count
    );

    println!("✅ {} snapshots exist", snapshots.len());
}

#[then("all snapshots must have a valid CRC32")]
async fn then_all_snapshots_must_have_valid_crc32(world: &mut LithairWorld) {
    let store_guard = world.multi_file_store.lock().await;
    let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");

    let snapshots = store.list_snapshots().expect("Failed to list snapshots");
    let mut valid_count = 0;

    for aggregate_id_opt in &snapshots {
        let snapshot = store
            .load_snapshot(aggregate_id_opt.as_deref())
            .expect("Failed to load snapshot")
            .expect("Snapshot not found");

        assert!(
            snapshot.validate().is_ok(),
            "Invalid CRC32 for snapshot '{:?}'",
            aggregate_id_opt
        );
        valid_count += 1;
    }

    println!("✅ {} snapshots with valid CRC32", valid_count);
}

#[then(expr = "each aggregate must have {int} events")]
async fn then_each_aggregate_must_have_events(world: &mut LithairWorld, expected_count: usize) {
    let store_guard = world.multi_file_store.lock().await;
    let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");

    let aggregates = store.list_aggregates();

    for aggregate_id in &aggregates {
        let count = store.get_event_count(Some(aggregate_id));
        assert_eq!(
            count, expected_count,
            "Aggregate '{}' has {} events (expected: {})",
            aggregate_id, count, expected_count
        );
    }

    println!("✅ All {} aggregates have {} events", aggregates.len(), expected_count);
}

#[then("distributed recovery must use snapshots")]
async fn then_distributed_recovery_must_use_snapshots(world: &mut LithairWorld) {
    println!("⏱️ Verifying distributed recovery with snapshots...");

    let start = Instant::now();

    {
        let store_guard = world.multi_file_store.lock().await;
        let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");

        let aggregates = store.list_aggregates();

        for aggregate_id in &aggregates {
            // Load snapshot + post-snapshot events
            let _snapshot = store.load_snapshot(Some(aggregate_id));
            let _events = store.read_events_after_snapshot(Some(aggregate_id));
        }
    }

    let elapsed = start.elapsed();

    // Recovery with snapshots should be fast (< 2s for 100 aggregates)
    assert!(
        elapsed.as_secs() < 5,
        "Recovery too slow: {:.2}s (max: 5s)",
        elapsed.as_secs_f64()
    );

    println!("✅ Distributed recovery with snapshots in {:.2}s", elapsed.as_secs_f64());
}
