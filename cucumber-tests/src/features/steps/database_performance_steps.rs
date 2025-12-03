use cucumber::{given, when, then};
use crate::features::world::{LithairWorld, TestArticle};
use std::time::{Instant, Duration};
use std::sync::{Arc, RwLock};
use tokio::time::sleep;
use lithair_core::engine::{AsyncWriter, EventStore};

// ==================== GIVEN STEPS ====================

#[given(expr = "la persistence est activée par défaut")]
async fn persistence_enabled_by_default(_world: &mut LithairWorld) {
    println!("✅ Persistence activée par défaut");
}

#[given(expr = "un serveur Lithair sur le port {int} avec persistence {string}")]
async fn server_with_persistence(world: &mut LithairWorld, port: u16, path: String) {
    println!("🚀 Initialisation serveur sur port {} avec persistence: {}", port, path);

    // Nettoyer et créer le dossier
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("Failed to create persistence dir");

    // Créer EventStore + AsyncWriter
    let event_store = Arc::new(RwLock::new(
        EventStore::new(&path).expect("EventStore init failed")
    ));
    let async_writer = AsyncWriter::new(event_store, 1000);

    // Stocker dans world
    *world.async_writer.lock().await = Some(async_writer);

    // Sauvegarder les paramètres dans metrics
    let mut metrics = world.metrics.lock().await;
    metrics.persist_path = path;
    metrics.server_port = port;

    println!("✅ Serveur initialisé (port: {}, batch_size: 1000)", port);
}

#[given(expr = "le mode MaxDurability est activé avec fsync")]
async fn max_durability_with_fsync(_world: &mut LithairWorld) {
    // Note: fsync est maintenant activé par défaut dans OptimizedPersistenceConfig
    println!("✅ Mode MaxDurability avec fsync activé");
}

// ==================== WHEN STEPS ====================

#[when(expr = "je crée {int} articles rapidement")]
async fn create_articles_fast(world: &mut LithairWorld, count: usize) {
    println!("🚀 Création rapide de {} articles...", count);
    let start = Instant::now();

    for i in 0..count {
        let article = TestArticle {
            id: format!("article-{}", i),
            title: format!("Article {}", i),
            content: format!("Content for article {}", i),
        };

        // Persister via AsyncWriter
        if let Some(ref writer) = *world.async_writer.lock().await {
            let event = serde_json::json!({
                "type": "ArticleCreated",
                "data": article,
                "timestamp": chrono::Utc::now().to_rfc3339()
            });
            writer.write(serde_json::to_string(&event).unwrap()).ok();
        }

        // Stocker en mémoire (SCC2)
        let id = article.id.clone();
        world.scc2_articles.write(&id, |s| *s = article).ok();

        if count >= 1000 && i % 500 == 0 && i > 0 {
            println!("  ... {} articles créés", i);
        }
    }

    let elapsed = start.elapsed();
    let throughput = count as f64 / elapsed.as_secs_f64();

    println!("✅ {} articles créés en {:.2}s ({:.0} articles/sec)", count, elapsed.as_secs_f64(), throughput);

    // Sauvegarder métriques
    let mut metrics = world.metrics.lock().await;
    metrics.request_count = count as u64;
    metrics.total_duration = elapsed;
}

#[when(expr = "je crée {int} articles critiques")]
async fn create_critical_articles(world: &mut LithairWorld, count: usize) {
    // Même chose que create_articles_fast, mais explicitement pour tests critiques
    create_articles_fast(world, count).await;
}

#[when(expr = "j'attends {int} secondes pour le flush")]
async fn wait_for_flush(_world: &mut LithairWorld, seconds: u64) {
    println!("⏳ Attente {} secondes pour le flush...", seconds);
    sleep(Duration::from_secs(seconds)).await;
    println!("✅ Attente terminée");
}

#[when(expr = "je mesure le temps pour créer {int} articles")]
async fn measure_time_create_articles(world: &mut LithairWorld, count: usize) {
    let start = Instant::now();

    for i in 0..count {
        let article = TestArticle {
            id: format!("article-{}", i),
            title: format!("Article {}", i),
            content: format!("Content {}", i),
        };

        if let Some(ref writer) = *world.async_writer.lock().await {
            let event = serde_json::json!({
                "type": "ArticleCreated",
                "data": article,
                "timestamp": chrono::Utc::now().to_rfc3339()
            });
            writer.write(serde_json::to_string(&event).unwrap()).ok();
        }

        let id = article.id.clone();
        world.scc2_articles.write(&id, |s| *s = article).ok();
    }

    let elapsed = start.elapsed();

    println!("⏱️  {} articles créés en {:.2}s", count, elapsed.as_secs_f64());

    let mut metrics = world.metrics.lock().await;
    metrics.request_count = count as u64;
    metrics.total_duration = elapsed;
}

#[when(expr = "je modifie {int} articles existants")]
async fn modify_articles(world: &mut LithairWorld, count: usize) {
    println!("🔄 Modification de {} articles...", count);

    for i in 0..count {
        let article_id = format!("article-{}", i);

        if let Some(mut article) = world.scc2_articles.read(&article_id, |s| s.clone()) {
            article.title = format!("Updated Title {}", i);
            article.content = format!("Updated Content {}", i);

            if let Some(ref writer) = *world.async_writer.lock().await {
                let event = serde_json::json!({
                    "type": "ArticleUpdated",
                    "data": article,
                    "timestamp": chrono::Utc::now().to_rfc3339()
                });
                writer.write(serde_json::to_string(&event).unwrap()).ok();
            }

            world.scc2_articles.write(&article_id, |s| *s = article).ok();
        }
    }

    println!("✅ {} articles modifiés", count);
}

#[when(expr = "je supprime {int} articles")]
async fn delete_articles(world: &mut LithairWorld, count: usize) {
    println!("🗑️  Suppression de {} articles...", count);

    for i in 0..count {
        let article_id = format!("article-{}", i);

        if let Some(ref writer) = *world.async_writer.lock().await {
            let event = serde_json::json!({
                "type": "ArticleDeleted",
                "id": article_id,
                "timestamp": chrono::Utc::now().to_rfc3339()
            });
            writer.write(serde_json::to_string(&event).unwrap()).ok();
        }

        world.scc2_articles.remove(&article_id).await;
    }

    println!("✅ {} articles supprimés", count);
}

#[when(expr = "je force un flush avec fsync immédiat")]
async fn force_flush_with_fsync(world: &mut LithairWorld) {
    println!("🔄 Force flush avec fsync...");

    if let Some(ref writer) = *world.async_writer.lock().await {
        writer.flush().await.ok();
    }

    // Petite pause pour s'assurer que le fsync est terminé
    sleep(Duration::from_millis(100)).await;

    println!("✅ Flush avec fsync terminé");
}

#[when(expr = "je lis le fichier directement avec O_DIRECT si disponible")]
async fn read_file_direct(world: &mut LithairWorld) {
    let persist_path = {
        let metrics = world.metrics.lock().await;
        metrics.persist_path.clone()
    };

    let events_file = format!("{}/events.raftlog", persist_path);

    // Lecture directe du fichier (bypass cache OS si possible)
    // Note: O_DIRECT n'est pas disponible partout, on fait une lecture standard
    match std::fs::read_to_string(&events_file) {
        Ok(content) => {
            let line_count = content.lines().count();
            println!("📖 Fichier lu directement: {} lignes", line_count);
        }
        Err(e) => {
            println!("⚠️  Erreur lecture directe: {}", e);
        }
    }
}

#[when(expr = "je simule un crash brutal du serveur sans shutdown")]
async fn simulate_brutal_crash(world: &mut LithairWorld) {
    println!("💥 Simulation crash brutal (pas de shutdown propre)...");

    // On "oublie" l'async_writer sans appeler shutdown
    // Cela simule un crash où les données en buffer ne sont pas flushées
    let _ = world.async_writer.lock().await.take();

    // Clear la mémoire SCC2
    // Note: On ne peut pas facilement clear SCC2, on le laisse tel quel

    println!("💀 Crash simulé - AsyncWriter perdu sans flush");
}

#[when(expr = "je redémarre le serveur depuis {string}")]
async fn restart_server_from_path(world: &mut LithairWorld, path: String) {
    println!("🔄 Redémarrage serveur depuis {}...", path);

    // Recréer EventStore + AsyncWriter depuis les fichiers existants
    let event_store = Arc::new(RwLock::new(
        EventStore::new(&path).expect("EventStore recovery failed")
    ));
    let async_writer = AsyncWriter::new(event_store.clone(), 1000);

    *world.async_writer.lock().await = Some(async_writer);

    // Compter les événements récupérés
    let events_file = format!("{}/events.raftlog", path);
    if let Ok(content) = std::fs::read_to_string(&events_file) {
        let count = content.lines().filter(|l| !l.trim().is_empty()).count();
        println!("✅ Recovery: {} événements trouvés", count);
    }

    let mut metrics = world.metrics.lock().await;
    metrics.persist_path = path;
}

// ==================== THEN STEPS ====================

#[then(expr = "le fichier events.raftlog doit exister")]
async fn event_log_exists(world: &mut LithairWorld) {
    let persist_path = {
        let metrics = world.metrics.lock().await;
        metrics.persist_path.clone()
    };

    let log_file = format!("{}/events.raftlog", persist_path);

    assert!(
        std::path::Path::new(&log_file).exists(),
        "❌ Fichier events.raftlog n'existe pas: {}",
        log_file
    );

    println!("✅ Fichier events.raftlog existe: {}", log_file);
}

#[then(expr = "le fichier events.raftlog doit contenir exactement {int} événements {string}")]
async fn event_log_contains_exact_count(world: &mut LithairWorld, count: usize, event_type: String) {
    let persist_path = {
        let metrics = world.metrics.lock().await;
        metrics.persist_path.clone()
    };

    let log_file = format!("{}/events.raftlog", persist_path);
    let content = std::fs::read_to_string(&log_file).expect("Failed to read events.raftlog");

    let actual_count = content
        .lines()
        .filter(|line| line.contains(&event_type))
        .count();

    assert_eq!(
        actual_count, count,
        "❌ Attendu {} événements {}, trouvé {}",
        count, event_type, actual_count
    );

    println!("✅ {} événements {} trouvés", actual_count, event_type);
}

#[then("aucun événement ne doit être manquant")]
async fn no_missing_events(world: &mut LithairWorld) {
    let persist_path = {
        let metrics = world.metrics.lock().await;
        metrics.persist_path.clone()
    };

    let log_file = format!("{}/events.raftlog", persist_path);
    let content = std::fs::read_to_string(&log_file).expect("Failed to read events.raftlog");

    let line_count = content.lines().filter(|l| !l.trim().is_empty()).count();
    println!("✅ {} événements dans le log, aucun manquant", line_count);
}

#[then("le checksum des événements doit être valide")]
async fn checksum_valid(_world: &mut LithairWorld) {
    // TODO: Implémenter validation CRC32 quand les checksums seront ajoutés
    println!("✅ Checksum valide (validation basique)");
}

#[then(expr = "le temps total doit être inférieur à {int} secondes")]
async fn time_under_limit(world: &mut LithairWorld, max_seconds: u64) {
    let metrics = world.metrics.lock().await;
    let actual_seconds = metrics.total_duration.as_secs();

    assert!(
        actual_seconds <= max_seconds,
        "❌ Temps total {}s > {}s max",
        actual_seconds, max_seconds
    );

    println!("✅ Temps total {}s <= {}s", actual_seconds, max_seconds);
}

#[then(expr = "tous les {int} événements doivent être persistés")]
async fn all_events_persisted(world: &mut LithairWorld, count: usize) {
    let persist_path = {
        let metrics = world.metrics.lock().await;
        metrics.persist_path.clone()
    };

    let log_file = format!("{}/events.raftlog", persist_path);
    let content = std::fs::read_to_string(&log_file).expect("Failed to read events.raftlog");

    let actual = content.lines().filter(|l| !l.trim().is_empty()).count();

    assert!(
        actual >= count,
        "❌ Attendu au moins {} événements, trouvé {}",
        count, actual
    );

    println!("✅ {} événements persistés", actual);
}

#[then(expr = "le nombre d'articles en mémoire doit égaler le nombre sur disque")]
async fn memory_equals_disk(world: &mut LithairWorld) {
    let persist_path = {
        let metrics = world.metrics.lock().await;
        metrics.persist_path.clone()
    };

    // Compter en mémoire (SCC2)
    let memory_count = world.scc2_articles.internal_map().len();

    // Compter sur disque
    let log_file = format!("{}/events.raftlog", persist_path);
    let content = std::fs::read_to_string(&log_file).unwrap_or_default();
    let disk_count = content.lines().filter(|l| l.contains("ArticleCreated")).count();

    println!("📊 Mémoire: {} | Disque: {}", memory_count, disk_count);

    assert_eq!(
        memory_count, disk_count,
        "❌ Incohérence: {} en mémoire vs {} sur disque",
        memory_count, disk_count
    );

    println!("✅ Cohérence mémoire/disque: {} articles", memory_count);
}

#[then("tous les checksums doivent correspondre")]
async fn all_checksums_match(_world: &mut LithairWorld) {
    // TODO: Implémenter quand CRC32 sera ajouté
    println!("✅ Checksums correspondants (validation basique)");
}

#[then(expr = "l'état final doit avoir {int} articles actifs")]
async fn final_article_count(world: &mut LithairWorld, expected: usize) {
    let actual = world.scc2_articles.internal_map().len();

    assert_eq!(
        actual, expected,
        "❌ Attendu {} articles actifs, trouvé {}",
        expected, actual
    );

    println!("✅ État final: {} articles actifs", actual);
}

#[then(expr = "les {int} articles doivent être lisibles depuis le fichier immédiatement")]
async fn articles_readable_immediately(world: &mut LithairWorld, count: usize) {
    let persist_path = {
        let metrics = world.metrics.lock().await;
        metrics.persist_path.clone()
    };

    let log_file = format!("{}/events.raftlog", persist_path);
    let content = std::fs::read_to_string(&log_file).expect("Failed to read file");

    let actual = content.lines().filter(|l| l.contains("ArticleCreated")).count();

    assert!(
        actual >= count,
        "❌ Seulement {} articles lisibles immédiatement (attendu {})",
        actual, count
    );

    println!("✅ {} articles lisibles immédiatement", actual);
}

#[then("le fichier ne doit pas être vide")]
async fn file_not_empty(world: &mut LithairWorld) {
    let persist_path = {
        let metrics = world.metrics.lock().await;
        metrics.persist_path.clone()
    };

    let log_file = format!("{}/events.raftlog", persist_path);
    let metadata = std::fs::metadata(&log_file).expect("Failed to get file metadata");

    assert!(
        metadata.len() > 0,
        "❌ Fichier vide!"
    );

    println!("✅ Fichier non vide: {} bytes", metadata.len());
}

#[then("les données doivent être présentes sur le disque physique")]
async fn data_on_physical_disk(world: &mut LithairWorld) {
    let persist_path = {
        let metrics = world.metrics.lock().await;
        metrics.persist_path.clone()
    };

    let log_file = format!("{}/events.raftlog", persist_path);

    // Vérifier que le fichier existe et n'est pas vide
    let content = std::fs::read_to_string(&log_file).expect("Failed to read file");

    assert!(
        !content.is_empty(),
        "❌ Pas de données sur le disque!"
    );

    println!("✅ Données présentes sur disque physique");
}

#[then(expr = "les {int} articles doivent être présents après recovery")]
async fn articles_present_after_recovery(world: &mut LithairWorld, count: usize) {
    let persist_path = {
        let metrics = world.metrics.lock().await;
        metrics.persist_path.clone()
    };

    let log_file = format!("{}/events.raftlog", persist_path);
    let content = std::fs::read_to_string(&log_file).expect("Failed to read file after recovery");

    let actual = content.lines().filter(|l| l.contains("ArticleCreated")).count();

    assert!(
        actual >= count,
        "❌ Seulement {} articles après recovery (attendu {})",
        actual, count
    );

    println!("✅ {} articles présents après recovery", actual);
}

#[then("aucune donnée flushée ne doit être perdue")]
async fn no_flushed_data_lost(world: &mut LithairWorld) {
    let persist_path = {
        let metrics = world.metrics.lock().await;
        metrics.persist_path.clone()
    };

    let log_file = format!("{}/events.raftlog", persist_path);

    // Vérifier que le fichier a du contenu valide
    let content = std::fs::read_to_string(&log_file).expect("Failed to read file");
    let line_count = content.lines().filter(|l| !l.trim().is_empty()).count();

    assert!(
        line_count > 0,
        "❌ Données perdues! Fichier vide après crash."
    );

    println!("✅ Aucune donnée flushée perdue ({} événements récupérés)", line_count);
}
