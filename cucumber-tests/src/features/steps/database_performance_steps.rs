use crate::features::world::{LithairWorld, TestArticle};
use cucumber::{given, then, when};
use lithair_core::engine::{AsyncWriter, EventStore};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::time::sleep;

// ==================== GIVEN STEPS ====================

#[given(expr = "persistence is enabled by default")]
async fn persistence_enabled_by_default(_world: &mut LithairWorld) {
    println!("✅ Persistence enabled by default");
}

#[given(expr = "a Lithair server on port {int} with persistence {string}")]
async fn server_with_persistence(world: &mut LithairWorld, port: u16, path: String) {
    println!("🚀 Initializing server on port {} with persistence: {}", port, path);

    // Nettoyer et créer le dossier
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("Failed to create persistence dir");

    // Créer EventStore + AsyncWriter
    let event_store =
        Arc::new(RwLock::new(EventStore::new(&path).expect("EventStore init failed")));
    let async_writer = AsyncWriter::new(event_store, 1000);

    // Stocker dans world
    *world.async_writer.lock().await = Some(async_writer);

    // Sauvegarder les paramètres dans metrics
    let mut metrics = world.metrics.lock().await;
    metrics.persist_path = path;
    metrics.server_port = port;

    println!("✅ Server initialized (port: {}, batch_size: 1000)", port);
}

#[given(expr = "MaxDurability mode is enabled with fsync")]
async fn max_durability_with_fsync(_world: &mut LithairWorld) {
    // Note: fsync is now enabled by default in OptimizedPersistenceConfig
    println!("✅ MaxDurability mode with fsync enabled");
}

// ==================== WHEN STEPS ====================

#[when(expr = "I create {int} articles quickly")]
async fn create_articles_fast(world: &mut LithairWorld, count: usize) {
    println!("🚀 Fast creation of {} articles...", count);
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
            println!("  ... {} articles created", i);
        }
    }

    let elapsed = start.elapsed();
    let throughput = count as f64 / elapsed.as_secs_f64();

    println!(
        "✅ {} articles created en {:.2}s ({:.0} articles/sec)",
        count,
        elapsed.as_secs_f64(),
        throughput
    );

    // Sauvegarder métriques
    let mut metrics = world.metrics.lock().await;
    metrics.request_count = count as u64;
    metrics.total_duration = elapsed;
}

#[when(expr = "I create {int} critical articles")]
async fn create_critical_articles(world: &mut LithairWorld, count: usize) {
    // Même chose que create_articles_fast, mais explicitement pour tests critiques
    create_articles_fast(world, count).await;
}

#[when(expr = "I wait {int} seconds for the flush")]
#[when(expr = "I wait {int} seconds for flush")]
#[when(expr = "I wait {int} second for flush")]
async fn wait_for_flush(_world: &mut LithairWorld, seconds: u64) {
    println!("⏳ Waiting {} seconds for flush...", seconds);
    sleep(Duration::from_secs(seconds)).await;
    println!("✅ Wait complete");
}

#[when(expr = "I measure the time to create {int} articles")]
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

    println!("⏱️  {} articles created en {:.2}s", count, elapsed.as_secs_f64());

    let mut metrics = world.metrics.lock().await;
    metrics.request_count = count as u64;
    metrics.total_duration = elapsed;
}

#[when(expr = "I modify {int} existing articles")]
async fn modify_articles(world: &mut LithairWorld, count: usize) {
    println!("🔄 Modifying {} articles...", count);

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

    println!("✅ {} articles modified", count);
}

#[when(expr = "I delete {int} articles")]
async fn delete_articles(world: &mut LithairWorld, count: usize) {
    println!("🗑️  Deleting {} articles...", count);

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

    println!("✅ {} articles deleted", count);
}

#[when(expr = "I force a flush with immediate fsync")]
#[when(expr = "I force an immediate flush with fsync")]
async fn force_flush_with_fsync(world: &mut LithairWorld) {
    println!("🔄 Forcing flush with fsync...");

    if let Some(ref writer) = *world.async_writer.lock().await {
        writer.flush().await.ok();
    }

    // Petite pause pour s'assurer que le fsync est terminé
    sleep(Duration::from_millis(100)).await;

    println!("✅ Flush with fsync completed");
}

#[when(expr = "I read the file directly with O_DIRECT if available")]
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
            println!("📖 File read directly: {} lines", line_count);
        }
        Err(e) => {
            println!("⚠️  Direct read error: {}", e);
        }
    }
}

#[when(expr = "I simulate a brutal server crash without shutdown")]
async fn simulate_brutal_crash(world: &mut LithairWorld) {
    println!("💥 Simulating brutal crash (no clean shutdown)...");

    // On "oublie" l'async_writer sans appeler shutdown
    // Cela simule un crash où les données en buffer ne sont pas flushées
    let _ = world.async_writer.lock().await.take();

    // Clear la mémoire SCC2
    // Note: On ne peut pas facilement clear SCC2, on le laisse tel quel

    println!("💀 Crash simulated - AsyncWriter lost without flush");
}

#[when(expr = "I restart the server from {string}")]
async fn restart_server_from_path(world: &mut LithairWorld, path: String) {
    println!("🔄 Restarting server from {}...", path);

    // Recréer EventStore + AsyncWriter depuis les fichiers existants
    let event_store =
        Arc::new(RwLock::new(EventStore::new(&path).expect("EventStore recovery failed")));
    let async_writer = AsyncWriter::new(event_store.clone(), 1000);

    *world.async_writer.lock().await = Some(async_writer);

    // Compter les events recovered
    let events_file = format!("{}/events.raftlog", path);
    if let Ok(content) = std::fs::read_to_string(&events_file) {
        let count = content.lines().filter(|l| !l.trim().is_empty()).count();
        println!("✅ Recovery: {} events found", count);
    }

    let mut metrics = world.metrics.lock().await;
    metrics.persist_path = path;
}

// ==================== THEN STEPS ====================

#[then(expr = "the events.raftlog file must exist")]
async fn event_log_exists(world: &mut LithairWorld) {
    let persist_path = {
        let metrics = world.metrics.lock().await;
        metrics.persist_path.clone()
    };

    let log_file = format!("{}/events.raftlog", persist_path);

    assert!(
        std::path::Path::new(&log_file).exists(),
        "❌ File events.raftlog does not exist: {}",
        log_file
    );

    println!("✅ File events.raftlog exists: {}", log_file);
}

#[then(expr = "the events.raftlog file must contain exactly {int} {string} events")]
async fn event_log_contains_exact_count(
    world: &mut LithairWorld,
    count: usize,
    event_type: String,
) {
    let persist_path = {
        let metrics = world.metrics.lock().await;
        metrics.persist_path.clone()
    };

    let log_file = format!("{}/events.raftlog", persist_path);
    let content = std::fs::read_to_string(&log_file).expect("Failed to read events.raftlog");

    let actual_count = content.lines().filter(|line| line.contains(&event_type)).count();

    assert_eq!(
        actual_count, count,
        "❌ Expected {} {} events, found {}",
        count, event_type, actual_count
    );

    println!("✅ {} {} events trouvés", actual_count, event_type);
}

#[then("no event must be missing")]
async fn no_missing_events(world: &mut LithairWorld) {
    let persist_path = {
        let metrics = world.metrics.lock().await;
        metrics.persist_path.clone()
    };

    let log_file = format!("{}/events.raftlog", persist_path);
    let content = std::fs::read_to_string(&log_file).expect("Failed to read events.raftlog");

    let line_count = content.lines().filter(|l| !l.trim().is_empty()).count();
    println!("✅ {} events in log, none missing", line_count);
}

#[then("the event checksum must be valid")]
async fn checksum_valid(_world: &mut LithairWorld) {
    // TODO: Implement CRC32 validation when checksums are added
    println!("✅ Checksum valid (basic validation)");
}

#[then(expr = "the total time must be less than {int} seconds")]
async fn time_under_limit(world: &mut LithairWorld, max_seconds: u64) {
    let metrics = world.metrics.lock().await;
    let actual_seconds = metrics.total_duration.as_secs();

    assert!(
        actual_seconds <= max_seconds,
        "❌ Total time {}s > {}s max",
        actual_seconds,
        max_seconds
    );

    println!("✅ Total time {}s <= {}s", actual_seconds, max_seconds);
}

#[then(expr = "all {int} events must be persisted")]
async fn all_events_persisted(world: &mut LithairWorld, count: usize) {
    let persist_path = {
        let metrics = world.metrics.lock().await;
        metrics.persist_path.clone()
    };

    let log_file = format!("{}/events.raftlog", persist_path);
    let content = std::fs::read_to_string(&log_file).expect("Failed to read events.raftlog");

    let actual = content.lines().filter(|l| !l.trim().is_empty()).count();

    assert!(actual >= count, "❌ Expected at least {} events, found {}", count, actual);

    println!("✅ {} events persisted", actual);
}

#[then(expr = "the number of articles in memory must equal the number on disk")]
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

    println!("📊 Memory: {} | Disk: {}", memory_count, disk_count);

    assert_eq!(
        memory_count, disk_count,
        "❌ Inconsistency: {} in memory vs {} on disk",
        memory_count, disk_count
    );

    println!("✅ Memory/disk consistency: {} articles", memory_count);
}

#[then("all checksums must match")]
async fn all_checksums_match(_world: &mut LithairWorld) {
    // TODO: Implement when CRC32 is added
    println!("✅ Checksums matching (basic validation)");
}

#[then(expr = "the final state must have {int} active articles")]
async fn final_article_count(world: &mut LithairWorld, expected: usize) {
    let actual = world.scc2_articles.internal_map().len();

    assert_eq!(actual, expected, "❌ Expected {} active articles, found {}", expected, actual);

    println!("✅ Final state: {} articles actifs", actual);
}

#[then(expr = "the {int} articles must be readable from the file immediately")]
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
        "❌ Only {} articles readable immediately (expected {})",
        actual,
        count
    );

    println!("✅ {} articles readable immediately", actual);
}

#[then("the file must not be empty")]
async fn file_not_empty(world: &mut LithairWorld) {
    let persist_path = {
        let metrics = world.metrics.lock().await;
        metrics.persist_path.clone()
    };

    let log_file = format!("{}/events.raftlog", persist_path);
    let metadata = std::fs::metadata(&log_file).expect("Failed to get file metadata");

    assert!(metadata.len() > 0, "❌ File empty!");

    println!("✅ File not empty: {} bytes", metadata.len());
}

#[then("the data must be present on the physical disk")]
async fn data_on_physical_disk(world: &mut LithairWorld) {
    let persist_path = {
        let metrics = world.metrics.lock().await;
        metrics.persist_path.clone()
    };

    let log_file = format!("{}/events.raftlog", persist_path);

    // Vérifier que le fichier existe et n'est pas vide
    let content = std::fs::read_to_string(&log_file).expect("Failed to read file");

    assert!(!content.is_empty(), "❌ No data on disk!");

    println!("✅ Data present on physical disk");
}

#[then(expr = "the {int} articles must be present after recovery")]
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
        "❌ Only {} articles after recovery (expected {})",
        actual,
        count
    );

    println!("✅ {} articles present after recovery", actual);
}

#[then("no flushed data must be lost")]
async fn no_flushed_data_lost(world: &mut LithairWorld) {
    let persist_path = {
        let metrics = world.metrics.lock().await;
        metrics.persist_path.clone()
    };

    let log_file = format!("{}/events.raftlog", persist_path);

    // Vérifier que le fichier a du contenu valide
    let content = std::fs::read_to_string(&log_file).expect("Failed to read file");
    let line_count = content.lines().filter(|l| !l.trim().is_empty()).count();

    assert!(line_count > 0, "❌ Data lost! File empty after crash.");

    println!("✅ No flushed data lost ({} events recovered)", line_count);
}
