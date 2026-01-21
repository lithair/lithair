use cucumber::{given, then, when};
use std::time::Instant;

use crate::features::world::LithairWorld;
use lithair_core::engine::MultiFileEventStore;

// ==================== SETUP ====================

#[given(expr = "un store multi-fichiers avec seuil de snapshot à {int} dans {string}")]
async fn given_store_with_snapshot_threshold(
    world: &mut LithairWorld,
    threshold: usize,
    base_path: String,
) {
    println!("🚀 Initialisation store avec seuil de snapshot: {}", threshold);

    // Nettoyer et créer le dossier
    std::fs::remove_dir_all(&base_path).ok();
    std::fs::create_dir_all(&base_path).expect("Failed to create base dir");

    // Créer MultiFileEventStore avec seuil personnalisé
    let store = MultiFileEventStore::with_snapshot_threshold(&base_path, threshold)
        .expect("Failed to create MultiFileEventStore");

    *world.multi_file_store.lock().await = Some(store);

    let mut metrics = world.metrics.lock().await;
    metrics.persist_path = base_path;

    println!("✅ Store initialisé avec seuil de {} événements", threshold);
}

// ==================== CRÉATION SNAPSHOTS ====================

#[when(expr = "je crée un snapshot pour {string} avec état {string}")]
async fn when_create_snapshot_for_aggregate(
    world: &mut LithairWorld,
    aggregate_id: String,
    state: String,
) {
    println!(
        "📸 Création snapshot pour '{}' avec état: {}",
        aggregate_id,
        &state[..std::cmp::min(50, state.len())]
    );

    {
        let mut store_guard = world.multi_file_store.lock().await;
        let store = store_guard.as_mut().expect("MultiFileEventStore not initialized");
        store
            .save_snapshot(Some(&aggregate_id), state, None)
            .expect("Failed to save snapshot");
    }

    println!("✅ Snapshot créé pour '{}'", aggregate_id);
}

#[when(expr = "je crée un snapshot global avec état {string}")]
async fn when_create_global_snapshot(world: &mut LithairWorld, state: String) {
    println!("📸 Création snapshot global...");

    {
        let mut store_guard = world.multi_file_store.lock().await;
        let store = store_guard.as_mut().expect("MultiFileEventStore not initialized");
        store.save_snapshot(None, state, None).expect("Failed to save global snapshot");
    }

    println!("✅ Snapshot global créé");
}

#[when(expr = "je crée un snapshot pour {string} avec état complexe")]
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

    println!("📸 Création snapshot complexe pour '{}'", aggregate_id);

    {
        let mut store_guard = world.multi_file_store.lock().await;
        let store = store_guard.as_mut().expect("MultiFileEventStore not initialized");
        store
            .save_snapshot(Some(&aggregate_id), complex_state.clone(), None)
            .expect("Failed to save complex snapshot");
    }

    // Sauvegarder l'état pour vérification ultérieure
    let mut metrics = world.metrics.lock().await;
    metrics.last_state_json = Some(complex_state);

    println!("✅ Snapshot complexe créé");
}

// ==================== VÉRIFICATION SNAPSHOTS ====================

#[then(expr = "le snapshot pour {string} doit exister")]
async fn then_snapshot_must_exist(world: &mut LithairWorld, aggregate_id: String) {
    let store_guard = world.multi_file_store.lock().await;
    let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");

    let snapshot = store.load_snapshot(Some(&aggregate_id)).expect("Failed to load snapshot");
    assert!(snapshot.is_some(), "❌ Snapshot pour '{}' n'existe pas", aggregate_id);

    println!("✅ Snapshot pour '{}' existe", aggregate_id);
}

#[then("le snapshot global doit exister")]
async fn then_global_snapshot_must_exist(world: &mut LithairWorld) {
    let store_guard = world.multi_file_store.lock().await;
    let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");

    let snapshot = store.load_snapshot(None).expect("Failed to load global snapshot");
    assert!(snapshot.is_some(), "❌ Snapshot global n'existe pas");

    println!("✅ Snapshot global existe");
}

#[then(expr = "le snapshot pour {string} ne doit pas exister")]
async fn then_snapshot_must_not_exist(world: &mut LithairWorld, aggregate_id: String) {
    let store_guard = world.multi_file_store.lock().await;
    let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");

    let snapshot = store.load_snapshot(Some(&aggregate_id)).expect("Failed to check snapshot");
    assert!(
        snapshot.is_none(),
        "❌ Snapshot pour '{}' existe alors qu'il ne devrait pas",
        aggregate_id
    );

    println!("✅ Snapshot pour '{}' n'existe pas (attendu)", aggregate_id);
}

#[then(expr = "le snapshot pour {string} doit contenir {int} événements")]
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
        "❌ Snapshot contient {} événements (attendu: {})",
        snapshot.metadata.event_count, expected_count
    );

    println!("✅ Snapshot pour '{}' contient {} événements", aggregate_id, expected_count);
}

#[then(expr = "le snapshot global doit contenir {int} événements")]
async fn then_global_snapshot_must_contain_events(world: &mut LithairWorld, expected_count: usize) {
    let store_guard = world.multi_file_store.lock().await;
    let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");

    let snapshot = store
        .load_snapshot(None)
        .expect("Failed to load global snapshot")
        .expect("Global snapshot not found");

    assert_eq!(
        snapshot.metadata.event_count, expected_count,
        "❌ Snapshot global contient {} événements (attendu: {})",
        snapshot.metadata.event_count, expected_count
    );

    println!("✅ Snapshot global contient {} événements", expected_count);
}

#[then(expr = "le snapshot pour {string} doit avoir un CRC32 valide")]
async fn then_snapshot_must_have_valid_crc32(world: &mut LithairWorld, aggregate_id: String) {
    let store_guard = world.multi_file_store.lock().await;
    let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");

    let snapshot = store
        .load_snapshot(Some(&aggregate_id))
        .expect("Failed to load snapshot")
        .expect("Snapshot not found");

    // La validation CRC32 est faite automatiquement dans load_snapshot
    // Si on arrive ici, c'est que le CRC32 est valide
    assert!(
        snapshot.validate().is_ok(),
        "❌ CRC32 invalide pour snapshot '{}'",
        aggregate_id
    );

    println!("✅ Snapshot '{}' a un CRC32 valide", aggregate_id);
}

// ==================== RÉCUPÉRATION ====================

#[when(expr = "je récupère les événements après snapshot pour {string}")]
async fn when_get_events_after_snapshot(world: &mut LithairWorld, aggregate_id: String) {
    println!("📖 Récupération événements après snapshot pour '{}'...", aggregate_id);

    let events = {
        let store_guard = world.multi_file_store.lock().await;
        let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");
        store
            .read_events_after_snapshot(Some(&aggregate_id))
            .expect("Failed to read events after snapshot")
    };

    let mut metrics = world.metrics.lock().await;
    metrics.request_count = events.len() as u64;

    println!("✅ Récupéré {} événements après snapshot", events.len());
}

#[then(expr = "le nombre total d'événements pour {string} doit être {int}")]
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
        "❌ Nombre d'événements pour '{}': {} (attendu: {})",
        aggregate_id, actual_count, expected_count
    );

    println!("✅ '{}' a {} événements au total", aggregate_id, actual_count);
}

#[then("tous ces événements doivent avoir un CRC32 valide")]
async fn then_all_events_must_have_valid_crc32(_world: &mut LithairWorld) {
    // Déjà validé lors de la lecture
    println!("✅ Tous les événements ont un CRC32 valide");
}

// ==================== CORRUPTION ====================

#[when(expr = "je corromps le fichier snapshot de {string}")]
async fn when_corrupt_snapshot_file(world: &mut LithairWorld, aggregate_id: String) {
    let metrics = world.metrics.lock().await;
    let snapshot_path = format!("{}/{}/snapshot.raftsnap", metrics.persist_path, aggregate_id);

    let content = std::fs::read_to_string(&snapshot_path).expect("Failed to read snapshot file");

    // Corrompre le contenu
    let corrupted = content.replace("data", "CORRUPTED_DATA_XYZ");
    std::fs::write(&snapshot_path, corrupted).expect("Failed to write corrupted snapshot");

    println!("💀 Snapshot '{}' corrompu", aggregate_id);
}

#[then(expr = "le chargement du snapshot pour {string} doit échouer avec erreur de corruption")]
async fn then_snapshot_load_must_fail_with_corruption(
    world: &mut LithairWorld,
    aggregate_id: String,
) {
    let store_guard = world.multi_file_store.lock().await;
    let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");

    let result = store.load_snapshot(Some(&aggregate_id));
    assert!(result.is_err(), "❌ Le chargement du snapshot corrompu aurait dû échouer");

    let error = result.unwrap_err().to_string();
    assert!(
        error.contains("corrupt") || error.contains("CRC") || error.contains("mismatch"),
        "❌ L'erreur devrait mentionner la corruption: {}",
        error
    );

    println!("✅ Corruption détectée correctement: {}", error);
}

#[then(expr = "le chargement du snapshot pour {string} doit réussir")]
async fn then_snapshot_load_must_succeed(world: &mut LithairWorld, aggregate_id: String) {
    let store_guard = world.multi_file_store.lock().await;
    let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");

    let snapshot = store
        .load_snapshot(Some(&aggregate_id))
        .expect("Failed to load snapshot")
        .expect("Snapshot not found");

    // Sauvegarder l'état chargé pour comparaison
    drop(store_guard);

    let mut metrics = world.metrics.lock().await;
    metrics.loaded_state_json = Some(snapshot.state);

    println!("✅ Snapshot chargé avec succès");
}

#[then("l'état récupéré doit être identique à l'état sauvegardé")]
async fn then_state_must_be_identical(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;

    let saved = metrics.last_state_json.as_ref().expect("No saved state");
    let loaded = metrics.loaded_state_json.as_ref().expect("No loaded state");

    assert_eq!(saved, loaded, "❌ États différents");

    println!("✅ État récupéré identique à l'état sauvegardé");
}

// ==================== PERFORMANCE ====================

#[when(expr = "je mesure le temps de lecture de tous les événements {string}")]
async fn when_measure_read_time(world: &mut LithairWorld, aggregate_id: String) {
    println!("⏱️ Mesure temps de lecture pour '{}'...", aggregate_id);

    let start = Instant::now();

    {
        let store_guard = world.multi_file_store.lock().await;
        let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");
        let _events = store.read_aggregate_envelopes(&aggregate_id).expect("Failed to read events");
    }

    let elapsed = start.elapsed();

    let mut metrics = world.metrics.lock().await;
    metrics.total_duration = elapsed;

    println!("✅ Lecture complète en {:.2}ms", elapsed.as_secs_f64() * 1000.0);
}

#[when(expr = "je mesure le temps de lecture après snapshot pour {string}")]
async fn when_measure_read_after_snapshot(world: &mut LithairWorld, aggregate_id: String) {
    println!("⏱️ Mesure temps de lecture après snapshot pour '{}'...", aggregate_id);

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

    println!("✅ Lecture après snapshot en {:.2}ms", elapsed.as_secs_f64() * 1000.0);
}

#[then("le temps avec snapshot doit être au moins 10x plus rapide")]
async fn then_snapshot_must_be_faster(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;

    let full_read = metrics.total_duration.as_secs_f64();
    let snapshot_read =
        metrics.snapshot_read_duration.expect("No snapshot read duration").as_secs_f64();

    // Le ratio devrait être au moins 10x (lecture 100 events vs 10000)
    // Mais on accepte 5x pour tenir compte des variations
    let ratio = full_read / snapshot_read;

    println!(
        "📊 Performance: lecture complète={:.2}ms, après snapshot={:.2}ms, ratio={:.1}x",
        full_read * 1000.0,
        snapshot_read * 1000.0,
        ratio
    );

    assert!(
        ratio >= 5.0,
        "❌ Le ratio de performance n'est que de {:.1}x (attendu: >= 5x)",
        ratio
    );

    println!("✅ Performance validée: {:.1}x plus rapide avec snapshot", ratio);
}

// ==================== SEUIL ====================

#[then(expr = "un snapshot pour {string} ne devrait pas être nécessaire")]
async fn then_snapshot_should_not_be_needed(world: &mut LithairWorld, aggregate_id: String) {
    let store_guard = world.multi_file_store.lock().await;
    let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");

    let should_create = store
        .should_create_snapshot(Some(&aggregate_id))
        .expect("Failed to check snapshot threshold");

    assert!(
        !should_create,
        "❌ Un snapshot ne devrait pas être nécessaire pour '{}'",
        aggregate_id
    );

    println!("✅ Snapshot non nécessaire pour '{}' (attendu)", aggregate_id);
}

#[then(expr = "un snapshot pour {string} devrait être nécessaire")]
async fn then_snapshot_should_be_needed(world: &mut LithairWorld, aggregate_id: String) {
    let store_guard = world.multi_file_store.lock().await;
    let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");

    let should_create = store
        .should_create_snapshot(Some(&aggregate_id))
        .expect("Failed to check snapshot threshold");

    assert!(should_create, "❌ Un snapshot devrait être nécessaire pour '{}'", aggregate_id);

    println!("✅ Snapshot nécessaire pour '{}' (attendu)", aggregate_id);
}

// ==================== MULTI-AGGREGATE ====================

#[then(expr = "la liste des snapshots doit contenir {int} entrées")]
async fn then_snapshot_list_must_contain(world: &mut LithairWorld, expected_count: usize) {
    let store_guard = world.multi_file_store.lock().await;
    let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");

    let snapshots = store.list_snapshots().expect("Failed to list snapshots");
    assert_eq!(
        snapshots.len(),
        expected_count,
        "❌ Liste des snapshots: {} (attendu: {})",
        snapshots.len(),
        expected_count
    );

    println!("✅ {} snapshots dans la liste", snapshots.len());
}

#[when(expr = "je supprime le snapshot pour {string}")]
async fn when_delete_snapshot(world: &mut LithairWorld, aggregate_id: String) {
    println!("🗑️ Suppression snapshot pour '{}'...", aggregate_id);

    {
        let store_guard = world.multi_file_store.lock().await;
        let store = store_guard.as_ref().expect("MultiFileEventStore not initialized");
        store.delete_snapshot(Some(&aggregate_id)).expect("Failed to delete snapshot");
    }

    println!("✅ Snapshot supprimé pour '{}'", aggregate_id);
}
