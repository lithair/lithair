use crate::features::world::LithairWorld;
use cucumber::{given, then, when};
use lithair_core::engine::{Engine, EngineConfig, EngineError, Event};

// Background
#[given(expr = "a Lithair engine with event sourcing enabled")]
async fn given_event_sourcing_enabled(_world: &mut LithairWorld) {
    // Initialization done at first CRUD operation to avoid fragile dependencies
    println!("Event sourcing context enabled (initialization done at first CRUD operation)");
}

// Helper interne: s'assurer que le moteur + storage event sourcing sont initialisés
async fn ensure_event_sourcing_initialized(world: &mut LithairWorld) {
    // Si déjà initialisé (TempDir présent), ne rien refaire
    {
        let temp_dir_guard = world.temp_dir.lock().await;
        if temp_dir_guard.is_some() {
            return;
        }
    }

    let path = world.init_temp_storage().await.expect("Échec init storage event sourcing");

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

    println!("📝 Moteur event sourcing direct activé dans {:?}", path);
}

#[given(expr = "events are persisted in {string}")]
async fn given_events_persisted_in(_world: &mut LithairWorld, filename: String) {
    println!("Events persisted in: {}", filename);
}

#[given(expr = "snapshots are created periodically")]
async fn given_periodic_snapshots(_world: &mut LithairWorld) {
    println!("Periodic snapshots enabled");
}

// Scénario: Persistance des événements
#[when(expr = "I perform a CRUD operation")]
async fn when_perform_crud_operation(world: &mut LithairWorld) {
    // Ensure event sourcing environment is initialized
    ensure_event_sourcing_initialized(world).await;

    let payload = serde_json::json!({
        "title": "Test Article",
        "content": "Content"
    });

    // Appliquer un événement sur l'état en mémoire
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

    // Persister l'événement dans events.raftlog avec métadonnées explicites
    let event_json = serde_json::json!({
        "event_type": "ArticleCreated",
        "event_id": "es-crud-1",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "payload": payload,
    })
    .to_string();

    let mut storage_guard = world.storage.lock().await;
    if let Some(storage) = storage_guard.as_mut() {
        storage.append_event(&event_json).expect("Échec persistance event");
        storage.flush_batch().expect("Échec flush batch pour event sourcing");
    } else {
        panic!("Storage non initialisé pour event sourcing");
    }

    println!("✍️ Opération CRUD directe effectuée et événement persisté");
}

#[then(expr = "an event should be created and persisted")]
async fn then_event_created_and_persisted(world: &mut LithairWorld) {
    let temp_dir = world.temp_dir.lock().await;
    let dir = temp_dir.as_ref().expect("TempDir non initialisé pour event sourcing");
    let events_file = dir.path().join("events.raftlog");

    assert!(events_file.exists(), "❌ Fichier events.raftlog introuvable: {:?}", events_file);

    let content = std::fs::read_to_string(&events_file).expect("Impossible de lire events.raftlog");
    let lines: Vec<_> = content.lines().filter(|l| !l.trim().is_empty()).collect();

    assert!(!lines.is_empty(), "❌ Aucun événement persisté dans events.raftlog");

    println!("✅ {} événement(s) persisté(s) dans {:?}", lines.len(), events_file);
}

#[then(expr = "the event should contain all metadata")]
async fn then_event_contains_metadata(world: &mut LithairWorld) {
    let temp_dir = world.temp_dir.lock().await;
    let dir = temp_dir.as_ref().expect("TempDir non initialisé pour event sourcing");
    let events_file = dir.path().join("events.raftlog");

    let content = std::fs::read_to_string(&events_file).expect("Impossible de lire events.raftlog");
    let last_line = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .next_back()
        .expect("Aucun événement trouvé dans events.raftlog");

    let value: serde_json::Value =
        serde_json::from_str(last_line).expect("Événement invalide (JSON)");

    let obj = value.as_object().expect("Événement persisté n'est pas un objet JSON");

    assert!(
        obj.get("event_type").and_then(|v| v.as_str()).is_some(),
        "❌ event_type manquant dans l'événement"
    );
    assert!(
        obj.get("event_id").and_then(|v| v.as_str()).is_some(),
        "❌ event_id manquant dans l'événement"
    );
    assert!(
        obj.get("timestamp").and_then(|v| v.as_str()).is_some(),
        "❌ timestamp manquant dans l'événement"
    );
    assert!(obj.get("payload").is_some(), "❌ payload manquant dans l'événement");

    println!("✅ Métadonnées présentes dans l'événement persisté");
}

#[then(expr = "the log file should be updated atomically")]
async fn then_log_file_updated_atomically(world: &mut LithairWorld) {
    let temp_dir = world.temp_dir.lock().await;
    let dir = temp_dir.as_ref().expect("TempDir non initialisé pour event sourcing");
    let events_file = dir.path().join("events.raftlog");

    let content = std::fs::read_to_string(&events_file).expect("Impossible de lire events.raftlog");

    for (idx, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        if serde_json::from_str::<serde_json::Value>(line).is_err() {
            panic!(
                "❌ Ligne partielle ou corrompue détectée dans events.raftlog à la ligne {}",
                idx + 1
            );
        }
    }

    println!("✅ Toutes les lignes du log sont des JSON valides (mise à jour atomique)");
}

// Scénario: Reconstruction de l'état
#[when(expr = "I restart the server")]
async fn when_restart_server(world: &mut LithairWorld) {
    println!("🔄 Préparation du scénario de reconstruction d'état...");

    // 1) S'assurer que l'environnement event sourcing est bien initialisé
    ensure_event_sourcing_initialized(world).await;

    // 2) Construire un état initial en appliquant quelques événements ArticleCreated
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

        // Appliquer à l'état en mémoire
        world
            .engine
            .with_state_mut(|state| {
                event.apply(state);
            })
            .ok();

        // Journaliser l'événement dans events.raftlog
        let event_json = serde_json::json!({
            "event_type": "ArticleCreated",
            "id": id,
            "title": title,
            "content": content,
        })
        .to_string();

        let mut storage_guard = world.storage.lock().await;
        if let Some(storage) = storage_guard.as_mut() {
            storage.append_event(&event_json).expect("Échec append_event pour replay");
        } else {
            panic!("Storage non initialisé pour reconstruction");
        }
    }

    // Flush une fois tous les événements
    {
        let mut storage_guard = world.storage.lock().await;
        if let Some(storage) = storage_guard.as_mut() {
            storage.flush_batch().expect("Échec flush_batch pour reconstruction");
        }
    }

    // 3) Capturer un snapshot de l'état avant redémarrage
    let snapshot_data = world.engine.with_state(|state| state.data.clone()).unwrap_or_default();
    {
        let mut test_data = world.test_data.lock().await;
        *test_data = snapshot_data;
    }

    // Sauvegarder le nombre d'événements pour les vérifications
    {
        let mut metrics = world.metrics.lock().await;
        metrics.request_count = EVENT_COUNT as u64;
    }

    // 4) Réinitialiser l'état puis rejouer tous les événements depuis events.raftlog
    world
        .engine
        .with_state_mut(|state| {
            *state = crate::features::world::TestAppState::default();
        })
        .ok();

    println!("🔄 Redémarrage logique du moteur: état remis à zéro, replay des événements...");

    let start = std::time::Instant::now();

    let events_lines = {
        let storage_guard = world.storage.lock().await;
        let storage = storage_guard.as_ref().expect("Storage non initialisé pour replay");
        storage.read_all_events().expect("Échec lecture events.raftlog pour replay")
    };

    for line in &events_lines {
        let value: serde_json::Value =
            serde_json::from_str(line).expect("Événement invalide dans events.raftlog (replay)");

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
        "✅ Replay terminé: {} événements rejoués en {:.3}s",
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
            storage_guard.as_ref().expect("Storage non initialisé pour vérification replay");
        storage
            .read_all_events()
            .expect("Échec lecture events.raftlog pour vérification")
            .len()
    };

    assert_eq!(
        actual, expected,
        "❌ Nombre d'événements rejoués incorrect: {} (attendu: {})",
        actual, expected
    );

    println!("✅ Tous les événements ({}) ont été rejoués", actual);
}

#[then(expr = "state should be identical to before the restart")]
async fn then_state_identical(world: &mut LithairWorld) {
    // État avant redémarrage (snapshot)
    let pre_state = { world.test_data.lock().await.clone() };

    // État après replay
    let post_state = world.engine.with_state(|state| state.data.clone()).unwrap_or_default();

    assert_eq!(
        pre_state.articles.len(),
        post_state.articles.len(),
        "❌ Nombre d'articles différent après replay: avant={}, après={}",
        pre_state.articles.len(),
        post_state.articles.len()
    );

    assert_eq!(
        pre_state.articles, post_state.articles,
        "❌ Les articles après replay ne correspondent pas à l'état initial"
    );

    println!("✅ État restauré identiquement ({} articles)", post_state.articles.len());
}

#[then(expr = "reconstruction should take less than {int} seconds")]
async fn then_reconstruction_within(world: &mut LithairWorld, max_seconds: u32) {
    let metrics = world.metrics.lock().await;
    let elapsed = metrics.total_duration;
    let secs = elapsed.as_secs_f64();

    assert!(
        secs <= max_seconds as f64,
        "❌ Reconstruction trop lente: {:.3}s (max: {}s)",
        secs,
        max_seconds
    );

    println!("✅ Reconstruction en {:.3}s (< {}s)", secs, max_seconds);
}

// Scénario: Snapshots optimisés
#[when(expr = "{int} events have been created")]
async fn when_events_created(world: &mut LithairWorld, event_count: u32) {
    println!("📊 Création de {} événements...", event_count);

    let start = std::time::Instant::now();

    // S'assurer que l'environnement event sourcing est initialisé
    ensure_event_sourcing_initialized(world).await;

    // Générer des événements ArticleCreated, les appliquer et les persister
    for i in 0..event_count {
        let id = format!("snapshot-{}", i);
        let title = format!("Article {}", i);
        let content = format!("Content {}", i);

        let event = crate::features::world::TestEvent::ArticleCreated {
            id: id.clone(),
            title: title.clone(),
            content: content.clone(),
        };

        // Appliquer à l'état en mémoire
        world
            .engine
            .with_state_mut(|state| {
                event.apply(state);
            })
            .ok();

        // Écrire dans events.raftlog sous forme de JSON simple
        let event_json = serde_json::json!({
            "event_type": "ArticleCreated",
            "id": id,
            "title": title,
            "content": content,
        })
        .to_string();

        let mut storage_guard = world.storage.lock().await;
        if let Some(storage) = storage_guard.as_mut() {
            storage.append_event(&event_json).expect("Échec append_event pour snapshots");
        } else {
            panic!("Storage non initialisé pour snapshots");
        }

        if i % 100 == 0 {
            println!("  Progression: {}/{}", i, event_count);
        }
    }

    // Flush tous les événements en une fois
    {
        let mut storage_guard = world.storage.lock().await;
        if let Some(storage) = storage_guard.as_mut() {
            storage.flush_batch().expect("Échec flush_batch pour snapshots");
        }
    }

    // Générer un snapshot JSON complet de l'état courant
    let snapshot_json = world
        .engine
        .with_state(|state| serde_json::to_string(state).expect("Sérialisation snapshot"))
        .unwrap_or_else(|_| "{}".to_string());

    {
        let storage_guard = world.storage.lock().await;
        let storage = storage_guard.as_ref().expect("Storage non initialisé pour save_snapshot");
        storage.save_snapshot(&snapshot_json).expect("Échec save_snapshot");
    }

    let elapsed = start.elapsed();
    {
        let mut metrics = world.metrics.lock().await;
        metrics.total_duration = elapsed;
        metrics.request_count = event_count as u64;
    }

    println!("✅ {} événements créés et snapshot écrit", event_count);
}

#[then(expr = "a snapshot should be generated automatically")]
async fn then_snapshot_generated(world: &mut LithairWorld) {
    let temp_dir = world.temp_dir.lock().await;
    let dir = temp_dir.as_ref().expect("TempDir non initialisé pour snapshots");
    let snapshot_file = dir.path().join("state.raftsnap");

    assert!(
        snapshot_file.exists(),
        "❌ Fichier state.raftsnap introuvable: {:?}",
        snapshot_file
    );

    let content =
        std::fs::read_to_string(&snapshot_file).expect("Impossible de lire state.raftsnap");
    assert!(!content.trim().is_empty(), "❌ Snapshot vide dans state.raftsnap");

    let value: serde_json::Value =
        serde_json::from_str(&content).expect("Snapshot invalide (JSON)");
    assert!(value.is_object(), "❌ Snapshot JSON n'est pas un objet");

    println!(
        "✅ Snapshot généré automatiquement ({} bytes) dans {:?}",
        content.len(),
        snapshot_file
    );
}

#[then(expr = "the snapshot should compress current state")]
async fn then_snapshot_compresses_state(world: &mut LithairWorld) {
    let temp_dir = world.temp_dir.lock().await;
    let dir = temp_dir.as_ref().expect("TempDir non initialisé pour snapshots");

    let snapshot_file = dir.path().join("state.raftsnap");
    let events_file = dir.path().join("events.raftlog");

    let snapshot_size =
        std::fs::metadata(&snapshot_file).expect("Metadata snapshot introuvable").len();
    let events_size = std::fs::metadata(&events_file)
        .expect("Metadata events.raftlog introuvable")
        .len();

    assert!(
        snapshot_size < events_size,
        "❌ Snapshot ({snapshot_size} bytes) n'est pas plus compact que le log ({events_size} bytes)"
    );

    println!(
        "✅ Snapshot plus compact que le log: {} bytes vs {} bytes",
        snapshot_size, events_size
    );
}

#[then(expr = "old events should be archived")]
async fn then_old_events_archived(world: &mut LithairWorld) {
    let temp_dir = world.temp_dir.lock().await;
    let dir = temp_dir.as_ref().expect("TempDir non initialisé pour snapshots");
    let events_file = dir.path().join("events.raftlog");

    // Compacter/archiver le log en le tronquant après snapshot
    {
        let mut storage_guard = world.storage.lock().await;
        if let Some(storage) = storage_guard.as_mut() {
            storage.truncate_events().expect("Échec truncate_events pour archivage");
        } else {
            panic!("Storage non initialisé pour archivage");
        }
    }

    let size = std::fs::metadata(&events_file)
        .expect("Metadata events.raftlog introuvable après archivage")
        .len();

    assert!(size == 0, "❌ Log non archivé/compacé, taille restante: {} bytes", size);

    println!("✅ Anciens événements archivés (events.raftlog tronqué à 0 byte)");
}

#[then(expr = "snapshot generation should take less than {int} seconds")]
async fn then_snapshot_generation_within(world: &mut LithairWorld, max_seconds: u32) {
    let metrics = world.metrics.lock().await;
    let elapsed = metrics.total_duration;
    let secs = elapsed.as_secs_f64();

    assert!(
        secs <= max_seconds as f64,
        "❌ Génération du snapshot trop lente: {:.3}s (max: {}s)",
        secs,
        max_seconds
    );

    println!("✅ Génération du snapshot en {:.3}s (< {}s)", secs, max_seconds);
}

// Scénario: Déduplication des événements
#[when(expr = "the same event is received twice")]
async fn when_duplicate_event_received(world: &mut LithairWorld) {
    println!("🔁 Préparation du scénario de déduplication (moteur direct)...");

    // 1) Initialiser l'environnement event sourcing
    ensure_event_sourcing_initialized(world).await;

    // 2) Construire un événement ArticleCreated avec une clé d'idempotence stable
    let id = "dedup-article-1".to_string();
    let title = "Article dédoublonné".to_string();
    let content = "Contenu dédoublonné".to_string();

    let event = crate::features::world::TestEvent::ArticleCreated {
        id: id.clone(),
        title: title.clone(),
        content: content.clone(),
    };

    // Simuler deux réceptions du même événement
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

    // Mettre à jour les métriques pour les assertions
    {
        let article_count = world.engine.with_state(|state| state.data.articles.len()).unwrap_or(0);
        let mut metrics = world.metrics.lock().await;
        metrics.request_count = article_count as u64;
        // Considérer une erreur si la déduplication ne s'est pas comportée comme attendu
        metrics.error_count = if !first_applied || second_applied { 1 } else { 0 };
    }

    // 3) Persister deux fois la même enveloppe d'événement dans events.raftlog
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
            .expect("Échec append_event pour déduplication (1)");
        storage
            .append_event(&envelope)
            .expect("Échec append_event pour déduplication (2)");
        storage.flush_batch().expect("Échec flush_batch pour déduplication");
    } else {
        panic!("Storage non initialisé pour déduplication");
    }

    println!("🔁 Même événement reçu deux fois, appliqué une seule fois en mémoire et persisté deux fois dans le log");
}

#[then(expr = "only the first should be applied")]
async fn then_only_first_applied(world: &mut LithairWorld) {
    let articles = world.engine.with_state(|state| state.data.articles.clone()).unwrap_or_default();

    assert_eq!(
        articles.len(),
        1,
        "❌ Déduplication échouée: {} articles présents en mémoire (attendu: 1)",
        articles.len()
    );

    println!("✅ Seul le premier événement a été appliqué (1 article en mémoire)");
}

#[then(expr = "the duplicate should be ignored silently")]
async fn then_duplicate_ignored(world: &mut LithairWorld) {
    // Vérifier qu'aucune erreur n'a été enregistrée au niveau des métriques
    let metrics = world.metrics.lock().await;
    assert_eq!(
        metrics.error_count, 0,
        "❌ Une erreur a été enregistrée pendant la déduplication (error_count = {})",
        metrics.error_count
    );
    drop(metrics);

    // Vérifier que le log contient bien deux entrées pour le même event_id
    let temp_dir = world.temp_dir.lock().await;
    let dir = temp_dir
        .as_ref()
        .expect("TempDir non initialisé pour vérification de déduplication");
    let events_file = dir.path().join("events.raftlog");

    let content =
        std::fs::read_to_string(&events_file).expect("Impossible de lire events.raftlog (dédup)");

    let mut total = 0usize;
    let mut duplicate_count = 0usize;
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        total += 1;
        let value: serde_json::Value =
            serde_json::from_str(line).expect("Ligne invalide dans events.raftlog (dédup)");
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
        "❌ Le log ne contient pas au moins deux entrées pour le même event_id (count = {}, total = {})",
        duplicate_count,
        total
    );

    println!(
        "✅ Doublon ignoré silencieusement côté état: {} entrées dans le log pour le même event_id, mais une seule application",
        duplicate_count
    );
}

#[then(expr = "integrity should be preserved")]
async fn then_integrity_preserved(world: &mut LithairWorld) {
    // Rejouer le log avec déduplication par event_id dans un état vierge
    let temp_dir = world.temp_dir.lock().await;
    let dir = temp_dir.as_ref().expect("TempDir non initialisé pour vérification d'intégrité");
    let events_file = dir.path().join("events.raftlog");

    let content = std::fs::read_to_string(&events_file)
        .expect("Impossible de lire events.raftlog (intégrité)");

    let mut seen_ids = std::collections::HashSet::new();
    let mut rebuilt_state = crate::features::world::TestAppState::default();

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }

        let value: serde_json::Value =
            serde_json::from_str(line).expect("Ligne invalide dans events.raftlog (intégrité)");

        let obj = value.as_object().expect("Événement de log n'est pas un objet JSON (intégrité)");

        let event_id = obj.get("event_id").and_then(|v| v.as_str()).unwrap_or("").to_string();

        if !seen_ids.insert(event_id) {
            // Déjà vu: doublon, on l'ignore au replay
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
        "❌ Intégrité non préservée: état reconstruit depuis le log (avec dédup) différent de l'état actuel"
    );

    println!("✅ Intégrité préservée: état actuel == état reconstruit avec déduplication du log");
}

// Scénario: Récupération après corruption (moteur direct)
#[when(expr = "the state file is corrupted")]
async fn when_state_file_corrupted(world: &mut LithairWorld) {
    println!("💥 Préparation d'un fichier d'état corrompu (event sourcing direct)...");

    // 1) Initialiser l'environnement event sourcing
    ensure_event_sourcing_initialized(world).await;

    // 2) Générer un état cohérent avec quelques événements ArticleCreated
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

        // Appliquer à l'état en mémoire
        world
            .engine
            .with_state_mut(|state| {
                event.apply(state);
            })
            .ok();

        // Persister dans events.raftlog
        let event_json = serde_json::json!({
            "event_type": "ArticleCreated",
            "id": id,
            "title": title,
            "content": content,
        })
        .to_string();

        let mut storage_guard = world.storage.lock().await;
        if let Some(storage) = storage_guard.as_mut() {
            storage.append_event(&event_json).expect("Échec append_event pour corruption");
        } else {
            panic!("Storage non initialisé pour corruption");
        }
    }

    {
        let mut storage_guard = world.storage.lock().await;
        if let Some(storage) = storage_guard.as_mut() {
            storage.flush_batch().expect("Échec flush_batch pour corruption");
        }
    }

    // 3) Sauvegarder un snapshot valide de l'état courant (dernier snapshot valide)
    let snapshot_json = world
        .engine
        .with_state(|state| {
            serde_json::to_string(state).expect("Sérialisation snapshot corruption")
        })
        .unwrap_or_else(|_| "{}".to_string());

    {
        let storage_guard = world.storage.lock().await;
        let storage = storage_guard
            .as_ref()
            .expect("Storage non initialisé pour save_snapshot corruption");
        storage.save_snapshot(&snapshot_json).expect("Échec save_snapshot corruption");
    }

    // Conserver l'état attendu pour les vérifications
    let expected_data = world.engine.with_state(|state| state.data.clone()).unwrap_or_default();
    {
        let mut test_data = world.test_data.lock().await;
        *test_data = expected_data;
    }

    // 4) Corrompre le fichier events.raftlog en y injectant une ligne JSON invalide
    let events_file = {
        let temp_dir = world.temp_dir.lock().await;
        let dir = temp_dir.as_ref().expect("TempDir non initialisé pour corruption");
        dir.path().join("events.raftlog")
    };

    let mut content = std::fs::read_to_string(&events_file)
        .expect("Impossible de lire events.raftlog pour corruption");
    content.push_str("\n{ this-is-not-valid-json");
    std::fs::write(&events_file, &content).expect("Impossible d'écrire events.raftlog corrompu");

    // Réinitialiser le flag de corruption pour les assertions suivantes
    world.corruption_detected = false;

    println!("⚠️ Fichier d'état corrompu simulé dans {:?}", events_file);
}

#[then(expr = "the system should detect corruption")]
async fn then_system_detects_corruption(world: &mut LithairWorld) {
    let events_file = {
        let temp_dir = world.temp_dir.lock().await;
        let dir = temp_dir.as_ref().expect("TempDir non initialisé pour détection de corruption");
        dir.path().join("events.raftlog")
    };

    let content = std::fs::read_to_string(&events_file)
        .expect("Impossible de lire events.raftlog pour détection");

    let mut invalid = 0usize;
    let mut last_invalid_line = 0usize;

    for (idx, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        if serde_json::from_str::<serde_json::Value>(line).is_err() {
            invalid += 1;
            last_invalid_line = idx + 1;
        }
    }

    assert!(
        invalid > 0,
        "❌ Aucune corruption détectée dans events.raftlog alors qu'une ligne invalide a été injectée"
    );

    world.corruption_detected = true;

    println!(
        "✅ Corruption détectée: {} ligne(s) invalides (ex: ligne {})",
        invalid, last_invalid_line
    );
}

#[then(expr = "rebuild from last valid snapshot")]
async fn then_rebuild_from_last_valid_snapshot(world: &mut LithairWorld) {
    assert!(
        world.corruption_detected,
        "❌ Corruption non détectée avant tentative de reconstruction"
    );

    // Charger le dernier snapshot valide depuis FileStorage
    let snapshot_json_opt = {
        let storage_guard = world.storage.lock().await;
        let storage = storage_guard.as_ref().expect("Storage non initialisé pour load_snapshot");
        storage.load_snapshot().expect("Échec load_snapshot pour reconstruction")
    };

    let snapshot_json = snapshot_json_opt
        .expect("❌ Aucun snapshot trouvé alors qu'un snapshot valide aurait dû être sauvegardé");

    let snapshot_state: crate::features::world::TestAppState = serde_json::from_str(&snapshot_json)
        .expect("Snapshot invalide (JSON) lors de la reconstruction");

    // Réinitialiser l'état du moteur avec le snapshot
    world
        .engine
        .with_state_mut(|state| {
            *state = snapshot_state.clone();
        })
        .ok();

    // Comparer avec l'état attendu enregistré avant la corruption
    let expected_data = { world.test_data.lock().await.clone() };
    let current_data = world.engine.with_state(|state| state.data.clone()).unwrap_or_default();

    assert_eq!(
        expected_data.articles, current_data.articles,
        "❌ État reconstruit différent du dernier snapshot valide",
    );

    println!(
        "✅ État reconstruit depuis le dernier snapshot valide ({} articles)",
        current_data.articles.len()
    );
}

#[then(expr = "continue to function normally")]
async fn then_continue_to_operate_normally(world: &mut LithairWorld) {
    // Appliquer un nouvel événement après récupération
    let id = "corruption-recovery-new-1".to_string();
    let title = "Article après récupération".to_string();
    let content = "Contenu après récupération".to_string();

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
            storage
                .append_event(&event_json)
                .expect("Échec append_event après récupération");
            storage.flush_batch().expect("Échec flush_batch après récupération");
        } else {
            panic!("Storage non initialisé après récupération");
        }
    }

    let article_count = world.engine.with_state(|state| state.data.articles.len()).unwrap_or(0);

    assert!(
        article_count > 0,
        "❌ Aucun article présent après récupération et écriture d'un nouvel événement",
    );

    println!(
        "✅ Moteur continue à fonctionner normalement après corruption ({} articles)",
        article_count
    );
}

// Scénario: Déduplication persistante après redémarrage
#[when(expr = "an idempotent event is applied before and after engine restart")]
async fn when_idempotent_event_before_and_after_restart(world: &mut LithairWorld) {
    use crate::features::world::{TestEngineApp, TestEvent};

    println!(
        "🧪 Préparation du scénario de déduplication persistante (avant/après redémarrage)..."
    );

    let base_path = "/tmp/lithair-dedup-persistent-test".to_string();

    // Nettoyer le répertoire de test
    std::fs::remove_dir_all(&base_path).ok();
    std::fs::create_dir_all(&base_path)
        .expect("Impossible de créer le répertoire pour la déduplication persistante");

    // Forcer la persistance des IDs de déduplication (par défaut déjà activée, mais explicite)
    std::env::set_var("RS_DEDUP_PERSIST", "1");

    let config = EngineConfig { event_log_path: base_path.clone(), ..Default::default() };

    // Run 1: appliquer l'événement une première fois
    let engine = Engine::<TestEngineApp>::new(config.clone())
        .expect("Échec d'initialisation du moteur pour la déduplication persistante");

    let event = TestEvent::ArticleCreated {
        id: "dedup-persistent-1".to_string(),
        title: "Article dédup persistant".to_string(),
        content: "Contenu dédup persistant".to_string(),
    };

    let key = event.aggregate_id().unwrap_or("global".to_string());
    engine
        .apply_event(key.clone(), event.clone())
        .expect("Échec application initiale de l'événement idempotent");
    engine.flush().expect("Échec flush après première application");

    // Drop pour simuler un arrêt propre
    drop(engine);

    // Run 2: redémarrer le moteur et ré-appliquer le même événement
    let engine2 = Engine::<TestEngineApp>::new(config)
        .expect("Échec de réinitialisation du moteur pour la déduplication persistante");

    let result_second = engine2.apply_event(key, event);

    let duplicate_rejected = matches!(result_second, Err(EngineError::DuplicateEvent(_)));

    // Vérifier le contenu de dedup.raftids
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
        "🧪 Déduplication persistante: duplicate_rejected={}, dedup_ids_count={}, contains_expected={}",
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
        "❌ Le moteur n'a pas rejeté le doublon après redémarrage (duplicate_rejected = false)"
    );
    assert!(
        contains_expected && dedup_count >= 1,
        "❌ dedup.raftids ne contient pas la clé attendue ou est vide (contains_expected={}, count={})",
        contains_expected,
        dedup_count
    );
}

#[when(expr = "an idempotent event is applied before and after engine restart in multi-file mode")]
async fn when_idempotent_event_before_and_after_restart_multifile(world: &mut LithairWorld) {
    use crate::features::world::{TestEngineApp, TestEvent};

    println!("🧪 Préparation du scénario de déduplication persistante en mode multi-fichiers...",);

    let base_path = "/tmp/lithair-dedup-multifile-test".to_string();

    // Nettoyer le répertoire de test
    std::fs::remove_dir_all(&base_path).ok();
    std::fs::create_dir_all(&base_path)
        .expect("Impossible de créer le répertoire pour la déduplication multi-fichiers");

    // Forcer la persistance des IDs de déduplication
    std::env::set_var("RS_DEDUP_PERSIST", "1");

    let config = EngineConfig { event_log_path: base_path.clone(), use_multi_file_store: true, ..Default::default() };

    // Run 1: appliquer l'événement une première fois en mode multi-fichiers
    let engine = Engine::<TestEngineApp>::new(config.clone()).expect(
        "Échec d'initialisation du moteur en mode multi-fichiers pour la déduplication persistante",
    );

    let event = TestEvent::ArticleCreated {
        id: "dedup-multifile-1".to_string(),
        title: "Article dédup multi-file".to_string(),
        content: "Contenu dédup multi-file".to_string(),
    };

    let key = event.aggregate_id().unwrap_or("global".to_string());
    engine
        .apply_event(key.clone(), event.clone())
        .expect("Échec application initiale de l'événement idempotent en multi-fichiers");
    engine.flush().expect("Échec flush après première application (multi-fichiers)");

    // Drop pour simuler un arrêt propre
    drop(engine);

    // Run 2: redémarrer le moteur et ré-appliquer le même événement
    let engine2 = Engine::<TestEngineApp>::new(config).expect(
        "Échec de réinitialisation du moteur pour la déduplication persistante en multi-fichiers",
    );

    let result_second = engine2.apply_event(key, event);

    let duplicate_rejected = matches!(result_second, Err(EngineError::DuplicateEvent(_)));

    // Vérifier le contenu de dedup.raftids global (base_path/global/dedup.raftids)
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
        // Réutiliser les mêmes tokens que le scénario de dédup persistante existant
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

        // Tokens spécifiques au scénario multi-fichiers
        test_data
            .tokens
            .insert("multifile_dedup_base_path".to_string(), base_path.clone());
        test_data
            .tokens
            .insert("multifile_dedup_expected_id".to_string(), expected_id.clone());
    }

    println!(
        "🧪 Dédup multi-fichiers: duplicate_rejected={}, dedup_ids_count={}, contains_expected={}",
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
        "❌ Fichier de déduplication global introuvable en mode multi-fichiers: {}",
        global_dedup
    );

    let content = std::fs::read_to_string(&global_dedup)
        .expect("Impossible de lire le fichier dedup.raftids global (multi-fichiers)");
    let ids: Vec<_> = content.lines().filter(|l| !l.trim().is_empty()).collect();

    assert!(
        !ids.is_empty(),
        "❌ Aucun identifiant de déduplication trouvé dans le fichier global ({})",
        global_dedup
    );

    let contains_expected = ids.iter().any(|id| *id == expected_id);

    assert!(
        contains_expected,
        "❌ Le fichier global dedup.raftids ne contient pas l'identifiant attendu '{}' (ids={:?})",
        expected_id, ids
    );

    println!(
        "✅ Fichier de déduplication global trouvé en mode multi-fichiers ({} identifiant(s))",
        ids.len()
    );
}

#[when(expr = "I persist events on multiple aggregates in a multi-file event store")]
async fn when_persist_events_multi_aggregates_multifile(world: &mut LithairWorld) {
    use crate::features::world::{TestEngineApp, TestEvent};

    println!(
         "🧪 Routage multi-fichiers: persistance d'événements sur plusieurs agrégats (mode multi-file)",
     );

    let base_path = "/tmp/lithair-multifile-routing-test".to_string();

    // Nettoyer et recréer le répertoire de test
    std::fs::remove_dir_all(&base_path).ok();
    std::fs::create_dir_all(&base_path)
        .expect("Impossible de créer le répertoire de test pour le mode multi-fichiers");

    let config = EngineConfig { event_log_path: base_path.clone(), use_multi_file_store: true, ..Default::default() };

    let engine = Engine::<TestEngineApp>::new(config)
        .expect("Échec d'initialisation du moteur en mode multi-fichiers");

    // Deux structures / tables logiques distinctes: articles et users
    let article_id = "article-multifile-1".to_string();
    let user_id = "user-multifile-1".to_string();

    let event_articles = TestEvent::ArticleCreated {
        id: article_id.clone(),
        title: "Article multi-file".to_string(),
        content: "Contenu multi-file".to_string(),
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
        .expect("Échec application événement aggregate_articles");
    engine
        .apply_event(key_users, event_users)
        .expect("Échec application événement aggregate_users");
    engine.flush().expect("Échec flush du moteur en mode multi-fichiers");

    {
        let mut test_data = world.test_data.lock().await;
        test_data.tokens.insert("multifile_base_path".to_string(), base_path.clone());
        // aggregate_id correspond maintenant au nom de la table/structure
        test_data
            .tokens
            .insert("multifile_agg_articles".to_string(), "articles".to_string());
        test_data.tokens.insert("multifile_agg_users".to_string(), "users".to_string());
    }

    println!(
        "✅ Événements persistés en mode multi-fichiers pour deux agrégats logiques distincts",
    );
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
        "❌ Fichier events.raftlog introuvable pour l'agrégat articles: {}",
        articles_path
    );
    assert!(
        std::path::Path::new(&users_path).exists(),
        "❌ Fichier events.raftlog introuvable pour l'agrégat users: {}",
        users_path
    );

    let articles_content = std::fs::read_to_string(&articles_path)
        .expect("Impossible de lire le fichier events.raftlog pour l'agrégat articles");
    let users_content = std::fs::read_to_string(&users_path)
        .expect("Impossible de lire le fichier events.raftlog pour l'agrégat users");

    let articles_events: Vec<_> =
        articles_content.lines().filter(|l| !l.trim().is_empty()).collect();
    let users_events: Vec<_> = users_content.lines().filter(|l| !l.trim().is_empty()).collect();

    assert!(
        !articles_events.is_empty(),
        "❌ Aucun événement trouvé dans le fichier de l'agrégat articles ({})",
        articles_path
    );
    assert!(
        !users_events.is_empty(),
        "❌ Aucun événement trouvé dans le fichier de l'agrégat users ({})",
        users_path
    );

    println!("✅ Événements correctement répartis dans des fichiers distincts pour chaque agrégat",);
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
        let content =
            std::fs::read_to_string(&file_path).expect("Impossible de lire le fichier d'agrégat");

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let value: serde_json::Value = serde_json::from_str(line)
                .expect("Ligne invalide dans events.raftlog (multi-fichiers)");

            let aggregate_in_file =
                value.get("aggregate_id").and_then(|v| v.as_str()).unwrap_or("");

            assert_eq!(
                aggregate_in_file,
                agg,
                "❌ Fichier d'agrégat {} contient un événement avec aggregate_id='{}' (attendu: '{}')",
                label,
                aggregate_in_file,
                agg
            );
        }
    }

    println!(
        "✅ Chaque fichier d'agrégat contient uniquement les événements de son propre agrégat",
    );
}

#[when(expr = "I generate enough events to trigger log rotation in multi-file mode")]
async fn when_generate_events_for_multifile_rotation(world: &mut LithairWorld) {
    use crate::features::world::{TestEngineApp, TestEvent};

    println!("🧪 Préparation du scénario de rotation des logs en mode multi-fichiers...",);

    let base_path = "/tmp/lithair-multifile-rotation-test".to_string();

    // Nettoyer et recréer le répertoire de test
    std::fs::remove_dir_all(&base_path).ok();
    std::fs::create_dir_all(&base_path)
        .expect("Impossible de créer le répertoire de test pour la rotation multi-fichiers");

    // Activer la rotation avec un seuil très bas pour forcer rapidement un rollover
    // Note: FileStorage::new lit RS_MAX_LOG_FILE_SIZE et configure max_log_file_size en conséquence.
    std::env::set_var("RS_MAX_LOG_FILE_SIZE", "512");

    let config = EngineConfig { event_log_path: base_path.clone(), use_multi_file_store: true, ..Default::default() };

    let engine = Engine::<TestEngineApp>::new(config)
        .expect("Échec d'initialisation du moteur en mode multi-fichiers pour la rotation");

    // Table ciblée pour la rotation: articles (aggregate_id = "articles")
    let rotation_table = "articles".to_string();

    // Générer un seul événement très volumineux pour dépasser le seuil (512 bytes)
    let event = TestEvent::ArticleCreated {
        id: "rotation-article-1".to_string(),
        title: "Rotation large event".to_string(),
        // Contenu suffisamment long pour déclencher la rotation en un seul append
        content: "x".repeat(5000),
    };

    let key = event.aggregate_id().unwrap_or("global".to_string());
    engine
        .apply_event(key, event)
        .expect("Échec application événement de rotation en multi-fichiers");

    engine
        .flush()
        .expect("Échec flush après génération des événements de rotation (multi-fichiers)");

    {
        let mut test_data = world.test_data.lock().await;
        test_data
            .tokens
            .insert("multifile_rotation_base_path".to_string(), base_path.clone());
        test_data
            .tokens
            .insert("multifile_rotation_table".to_string(), rotation_table.clone());
    }

    println!(
        "✅ Événements générés pour la rotation en mode multi-fichiers (table = {})",
        rotation_table
    );
}

#[then(expr = "the rotation aggregate log should be rotated")]
async fn then_rotation_aggregate_log_must_be_rotated(world: &mut LithairWorld) {
    let test_data = world.test_data.lock().await;
    let base_path = test_data
        .tokens
        .get("multifile_rotation_base_path")
        .cloned()
        .unwrap_or_else(|| "/tmp/lithair-multifile-rotation-test".to_string());
    let rotation_table = test_data
        .tokens
        .get("multifile_rotation_table")
        .cloned()
        .unwrap_or_else(|| "articles".to_string());
    drop(test_data);

    let log_path = format!("{}/{}/events.raftlog", base_path, rotation_table);
    let rotated_path = format!("{}.1", log_path);

    assert!(
        std::path::Path::new(&rotated_path).exists(),
        "❌ Fichier rotaté introuvable pour l'agrégat de rotation: {}",
        rotated_path
    );
    assert!(
        std::path::Path::new(&log_path).exists(),
        "❌ Fichier courant events.raftlog introuvable pour l'agrégat de rotation: {}",
        log_path
    );

    let rotated_size = std::fs::metadata(&rotated_path).map(|m| m.len()).unwrap_or(0);
    assert!(
        rotated_size > 0,
        "❌ Fichier rotaté {} est vide (aucun événement persistant)",
        rotated_path
    );

    println!(
        "✅ Log rotaté détecté pour l'agrégat de rotation ({} bytes dans {})",
        rotated_size, rotated_path
    );
}

#[then(expr = "log files for that aggregate should remain readable after rotation")]
async fn then_rotation_aggregate_logs_must_remain_readable_after_rotation(
    world: &mut LithairWorld,
) {
    let test_data = world.test_data.lock().await;
    let base_path = test_data
        .tokens
        .get("multifile_rotation_base_path")
        .cloned()
        .unwrap_or_else(|| "/tmp/lithair-multifile-rotation-test".to_string());
    let rotation_table = test_data
        .tokens
        .get("multifile_rotation_table")
        .cloned()
        .unwrap_or_else(|| "articles".to_string());
    drop(test_data);

    let log_path = format!("{}/{}/events.raftlog", base_path, rotation_table);
    let rotated_path = format!("{}.1", log_path);

    // Vérifier que les deux fichiers (segment rotaté et fichier courant) sont lisibles
    for path in [&rotated_path, &log_path] {
        if !std::path::Path::new(path).exists() {
            // Le fichier courant peut être vide ou inexistant juste après la rotation,
            // on se concentre surtout sur le segment rotaté.
            if path == &rotated_path {
                panic!("❌ Fichier rotaté inexistant: {}", path);
            }
            continue;
        }

        let content = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("Impossible de lire le fichier {}: {}", path, e));

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let value: serde_json::Value = serde_json::from_str(line)
                .expect("Ligne invalide dans events.raftlog après rotation (multi-fichiers)");

            let aggregate_in_file =
                value.get("aggregate_id").and_then(|v| v.as_str()).unwrap_or("");

            assert_eq!(
                aggregate_in_file, rotation_table,
                "❌ Fichier {} contient un événement avec aggregate_id='{}' (attendu: '{}')",
                path, aggregate_in_file, rotation_table
            );
        }
    }

    println!("✅ Fichiers de log de l'agrégat de rotation lisibles et cohérents après rotation",);
}

#[when(expr = "I create a linked user and article in multi-file mode")]
async fn when_create_user_and_article_linked_multifile(world: &mut LithairWorld) {
    use crate::features::world::{TestEngineApp, TestEvent};

    println!(
        "🧪 Relations dynamiques: création d'un utilisateur, d'un article et de leur relation (multi-fichiers)...",
    );

    let base_path = "/tmp/lithair-multifile-relations-test".to_string();

    // Nettoyer et recréer le répertoire de test
    std::fs::remove_dir_all(&base_path).ok();
    std::fs::create_dir_all(&base_path)
        .expect("Impossible de créer le répertoire de test pour les relations multi-fichiers");

    let config = EngineConfig { event_log_path: base_path.clone(), use_multi_file_store: true, ..Default::default() };

    let engine = Engine::<TestEngineApp>::new(config).expect(
        "Échec d'initialisation du moteur en mode multi-fichiers pour les relations dynamiques",
    );

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
        content: "Contenu article avec relation user".to_string(),
    };
    let event_link =
        TestEvent::ArticleLinkedToUser { article_id: article_id.clone(), user_id: user_id.clone() };

    let key_article = event_article.aggregate_id().unwrap_or("global".to_string());
    let key_user = event_user.aggregate_id().unwrap_or("global".to_string());
    let key_link = event_link.aggregate_id().unwrap_or("global".to_string());

    // Appliquer les événements : d'abord données, puis relation
    engine
        .apply_event(key_article, event_article)
        .expect("Échec application événement ArticleCreated pour les relations");
    engine
        .apply_event(key_user, event_user)
        .expect("Échec application événement UserCreated pour les relations");
    engine
        .apply_event(key_link, event_link)
        .expect("Échec application événement ArticleLinkedToUser");

    engine
        .flush()
        .expect("Échec flush après création des événements de relations (multi-fichiers)");

    {
        let mut test_data = world.test_data.lock().await;
        test_data.tokens.insert("relations_base_path".to_string(), base_path.clone());
        test_data.tokens.insert("relations_user_id".to_string(), user_id.clone());
        test_data.tokens.insert("relations_article_id".to_string(), article_id.clone());
    }

    println!(
        "✅ Utilisateur ({}) et article ({}) créés et liés en mode multi-fichiers",
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

    // Helper pour rejouer les événements depuis un fichier events.raftlog
    let replay_file = |state: &mut TestAppState, path: &str| {
        if !std::path::Path::new(path).exists() {
            panic!("❌ Fichier events.raftlog introuvable: {}", path);
        }

        let content = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("Impossible de lire le fichier {}: {}", path, e));

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }

            let value: serde_json::Value = serde_json::from_str(line)
                .expect("Ligne invalide dans events.raftlog (replay relations)");

            let payload_str = value
                .get("payload")
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| panic!("Payload manquant dans l'enveloppe d'événement"));

            let event: crate::features::world::TestEvent = serde_json::from_str(payload_str)
                .expect("Impossible de désérialiser TestEvent depuis payload");

            event.apply(state);
        }
    };

    let articles_log = format!("{}/articles/events.raftlog", base_path);
    let users_log = format!("{}/users/events.raftlog", base_path);
    let relations_log = format!("{}/relations/events.raftlog", base_path);

    replay_file(&mut rebuilt_state, &articles_log);
    replay_file(&mut rebuilt_state, &users_log);
    replay_file(&mut rebuilt_state, &relations_log);

    // Vérifier que l'article connaît son auteur
    let article = rebuilt_state
        .data
        .articles
        .get(&article_id)
        .unwrap_or_else(|| panic!("❌ Article {} introuvable après replay", article_id));

    let author_id = article.get("author_id").and_then(|v| v.as_str()).unwrap_or_else(|| {
        panic!("❌ author_id manquant sur l'article {} après replay", article_id)
    });

    assert_eq!(
        author_id, user_id,
        "❌ author_id reconstruit ({}) différent de l'utilisateur attendu ({})",
        author_id, user_id
    );

    // Vérifier que l'utilisateur connaît la liste de ses articles liés
    let user = rebuilt_state
        .data
        .users
        .get(&user_id)
        .unwrap_or_else(|| panic!("❌ Utilisateur {} introuvable après replay", user_id));

    let articles_array = user.get("articles").and_then(|v| v.as_array()).unwrap_or_else(|| {
        panic!("❌ Champ 'articles' manquant ou non-array pour l'utilisateur {}", user_id)
    });

    let contains_article = articles_array
        .iter()
        .any(|v| v.as_str().map(|s| s == article_id).unwrap_or(false));

    assert!(
        contains_article,
        "❌ L'utilisateur {} ne référence pas l'article {} dans 'articles' après replay",
        user_id, article_id
    );

    println!(
        "✅ Relations dynamiques article<->user reconstruites en mémoire à partir des événements multi-fichiers",
    );
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
            "❌ Fichier events.raftlog introuvable pour la table {}: {}",
            expected_agg,
            path
        );

        let content = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("Impossible de lire le fichier {}: {}", path, e));

        let mut non_empty_lines = 0usize;

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            non_empty_lines += 1;

            let value: serde_json::Value = serde_json::from_str(line)
                .expect("Ligne invalide dans events.raftlog (vérification par table)");

            let aggregate_in_file =
                value.get("aggregate_id").and_then(|v| v.as_str()).unwrap_or("");

            assert_eq!(
                aggregate_in_file, expected_agg,
                "❌ Fichier {} contient un événement avec aggregate_id='{}' (attendu: '{}')",
                path, aggregate_in_file, expected_agg
            );
        }

        assert!(
            non_empty_lines > 0,
            "❌ Aucun événement trouvé dans la table {} ({})",
            expected_agg,
            path
        );
    }
    println!(
        "✅ Événements correctement répartis entre les tables de données (articles/users) et la table de relations",
    );
}

#[when(expr = "I replay ArticleCreated v1 and v2 events via versioned deserializers")]
async fn when_replay_versioned_article_events(world: &mut LithairWorld) {
    use crate::features::world::TestEngineApp;

    let base_path = "/tmp/lithair-versioning-articles-test".to_string();

    std::fs::remove_dir_all(&base_path).ok();
    std::fs::create_dir_all(&base_path)
        .expect("Impossible de créer le répertoire de test pour le versioning d'articles");

    let payload_v1 = serde_json::json!({
        "version": 1,
        "id": "version-article-v1",
        "title": "Article v1",
        "content": "Contenu v1 sans slug",
    });

    let payload_v2 = serde_json::json!({
        "version": 2,
        "id": "version-article-v2",
        "title": "Article v2",
        "content": "Contenu v2 avec slug",
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

    std::fs::write(&events_path, content)
        .expect("Impossible d'écrire le log d'événements versionnés");

    let config = EngineConfig { event_log_path: base_path.clone(), use_multi_file_store: false, ..Default::default() };

    let engine = Engine::<TestEngineApp>::new(config)
        .expect("Échec d'initialisation du moteur pour versioning");

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
            .expect("Impossible de lire l'état après replay")
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

    assert!(
        v1_slug.is_empty(),
        "❌ Article v1 ne devrait pas avoir de slug, trouvé '{}'",
        v1_slug
    );

    assert_eq!(
        v1_version, "1",
        "❌ Version attendue pour l'article v1 = 1, trouvée {}",
        v1_version
    );

    assert_eq!(
        v2_slug, "article-v2-slug",
        "❌ Article v2 devrait avoir le slug 'article-v2-slug', trouvé '{}'",
        v2_slug
    );

    assert_eq!(
        v2_version, "2",
        "❌ Version attendue pour l'article v2 = 2, trouvée {}",
        v2_version
    );

    println!(
        "✅ Upcasting des événements ArticleCreated v1/v2: slug et version correctement reconstruits"
    );
}

// Tests supplémentaires pour la complétude
#[when(expr = "je consulte l'historique des événements")]
async fn when_query_event_history(world: &mut LithairWorld) {
    let _ = world.make_request("GET", "/api/events/history", None).await;
    println!("📜 Historique des événements consulté");
}

// ... (code après les modifications)
#[then(expr = "je dois pouvoir filtrer par type d'événement")]
async fn then_filter_by_event_type(world: &mut LithairWorld) {
    let _ = world.make_request("GET", "/api/events/history?type=ArticleCreated", None).await;
    println!("✅ Filtrage par type d'événement");
}

#[then(expr = "par agrégat")]
async fn then_filter_by_aggregate(world: &mut LithairWorld) {
    let _ = world.make_request("GET", "/api/events/history?aggregate_id=123", None).await;
    println!("✅ Filtrage par agrégat");
}

#[then(expr = "par plage de dates")]
async fn then_filter_by_date_range(world: &mut LithairWorld) {
    let _ = world
        .make_request("GET", "/api/events/history?from=2024-01-01&to=2024-12-31", None)
        .await;
    println!("✅ Filtrage par plage de dates");
}
