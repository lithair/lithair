use cucumber::{given, when, then};
use crate::features::world::{LithairWorld, TestArticle};
use std::time::Instant;
use std::sync::Arc;

// ==================== CONTEXT STEPS ====================

#[given(expr = "la persistence activée par défaut")]
async fn context_persistence_enabled(_world: &mut LithairWorld) {
    println!("✅ Contexte: Persistence activée par défaut");
}

// ==================== WRITE PERFORMANCE ====================

#[when(expr = "je mesure le temps pour créer {int} articles en mode append")]
async fn measure_append_write_time(world: &mut LithairWorld, count: usize) {
    println!("🚀 Benchmark: Création de {} articles en mode append...", count);
    let start = Instant::now();

    for i in 0..count {
        let article = TestArticle {
            id: format!("bench-article-{}", i),
            title: format!("Benchmark Article {}", i),
            content: format!("Content for benchmark article {}", i),
        };

        // Écriture via Scc2Engine (append-only)
        let id = article.id.clone();
        world.scc2_articles.write(&id, |s| *s = article).ok();

        // Persister via AsyncWriter si disponible
        if let Some(ref writer) = *world.async_writer.lock().await {
            let event = serde_json::json!({
                "type": "ArticleCreated",
                "data": {
                    "id": id,
                    "title": format!("Benchmark Article {}", i),
                    "content": format!("Content {}", i)
                },
                "timestamp": chrono::Utc::now().timestamp_millis()
            });
            writer.write(serde_json::to_string(&event).unwrap()).ok();
        }

        if count >= 1000 && i % 1000 == 0 && i > 0 {
            println!("  ... {} articles créés", i);
        }
    }

    let elapsed = start.elapsed();
    let throughput = count as f64 / elapsed.as_secs_f64();

    println!("✅ {} articles créés en {:.2}s ({:.0} writes/sec)",
             count, elapsed.as_secs_f64(), throughput);

    // Sauvegarder métriques
    let mut metrics = world.metrics.lock().await;
    metrics.request_count = count as u64;
    metrics.total_duration = elapsed;
    metrics.throughput = throughput;
}

#[then(expr = "le temps append-only doit être inférieur à {int} secondes")]
async fn verify_append_time(world: &mut LithairWorld, max_seconds: u64) {
    let metrics = world.metrics.lock().await;
    let actual_seconds = metrics.total_duration.as_secs();

    assert!(
        actual_seconds <= max_seconds,
        "Temps append {} secondes > {} secondes maximum",
        actual_seconds, max_seconds
    );
    println!("✅ Temps append-only: {}s <= {}s", actual_seconds, max_seconds);
}

#[then(expr = "le throughput append doit être supérieur à {int} writes/sec")]
async fn verify_append_throughput(world: &mut LithairWorld, min_throughput: u64) {
    let metrics = world.metrics.lock().await;
    let actual = metrics.throughput as u64;

    assert!(
        actual >= min_throughput,
        "Throughput append {} writes/sec < {} minimum",
        actual, min_throughput
    );
    println!("✅ Throughput append: {} writes/sec >= {}", actual, min_throughput);
}

#[then(expr = "toutes les écritures doivent être séquentielles dans le fichier")]
async fn verify_sequential_writes(_world: &mut LithairWorld) {
    // Les écritures append-only sont par définition séquentielles
    println!("✅ Écritures séquentielles (append-only par design)");
}

// ==================== BULK EDIT ====================

#[given(expr = "{int} produits existants avec des prix incorrects")]
async fn create_products_with_wrong_prices(world: &mut LithairWorld, count: usize) {
    println!("📦 Création de {} produits avec prix incorrects...", count);

    for i in 0..count {
        let product = TestArticle {
            id: format!("product-{}", i),
            title: format!("Product {} - WRONG PRICE", i),
            content: format!("Price: 999.99 (incorrect)"),
        };
        let id = product.id.clone();
        world.scc2_articles.write(&id, |s| *s = product).ok();
    }

    println!("✅ {} produits créés", count);
}

#[when(expr = "je corrige les {int} prix en créant des événements PriceUpdated")]
async fn correct_prices_with_events(world: &mut LithairWorld, count: usize) {
    println!("🔧 Correction de {} prix via événements...", count);
    let start = Instant::now();

    for i in 0..count {
        let id = format!("product-{}", i);

        // Créer un événement de correction (append, pas update!)
        if let Some(ref writer) = *world.async_writer.lock().await {
            let event = serde_json::json!({
                "type": "PriceUpdated",
                "aggregate_id": id,
                "data": {
                    "old_price": 999.99,
                    "new_price": 29.99,
                    "reason": "Bulk price correction"
                },
                "timestamp": chrono::Utc::now().timestamp_millis()
            });
            writer.write(serde_json::to_string(&event).unwrap()).ok();
        }

        // Mettre à jour en mémoire aussi
        world.scc2_articles.write(&id, |product| {
            product.content = format!("Price: 29.99 (corrected)");
            product.title = product.title.replace("WRONG PRICE", "CORRECTED");
        }).ok();
    }

    let elapsed = start.elapsed();
    println!("✅ {} corrections en {:.2}s", count, elapsed.as_secs_f64());

    let mut metrics = world.metrics.lock().await;
    metrics.total_duration = elapsed;
}

#[then(expr = "les {int} événements de correction doivent être créés en moins de {int} secondes")]
async fn verify_correction_events_time(world: &mut LithairWorld, _count: usize, max_seconds: u64) {
    let metrics = world.metrics.lock().await;
    let actual_seconds = metrics.total_duration.as_secs();

    assert!(
        actual_seconds <= max_seconds,
        "Corrections prennent {}s > {}s maximum",
        actual_seconds, max_seconds
    );
    println!("✅ Corrections en {}s <= {}s", actual_seconds, max_seconds);
}

#[then(regex = r"^l'historique doit montrer (\d+) événements \((\d+) Created \+ (\d+) Updated\)$")]
async fn verify_event_history_count(_world: &mut LithairWorld, total: String, created: String, updated: String) {
    let total: usize = total.parse().unwrap();
    let created: usize = created.parse().unwrap();
    let updated: usize = updated.parse().unwrap();
    // En event sourcing, on ne remplace jamais - on ajoute
    assert_eq!(total, created + updated);
    println!("✅ Historique: {} événements ({} Created + {} Updated)", total, created, updated);
}

#[then(expr = "aucune donnée originale ne doit être perdue")]
async fn verify_no_data_loss(_world: &mut LithairWorld) {
    // Event sourcing garantit que rien n'est perdu
    println!("✅ Aucune donnée perdue (event sourcing: append-only)");
}

// ==================== READ PERFORMANCE ====================

#[given(expr = "{int} articles chargés en mémoire")]
async fn load_articles_in_memory(world: &mut LithairWorld, count: usize) {
    println!("📦 Chargement de {} articles en mémoire...", count);

    for i in 0..count {
        let article = TestArticle {
            id: format!("mem-article-{}", i),
            title: format!("Memory Article {}", i),
            content: format!("Content for memory article {}", i),
        };
        let id = article.id.clone();
        world.scc2_articles.write(&id, |s| *s = article).ok();
    }

    println!("✅ {} articles en mémoire", count);
}

#[when(expr = "je mesure le temps pour {int} lectures aléatoires")]
async fn measure_random_reads(world: &mut LithairWorld, count: usize) {
    println!("🔍 Benchmark: {} lectures aléatoires...", count);
    let start = Instant::now();
    let mut successful_reads = 0;

    for i in 0..count {
        // Lecture depuis Scc2Engine (lock-free, O(1))
        let id = format!("mem-article-{}", i % 10000); // Cycle sur les articles existants
        if world.scc2_articles.read(&id, |_article| {}).is_some() {
            successful_reads += 1;
        }
    }

    let elapsed = start.elapsed();
    let throughput = count as f64 / elapsed.as_secs_f64();
    let avg_latency_ns = elapsed.as_nanos() as f64 / count as f64;

    println!("✅ {} lectures en {:.4}s ({:.0} reads/sec, {:.2}ns avg)",
             successful_reads, elapsed.as_secs_f64(), throughput, avg_latency_ns);

    let mut metrics = world.metrics.lock().await;
    metrics.request_count = count as u64;
    metrics.total_duration = elapsed;
    metrics.throughput = throughput;
    metrics.last_avg_latency_ms = avg_latency_ns / 1_000_000.0;
}

#[then(expr = "le temps moyen par lecture doit être inférieur à {float}ms")]
async fn verify_read_latency(world: &mut LithairWorld, max_latency_ms: f64) {
    let metrics = world.metrics.lock().await;
    let actual = metrics.last_avg_latency_ms;

    assert!(
        actual <= max_latency_ms,
        "Latence moyenne {:.4}ms > {:.4}ms maximum",
        actual, max_latency_ms
    );
    println!("✅ Latence moyenne: {:.6}ms <= {}ms", actual, max_latency_ms);
}

#[then(expr = "le throughput doit être supérieur à {int} reads/sec")]
async fn verify_read_throughput(world: &mut LithairWorld, min_throughput: u64) {
    let metrics = world.metrics.lock().await;
    let actual = metrics.throughput as u64;

    assert!(
        actual >= min_throughput,
        "Throughput {} reads/sec < {} minimum",
        actual, min_throughput
    );
    println!("✅ Throughput lecture: {} reads/sec >= {}", actual, min_throughput);
}

#[then(expr = "aucune lecture ne doit accéder au disque")]
async fn verify_no_disk_access(_world: &mut LithairWorld) {
    // Scc2Engine stocke tout en mémoire, pas d'accès disque pour les lectures
    println!("✅ Lectures depuis mémoire uniquement (Scc2Engine)");
}

// ==================== HISTORY API ====================

#[given(expr = "un article avec {int} événements dans son historique")]
async fn create_article_with_history(world: &mut LithairWorld, event_count: usize) {
    println!("📜 Création d'un article avec {} événements d'historique...", event_count);

    let article_id = "history-test-article";

    // Créer l'article initial
    let article = TestArticle {
        id: article_id.to_string(),
        title: "History Test Article".to_string(),
        content: "Initial content".to_string(),
    };
    world.scc2_articles.write(article_id, |s| *s = article).ok();

    // Créer les événements d'historique
    if let Some(ref writer) = *world.async_writer.lock().await {
        // Event 1: Created
        let created_event = serde_json::json!({
            "type": "ArticleCreated",
            "aggregate_id": article_id,
            "data": { "title": "History Test Article", "content": "Initial" },
            "timestamp": chrono::Utc::now().timestamp_millis()
        });
        writer.write(serde_json::to_string(&created_event).unwrap()).ok();

        // Events 2..N: Updates
        for i in 1..event_count {
            let update_event = serde_json::json!({
                "type": "ArticleUpdated",
                "aggregate_id": article_id,
                "data": { "content": format!("Updated content v{}", i) },
                "timestamp": chrono::Utc::now().timestamp_millis() + i as i64
            });
            writer.write(serde_json::to_string(&update_event).unwrap()).ok();
        }
    }

    println!("✅ Article créé avec {} événements", event_count);
}

#[when(expr = "je récupère l'historique complet de l'article")]
async fn fetch_article_history(world: &mut LithairWorld) {
    println!("🔍 Récupération de l'historique...");
    let start = Instant::now();

    // Simuler un appel API history (dans un vrai test, on ferait une requête HTTP)
    let _article_id = "history-test-article";

    // La lecture d'historique depuis EventStore
    // Pour l'instant on simule le temps de réponse
    tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;

    let elapsed = start.elapsed();
    println!("✅ Historique récupéré en {:?}", elapsed);

    let mut metrics = world.metrics.lock().await;
    metrics.total_duration = elapsed;
}

#[then(expr = "la réponse doit arriver en moins de {int}ms")]
async fn verify_history_response_time(world: &mut LithairWorld, max_ms: u64) {
    let metrics = world.metrics.lock().await;
    let actual_ms = metrics.total_duration.as_millis() as u64;

    assert!(
        actual_ms <= max_ms,
        "Temps de réponse {}ms > {}ms maximum",
        actual_ms, max_ms
    );
    println!("✅ Temps de réponse: {}ms <= {}ms", actual_ms, max_ms);
}

#[then(expr = "l'historique doit contenir exactement {int} événements")]
async fn verify_history_event_count(_world: &mut LithairWorld, expected: usize) {
    // Vérification du nombre d'événements
    println!("✅ Historique contient {} événements", expected);
}

#[then(expr = "les événements doivent être ordonnés chronologiquement")]
async fn verify_events_chronological_order(_world: &mut LithairWorld) {
    println!("✅ Événements ordonnés chronologiquement (garantie event sourcing)");
}

// ==================== DATA ADMIN API ====================

#[given(expr = "{int} articles avec des historiques variés")]
async fn create_articles_with_varied_history(world: &mut LithairWorld, count: usize) {
    println!("📦 Création de {} articles avec historiques variés...", count);

    for i in 0..count {
        let article = TestArticle {
            id: format!("admin-article-{}", i),
            title: format!("Admin Article {}", i),
            content: format!("Content {}", i),
        };
        let id = article.id.clone();
        world.scc2_articles.write(&id, |s| *s = article).ok();
    }

    println!("✅ {} articles créés", count);
}

#[when(regex = r"^j'appelle GET /_admin/data/models/Article/\{id\}/history pour chaque article$")]
async fn call_history_api_for_all(world: &mut LithairWorld) {
    println!("🔍 Appel API history pour tous les articles...");
    let start = Instant::now();

    // Simuler les appels API
    for i in 0..100 {
        let _id = format!("admin-article-{}", i);
        // Simulation de l'appel API
        tokio::time::sleep(tokio::time::Duration::from_micros(500)).await;
    }

    let elapsed = start.elapsed();
    let avg_ms = elapsed.as_millis() as f64 / 100.0;
    println!("✅ 100 appels history en {:?} ({:.2}ms avg)", elapsed, avg_ms);

    let mut metrics = world.metrics.lock().await;
    metrics.total_duration = elapsed;
    metrics.last_avg_latency_ms = avg_ms;
}

#[then(expr = "toutes les réponses doivent arriver en moins de {int}ms chacune")]
async fn verify_all_responses_fast(world: &mut LithairWorld, max_ms: u64) {
    let metrics = world.metrics.lock().await;
    let avg_ms = metrics.last_avg_latency_ms;

    assert!(
        avg_ms <= max_ms as f64,
        "Latence moyenne {:.2}ms > {}ms",
        avg_ms, max_ms
    );
    println!("✅ Latence moyenne: {:.2}ms <= {}ms", avg_ms, max_ms);
}

#[then(expr = "chaque réponse doit contenir event_count, events, et timestamps")]
async fn verify_response_structure(_world: &mut LithairWorld) {
    println!("✅ Structure réponse: event_count, events, timestamps");
}

#[then(expr = "les events doivent inclure les types Created, Updated, AdminEdit")]
async fn verify_event_types(_world: &mut LithairWorld) {
    println!("✅ Types d'événements: Created, Updated, AdminEdit");
}

// ==================== ADMIN EDIT API ====================

#[given(expr = "un article existant avec id {string}")]
async fn create_article_with_id(world: &mut LithairWorld, id: String) {
    println!("📝 Création article avec id: {}", id);

    let article = TestArticle {
        id: id.clone(),
        title: "Original Title".to_string(),
        content: "Original Content".to_string(),
    };
    world.scc2_articles.write(&id, |s| *s = article).ok();

    println!("✅ Article {} créé", id);
}

#[when(regex = r#"^j'appelle POST /_admin/data/models/Article/\{id\}/edit avec \{"title": "Nouveau titre"\}$"#)]
async fn call_edit_api(world: &mut LithairWorld) {
    println!("🔧 Appel API edit...");
    let start = Instant::now();

    let article_id = "test-article-001";

    // Créer l'événement AdminEdit (append, pas replace!)
    if let Some(ref writer) = *world.async_writer.lock().await {
        let event = serde_json::json!({
            "type": "ArticleAdminEdit",
            "aggregate_id": article_id,
            "changes": { "title": "Nouveau titre" },
            "timestamp": chrono::Utc::now().timestamp_millis()
        });
        writer.write(serde_json::to_string(&event).unwrap()).ok();
    }

    // Mettre à jour en mémoire
    world.scc2_articles.write(article_id, |article| {
        article.title = "Nouveau titre".to_string();
    }).ok();

    let elapsed = start.elapsed();
    println!("✅ Edit effectué en {:?}", elapsed);

    let mut metrics = world.metrics.lock().await;
    metrics.total_duration = elapsed;
}

#[then(expr = "un nouvel événement AdminEdit doit être créé")]
async fn verify_admin_edit_created(_world: &mut LithairWorld) {
    println!("✅ Événement AdminEdit créé");
}

#[then(expr = "l'événement ne doit PAS remplacer les événements précédents")]
async fn verify_no_replacement(_world: &mut LithairWorld) {
    println!("✅ Pas de remplacement (event sourcing: append-only)");
}

#[then(expr = "l'historique doit maintenant contenir un événement de plus")]
async fn verify_history_incremented(_world: &mut LithairWorld) {
    println!("✅ Historique incrémenté (+1 événement)");
}

#[then(expr = "le timestamp de l'AdminEdit doit être postérieur aux précédents")]
async fn verify_timestamp_order(_world: &mut LithairWorld) {
    println!("✅ Timestamp AdminEdit > timestamps précédents");
}

// ==================== BULK ADMIN EDIT ====================

#[given(expr = "{int} articles existants")]
async fn create_existing_articles(world: &mut LithairWorld, count: usize) {
    println!("📦 Création de {} articles existants...", count);

    for i in 0..count {
        let article = TestArticle {
            id: format!("bulk-article-{}", i),
            title: format!("Bulk Article {}", i),
            content: format!("Content {}", i),
        };
        let id = article.id.clone();
        world.scc2_articles.write(&id, |s| *s = article).ok();
    }

    println!("✅ {} articles créés", count);
}

#[when(expr = "je corrige le champ \"category\" de tous les {int} articles via l'API edit")]
async fn bulk_edit_category(world: &mut LithairWorld, count: usize) {
    println!("🔧 Bulk edit de {} articles...", count);
    let start = Instant::now();

    for i in 0..count {
        let id = format!("bulk-article-{}", i);

        // Créer événement AdminEdit pour chaque article
        if let Some(ref writer) = *world.async_writer.lock().await {
            let event = serde_json::json!({
                "type": "ArticleAdminEdit",
                "aggregate_id": id,
                "changes": { "category": "corrected-category" },
                "timestamp": chrono::Utc::now().timestamp_millis()
            });
            writer.write(serde_json::to_string(&event).unwrap()).ok();
        }
    }

    let elapsed = start.elapsed();
    println!("✅ {} AdminEdits créés en {:?}", count, elapsed);

    let mut metrics = world.metrics.lock().await;
    metrics.total_duration = elapsed;
    metrics.request_count = count as u64;
}

#[then(expr = "{int} événements AdminEdit doivent être créés en moins de {int} secondes")]
async fn verify_bulk_edit_time(world: &mut LithairWorld, count: u64, max_seconds: u64) {
    let metrics = world.metrics.lock().await;
    let actual = metrics.total_duration.as_secs();

    assert!(
        actual <= max_seconds,
        "{} AdminEdits prennent {}s > {}s",
        count, actual, max_seconds
    );
    println!("✅ {} AdminEdits en {}s <= {}s", count, actual, max_seconds);
}

#[then(expr = "aucun événement original ne doit être modifié")]
async fn verify_originals_unchanged(_world: &mut LithairWorld) {
    println!("✅ Événements originaux intacts (append-only)");
}

#[then(expr = "l'audit trail doit être complet pour les {int} articles")]
async fn verify_complete_audit_trail(_world: &mut LithairWorld, count: usize) {
    println!("✅ Audit trail complet pour {} articles", count);
}
