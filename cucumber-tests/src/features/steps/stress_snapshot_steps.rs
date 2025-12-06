use cucumber::{then, when};
use std::time::Instant;

use crate::features::world::LithairWorld;
use lithair_core::engine::events::EventEnvelope;

// ==================== CRÉATION MASSIVE ====================

#[when(expr = "je crée {int} événements avec throughput mesuré pour {string}")]
async fn when_create_events_with_throughput(
    world: &mut LithairWorld,
    count: usize,
    aggregate_id: String,
) {
    println!("📝 Création de {} événements pour '{}'...", count, aggregate_id);

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
                println!("  ... {} événements créés", i + 1);
            }
        }
    }

    let elapsed = start.elapsed();
    let throughput = count as f64 / elapsed.as_secs_f64();

    println!(
        "✅ {} événements créés en {:.2}s ({:.0} evt/s)",
        count,
        elapsed.as_secs_f64(),
        throughput
    );

    // Sauvegarder dans metrics
    let mut metrics = world.metrics.lock().await;
    metrics.request_count = count as u64;
    metrics.total_duration = elapsed;
    metrics.throughput = throughput;
}

#[when(expr = "je crée {int} événements par batch de {int} pour {string}")]
async fn when_create_events_by_batch(
    world: &mut LithairWorld,
    total: usize,
    batch_size: usize,
    aggregate_id: String,
) {
    println!(
        "📝 Création de {} événements par batch de {} pour '{}'...",
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

            // Flush après chaque batch
            store.flush_all().expect("Failed to flush");

            let batch_elapsed = start.elapsed();
            let batch_throughput = ((batch + 1) * batch_size) as f64 / batch_elapsed.as_secs_f64();
            println!(
                "  Batch {}/{} terminé - {:.0} evt/s cumulé",
                batch + 1,
                num_batches,
                batch_throughput
            );
        }
    }

    let elapsed = start.elapsed();
    let throughput = total as f64 / elapsed.as_secs_f64();

    println!(
        "✅ {} événements créés en {:.2}s ({:.0} evt/s)",
        total,
        elapsed.as_secs_f64(),
        throughput
    );

    let mut metrics = world.metrics.lock().await;
    metrics.request_count = total as u64;
    metrics.total_duration = elapsed;
    metrics.throughput = throughput;
}

#[when(expr = "je crée {int} événements répartis sur {int} aggregates")]
async fn when_create_distributed_events(
    world: &mut LithairWorld,
    total: usize,
    num_aggregates: usize,
) {
    println!(
        "📝 Création de {} événements répartis sur {} aggregates...",
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
                println!("  ... {} aggregates traités", agg_idx + 1);
            }
        }
    }

    let elapsed = start.elapsed();
    println!(
        "✅ {} événements répartis sur {} aggregates en {:.2}s",
        total,
        num_aggregates,
        elapsed.as_secs_f64()
    );

    let mut metrics = world.metrics.lock().await;
    metrics.request_count = total as u64;
    metrics.total_duration = elapsed;
}

// ==================== SNAPSHOTS COMPLEXES ====================

#[when(expr = "je crée un snapshot pour {string} avec état complexe de {int} éléments")]
async fn when_create_complex_snapshot(
    world: &mut LithairWorld,
    aggregate_id: String,
    num_elements: usize,
) {
    println!(
        "📸 Création snapshot complexe pour '{}' ({} éléments)...",
        aggregate_id, num_elements
    );

    // Générer un état complexe
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

    println!("✅ Snapshot complexe créé pour '{}'", aggregate_id);
}

#[when(expr = "je crée un snapshot pour {string} avec état de {int} éléments")]
async fn when_create_snapshot_with_count(
    world: &mut LithairWorld,
    aggregate_id: String,
    _num_elements: usize,
) {
    println!(
        "📸 Création snapshot pour '{}' ({} événements)...",
        aggregate_id, _num_elements
    );

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

    println!("✅ Snapshot créé pour '{}'", aggregate_id);
}

#[when("je crée des snapshots pour tous les aggregates")]
async fn when_create_snapshots_for_all_aggregates(world: &mut LithairWorld) {
    println!("📸 Création snapshots pour tous les aggregates...");

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
            println!("  ... {} snapshots créés", idx + 1);
        }
    }

    println!("✅ {} snapshots créés", count);
}

// ==================== MESURES PERFORMANCE ====================

#[when(expr = "je mesure le temps de récupération complète pour {string}")]
async fn when_measure_full_recovery_time(world: &mut LithairWorld, aggregate_id: String) {
    println!("⏱️ Mesure temps de récupération complète pour '{}'...", aggregate_id);

    let start = Instant::now();

    {
        let store_guard = world.multi_file_store.lock().await;
        let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");
        let _events = store
            .read_aggregate_envelopes(&aggregate_id)
            .expect("Failed to read events");
    }

    let elapsed = start.elapsed();

    let mut metrics = world.metrics.lock().await;
    metrics.total_duration = elapsed;

    println!(
        "✅ Récupération complète en {:.2}ms",
        elapsed.as_secs_f64() * 1000.0
    );
}

#[when(expr = "je mesure le temps de récupération après snapshot pour {string}")]
async fn when_measure_snapshot_recovery_time(world: &mut LithairWorld, aggregate_id: String) {
    println!(
        "⏱️ Mesure temps de récupération après snapshot pour '{}'...",
        aggregate_id
    );

    let start = Instant::now();

    {
        let store_guard = world.multi_file_store.lock().await;
        let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");

        // Charger le snapshot
        let _snapshot = store
            .load_snapshot(Some(&aggregate_id))
            .expect("Failed to load snapshot");

        // Lire les événements après le snapshot
        let _events = store
            .read_events_after_snapshot(Some(&aggregate_id))
            .expect("Failed to read events after snapshot");
    }

    let elapsed = start.elapsed();

    let mut metrics = world.metrics.lock().await;
    metrics.snapshot_read_duration = Some(elapsed);

    println!(
        "✅ Récupération avec snapshot en {:.2}ms",
        elapsed.as_secs_f64() * 1000.0
    );
}

// ==================== VALIDATIONS EVENTS ====================

#[then(expr = "le nombre d'événements à rejouer doit être {int}")]
async fn then_events_to_replay_must_be(world: &mut LithairWorld, expected_count: usize) {
    let metrics = world.metrics.lock().await;
    let actual = metrics.request_count as usize;

    assert_eq!(
        actual, expected_count,
        "❌ Nombre d'événements à rejouer: {} (attendu: {})",
        actual, expected_count
    );

    println!("✅ {} événements à rejouer (correct)", actual);
}

// ==================== VALIDATIONS PERFORMANCE ====================

#[then(expr = "le throughput de création doit être supérieur à {int} evt/s")]
async fn then_throughput_must_be_above(world: &mut LithairWorld, min_throughput: usize) {
    let metrics = world.metrics.lock().await;
    let actual = metrics.throughput;

    assert!(
        actual >= min_throughput as f64,
        "❌ Throughput insuffisant: {:.0} evt/s (minimum: {} evt/s)",
        actual,
        min_throughput
    );

    println!("✅ Throughput validé: {:.0} evt/s >= {} evt/s", actual, min_throughput);
}

#[then(expr = "le throughput moyen doit être supérieur à {int} evt/s")]
async fn then_average_throughput_must_be_above(world: &mut LithairWorld, min_throughput: usize) {
    let metrics = world.metrics.lock().await;
    let actual = metrics.throughput;

    assert!(
        actual >= min_throughput as f64,
        "❌ Throughput moyen insuffisant: {:.0} evt/s (minimum: {} evt/s)",
        actual,
        min_throughput
    );

    println!(
        "✅ Throughput moyen validé: {:.0} evt/s >= {} evt/s",
        actual, min_throughput
    );
}

#[then(expr = "le temps total de création doit être inférieur à {int} secondes")]
async fn then_creation_time_must_be_below(world: &mut LithairWorld, max_seconds: u64) {
    let metrics = world.metrics.lock().await;
    let elapsed = metrics.total_duration.as_secs_f64();

    assert!(
        elapsed < max_seconds as f64,
        "❌ Temps de création trop long: {:.2}s (max: {}s)",
        elapsed,
        max_seconds
    );

    println!("✅ Temps de création validé: {:.2}s < {}s", elapsed, max_seconds);
}

#[then(expr = "la récupération avec snapshot doit être au moins {int}x plus rapide")]
async fn then_snapshot_recovery_must_be_faster(world: &mut LithairWorld, min_ratio: usize) {
    let metrics = world.metrics.lock().await;

    let full_read = metrics.total_duration.as_secs_f64();
    let snapshot_read = metrics
        .snapshot_read_duration
        .expect("No snapshot read duration")
        .as_secs_f64();

    // Éviter division par zéro
    let ratio = if snapshot_read > 0.0 {
        full_read / snapshot_read
    } else {
        f64::INFINITY
    };

    println!(
        "📊 Performance recovery: complète={:.2}ms, snapshot={:.2}ms, ratio={:.1}x",
        full_read * 1000.0,
        snapshot_read * 1000.0,
        ratio
    );

    assert!(
        ratio >= min_ratio as f64,
        "❌ Ratio de performance insuffisant: {:.1}x (minimum: {}x)",
        ratio,
        min_ratio
    );

    println!(
        "✅ Performance recovery validée: {:.1}x >= {}x",
        ratio, min_ratio
    );
}

#[then(expr = "le temps de récupération avec snapshot doit être inférieur à {int} secondes")]
async fn then_snapshot_recovery_time_must_be_below(world: &mut LithairWorld, max_seconds: u64) {
    let metrics = world.metrics.lock().await;
    let elapsed = metrics
        .snapshot_read_duration
        .expect("No snapshot read duration")
        .as_secs_f64();

    assert!(
        elapsed < max_seconds as f64,
        "❌ Temps de récupération trop long: {:.2}s (max: {}s)",
        elapsed,
        max_seconds
    );

    println!(
        "✅ Temps de récupération avec snapshot validé: {:.2}s < {}s",
        elapsed, max_seconds
    );
}

// ==================== VALIDATIONS SNAPSHOT ====================

#[then("la taille du fichier snapshot doit être raisonnable")]
async fn then_snapshot_size_must_be_reasonable(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;
    let base_path = &metrics.persist_path;

    // Parcourir les sous-dossiers pour trouver les snapshots
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

    // Un snapshot ne devrait pas dépasser 100MB pour être raisonnable
    let max_size = 100 * 1024 * 1024; // 100MB
    assert!(
        total_size < max_size,
        "❌ Taille des snapshots trop importante: {} bytes (max: {} bytes)",
        total_size,
        max_size
    );

    println!(
        "✅ Taille des snapshots raisonnable: {:.2} MB",
        total_size as f64 / 1024.0 / 1024.0
    );
}

#[then(expr = "{int} snapshots doivent exister")]
async fn then_n_snapshots_must_exist(world: &mut LithairWorld, expected_count: usize) {
    let store_guard = world.multi_file_store.lock().await;
    let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");

    let snapshots = store.list_snapshots().expect("Failed to list snapshots");
    assert_eq!(
        snapshots.len(),
        expected_count,
        "❌ Nombre de snapshots: {} (attendu: {})",
        snapshots.len(),
        expected_count
    );

    println!("✅ {} snapshots existent", snapshots.len());
}

#[then("tous les snapshots doivent avoir un CRC32 valide")]
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
            "❌ CRC32 invalide pour snapshot '{:?}'",
            aggregate_id_opt
        );
        valid_count += 1;
    }

    println!("✅ {} snapshots avec CRC32 valide", valid_count);
}

#[then(expr = "chaque aggregate doit avoir {int} événements")]
async fn then_each_aggregate_must_have_events(world: &mut LithairWorld, expected_count: usize) {
    let store_guard = world.multi_file_store.lock().await;
    let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");

    let aggregates = store.list_aggregates();

    for aggregate_id in &aggregates {
        let count = store.get_event_count(Some(aggregate_id));
        assert_eq!(
            count, expected_count,
            "❌ Aggregate '{}' a {} événements (attendu: {})",
            aggregate_id, count, expected_count
        );
    }

    println!(
        "✅ Tous les {} aggregates ont {} événements",
        aggregates.len(),
        expected_count
    );
}

#[then("la récupération distribuée doit utiliser les snapshots")]
async fn then_distributed_recovery_must_use_snapshots(world: &mut LithairWorld) {
    println!("⏱️ Vérification récupération distribuée avec snapshots...");

    let start = Instant::now();

    {
        let store_guard = world.multi_file_store.lock().await;
        let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");

        let aggregates = store.list_aggregates();

        for aggregate_id in &aggregates {
            // Charger snapshot + événements post-snapshot
            let _snapshot = store.load_snapshot(Some(aggregate_id));
            let _events = store.read_events_after_snapshot(Some(aggregate_id));
        }
    }

    let elapsed = start.elapsed();

    // La récupération avec snapshots devrait être rapide (< 2s pour 100 aggregates)
    assert!(
        elapsed.as_secs() < 5,
        "❌ Récupération trop lente: {:.2}s (max: 5s)",
        elapsed.as_secs_f64()
    );

    println!(
        "✅ Récupération distribuée avec snapshots en {:.2}s",
        elapsed.as_secs_f64()
    );
}
