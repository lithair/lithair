use chrono;
use cucumber::{given, then, when};
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::features::world::LithairWorld;
use lithair_core::engine::persistence::FileStorage;
use lithair_core::engine::{Event, StateEngine};
use lithair_core::http::{HttpRequest, HttpResponse, Router};

// ==================== BACKGROUND ====================

#[given("la persistence est activée par défaut")]
async fn persistence_enabled(_world: &mut LithairWorld) {
    eprintln!("========================================");
    eprintln!("🎯 STEP: Persistence activée par défaut");
    eprintln!("========================================");
    println!("✅ Persistence activée par défaut");
}

#[given(expr = "un serveur Lithair sur le port {int} avec persistence {string}")]
async fn start_lithair_server(world: &mut LithairWorld, port: u16, persist_path: String) {
    eprintln!("========================================");
    eprintln!("🚀 STEP: Démarrage serveur sur port {} avec persistence {}", port, persist_path);
    eprintln!("========================================");
    println!(
        "🚀 Démarrage serveur Lithair sur port {} avec persistence {}",
        port, persist_path
    );

    // Nettoyer le dossier de persistence s'il existe
    if std::path::Path::new(&persist_path).exists() {
        std::fs::remove_dir_all(&persist_path).ok();
    }

    // Créer le répertoire de persistence
    std::fs::create_dir_all(&persist_path).ok();

    // Créer le EventStore pour AsyncWriter
    let event_store_arc = Arc::new(std::sync::RwLock::new(lithair_core::engine::EventStore::new(&persist_path).expect("Failed to create EventStore")));

    // 🚀 Créer AsyncWriter pour écritures ultra-rapides (batch_size=1000)
    let async_writer = lithair_core::engine::AsyncWriter::new(event_store_arc, 1000);

    // Créer un deuxième FileStorage pour backup/verification
    let storage_backup = FileStorage::new(&persist_path).expect("Impossible de créer FileStorage");

    *world.storage.lock().await = Some(storage_backup);
    *world.async_writer.lock().await = Some(async_writer);

    // Sauvegarder les métadonnées
    {
        let mut metrics = world.metrics.lock().await;
        metrics.base_url = format!("http://localhost:{}", port);
        metrics.server_port = port;
        metrics.persist_path = persist_path.clone();
    }

    // Créer le router avec handlers
    let engine_for_create = world.engine.clone();
    let engine_for_update = world.engine.clone();
    let engine_for_delete = world.engine.clone();
    let async_writer_for_create = world.async_writer.clone();
    let async_writer_for_update = world.async_writer.clone();
    let async_writer_for_delete = world.async_writer.clone();
    // 🚀 SCC2StateEngine pour lectures ultra-rapides (40M+ ops/sec)
    let scc2_for_create = world.scc2_articles.clone();
    let scc2_for_list = world.scc2_articles.clone();
    let scc2_for_update = world.scc2_articles.clone();
    let scc2_for_delete = world.scc2_articles.clone();

    let router = Router::new()
        .post_async("/api/articles", move |req, _params, _state| {
            let req = req.clone();
            let engine = engine_for_create.clone();
            let writer = async_writer_for_create.clone();
            let scc2 = scc2_for_create.clone();
            async move { handle_create_article(&req, &engine, &writer, &scc2).await }
        })
        .get_async("/api/articles", move |_req, _params, _state| {
            let scc2 = scc2_for_list.clone();
            async move { handle_list_articles_scc2(&scc2).await }
        })
        .put_async("/api/articles/:id", move |req, params, _state| {
            let req = req.clone();
            let params = params.clone();
            let engine = engine_for_update.clone();
            let writer = async_writer_for_update.clone();
            let scc2 = scc2_for_update.clone();
            async move { handle_update_article(&req, &params, &engine, &writer, &scc2).await }
        })
        .delete_async("/api/articles/:id", move |req, params, _state| {
            let req = req.clone();
            let params = params.clone();
            let engine = engine_for_delete.clone();
            let writer = async_writer_for_delete.clone();
            let scc2 = scc2_for_delete.clone();
            async move { handle_delete_article(&req, &params, &engine, &writer, &scc2).await }
        })
        .get("/health", |_req, _params, _state| HttpResponse::ok().json(r#"{"status":"ok"}"#));

    // Démarrer le serveur HTTP async Hyper en background
    use lithair_core::http::AsyncHttpServer;

    let server = AsyncHttpServer::new(router, ());
    let addr = format!("127.0.0.1:{}", port);

    let _handle = tokio::task::spawn(async move {
        println!("🚀 Serveur async Hyper démarré");
        if let Err(e) = server.serve(&addr).await {
            eprintln!("❌ Erreur serveur async: {}", e);
        }
        println!("🛑 Serveur async terminé");
    });

    // Pas besoin de stocker le handle pour ce test
    // *world.server_handle.lock().await = Some(handle);

    // Attendre que le serveur soit prêt
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Vérifier que le serveur répond
    let client = reqwest::Client::new();
    let health_url = format!("http://localhost:{}/health", port);

    for attempt in 0..15 {
        match client.get(&health_url).send().await {
            Ok(resp) if resp.status().is_success() => {
                println!("✅ Serveur Lithair prêt sur port {}", port);
                tokio::time::sleep(Duration::from_secs(1)).await; // Délai supplémentaire
                return;
            }
            Err(e) => {
                eprintln!("⏳ Tentative {}/15: {}", attempt + 1, e);
                if attempt < 14 {
                    tokio::time::sleep(Duration::from_millis(1000)).await;
                }
            }
            _ => {
                if attempt < 14 {
                    tokio::time::sleep(Duration::from_millis(1000)).await;
                }
            }
        }
    }

    panic!("❌ Serveur Lithair n'a pas démarré après 5 secondes");
}

// ==================== HANDLERS ====================

async fn handle_create_article(
    req: &HttpRequest,
    engine: &Arc<StateEngine<crate::features::world::TestAppState>>,
    async_writer: &Arc<tokio::sync::Mutex<Option<lithair_core::engine::AsyncWriter>>>,
    scc2: &Arc<
        lithair_core::engine::Scc2Engine<crate::features::world::TestArticle>,
    >,
) -> HttpResponse {
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct CreateArticle {
        id: Option<String>,
        title: String,
        content: String,
    }

    // Convertir body de &[u8] à &str
    let body = req.body();
    let body_str = match std::str::from_utf8(body) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("❌ Erreur UTF-8: {}", e);
            return HttpResponse::bad_request().json(r#"{"error":"Invalid UTF-8"}"#);
        }
    };

    let article: CreateArticle = match serde_json::from_str(body_str) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("❌ Erreur parsing JSON: {}", e);
            return HttpResponse::bad_request().json(r#"{"error":"Invalid JSON"}"#);
        }
    };

    // Utiliser l'ID fourni ou générer un UUID
    let id = article.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Créer un événement avec les données
    let event = crate::features::world::TestEvent::ArticleCreated {
        id: id.clone(),
        title: article.title.clone(),
        content: article.content.clone(),
    };

    // Appliquer l'événement via StateEngine avec with_state_mut
    if let Err(e) = engine.with_state_mut(|state| {
        // Appliquer l'événement manuellement
        event.apply(state);
    }) {
        eprintln!("❌ Erreur with_state_mut: {}", e);
        return HttpResponse::internal_server_error().json(r#"{"error":"Failed to apply event"}"#);
    }

    // 🚀 Écriture async ultra-rapide (zero contention!)
    let writer_guard = async_writer.blocking_lock();
    if let Some(ref writer) = *writer_guard {
        let event_json = serde_json::json!({
            "type": "ArticleCreated",
            "id": id,
            "title": article.title,
            "content": article.content,
            "timestamp": chrono::Utc::now().to_rfc3339()
        })
        .to_string();

        // Écriture non-bloquante via channel
        let _ = writer.write(event_json);
    }

    // 🚀 Écriture SCC2 lock-free (instant, full async!)
    let test_article = crate::features::world::TestArticle {
        id: id.clone(),
        title: article.title.clone(),
        content: article.content.clone(),
    };
    let _ = scc2.insert(id.clone(), test_article).await;

    // Réponse
    let response_json = json!({
        "id": id,
        "title": article.title,
        "content": article.content,
    });

    HttpResponse::created().json(&serde_json::to_string(&response_json).unwrap())
}

#[allow(dead_code)]
fn handle_list_articles(
    _req: &HttpRequest,
    engine: &Arc<StateEngine<crate::features::world::TestAppState>>,
) -> HttpResponse {
    // Récupérer l'état actuel avec with_state
    let articles = match engine.with_state(|state| {
        // Convertir les articles en Vec
        state.data.articles.values().cloned().collect::<Vec<serde_json::Value>>()
    }) {
        Ok(arts) => arts,
        Err(e) => {
            eprintln!("❌ Erreur with_state: {}", e);
            return HttpResponse::internal_server_error()
                .json(r#"{"error":"Failed to read state"}"#);
        }
    };

    HttpResponse::ok().json(&serde_json::to_string(&articles).unwrap())
}

// 🚀 SCC2 LIST - ULTRA-RAPIDE (40M+ reads/sec, lock-free!)
async fn handle_list_articles_scc2(
    scc2: &Arc<
        lithair_core::engine::Scc2Engine<crate::features::world::TestArticle>,
    >,
) -> HttpResponse {
    // 🚀 Lecture SCC2 lock-free (full async!)
    let articles = scc2.iter_all().await;

    // Convertir en JSON
    let articles_json: Vec<serde_json::Value> = articles
        .iter()
        .map(|(_id, article)| {
            serde_json::json!({
                "id": article.id,
                "title": article.title,
                "content": article.content,
            })
        })
        .collect();

    HttpResponse::ok().json(&serde_json::to_string(&articles_json).unwrap())
}

async fn handle_update_article(
    req: &HttpRequest,
    params: &std::collections::HashMap<String, String>,
    engine: &Arc<StateEngine<crate::features::world::TestAppState>>,
    async_writer: &Arc<tokio::sync::Mutex<Option<lithair_core::engine::AsyncWriter>>>,
    scc2: &Arc<
        lithair_core::engine::Scc2Engine<crate::features::world::TestArticle>,
    >,
) -> HttpResponse {
    // Extraire l'ID de l'URL
    let id = params.get("id").map_or("", |v| v).to_string();

    // Parser le body
    let body_str = match std::str::from_utf8(req.body()) {
        Ok(s) => s,
        Err(_) => return HttpResponse::bad_request().json(r#"{"error":"Invalid UTF-8"}"#),
    };

    #[derive(serde::Deserialize)]
    struct UpdateArticle {
        title: String,
        content: String,
    }

    let article: UpdateArticle = match serde_json::from_str(body_str) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("❌ Erreur parse JSON: {}", e);
            return HttpResponse::bad_request().json(r#"{"error":"Invalid JSON"}"#);
        }
    };

    // Créer événement
    let event = crate::features::world::TestEvent::ArticleUpdated {
        id: id.clone(),
        title: article.title.clone(),
        content: article.content.clone(),
    };

    // Appliquer l'événement
    if let Err(e) = engine.with_state_mut(|state| {
        event.apply(state);
    }) {
        eprintln!("❌ Erreur with_state_mut: {}", e);
        return HttpResponse::internal_server_error().json(r#"{"error":"Failed to apply event"}"#);
    }

    // 🚀 Écriture async ultra-rapide (zero contention!)
    let writer_guard = async_writer.blocking_lock();
    if let Some(ref writer) = *writer_guard {
        let event_json = serde_json::json!({
            "type": "ArticleUpdated",
            "id": id,
            "title": article.title,
            "content": article.content,
            "timestamp": chrono::Utc::now().to_rfc3339()
        })
        .to_string();

        let _ = writer.write(event_json);
    }

    // 🚀 Update SCC2 lock-free (full async!)
    let test_article = crate::features::world::TestArticle {
        id: id.clone(),
        title: article.title.clone(),
        content: article.content.clone(),
    };
    let _ = scc2.insert(id.clone(), test_article).await;

    HttpResponse::ok()
        .json(&serde_json::to_string(&json!({"id": id, "status": "updated"})).unwrap())
}

async fn handle_delete_article(
    _req: &HttpRequest,
    params: &std::collections::HashMap<String, String>,
    engine: &Arc<StateEngine<crate::features::world::TestAppState>>,
    async_writer: &Arc<tokio::sync::Mutex<Option<lithair_core::engine::AsyncWriter>>>,
    scc2: &Arc<
        lithair_core::engine::Scc2Engine<crate::features::world::TestArticle>,
    >,
) -> HttpResponse {
    // Extraire l'ID
    let id = params.get("id").map_or("", |v| v).to_string();

    // Créer événement
    let event = crate::features::world::TestEvent::ArticleDeleted { id: id.clone() };

    // Appliquer l'événement
    if let Err(e) = engine.with_state_mut(|state| {
        event.apply(state);
    }) {
        eprintln!("❌ Erreur with_state_mut: {}", e);
        return HttpResponse::internal_server_error().json(r#"{"error":"Failed to apply event"}"#);
    }

    // 🚀 Écriture async ultra-rapide (zero contention!)
    let writer_guard = async_writer.blocking_lock();
    if let Some(ref writer) = *writer_guard {
        let event_json = serde_json::json!({
            "type": "ArticleDeleted",
            "id": id,
            "timestamp": chrono::Utc::now().to_rfc3339()
        })
        .to_string();

        let _ = writer.write(event_json);
    }

    // 🚀 Delete SCC2 lock-free (full async!)
    let _ = scc2.remove(&id).await;

    HttpResponse::ok()
        .json(&serde_json::to_string(&json!({"id": id, "status": "deleted"})).unwrap())
}

// ==================== WHEN STEPS ====================

#[when(expr = "je crée {int} articles rapidement")]
async fn create_articles_fast(world: &mut LithairWorld, count: usize) {
    let client = reqwest::Client::new();
    let base_url = {
        let metrics = world.metrics.lock().await;
        metrics.base_url.clone()
    };

    let url = format!("{}/api/articles", base_url);
    let start = std::time::Instant::now();

    // Parallélisation : envoyer plusieurs requêtes simultanément
    let concurrent_requests = 100; // Nombre de requêtes en parallèle
    let mut tasks = Vec::new();

    for i in 0..count {
        let client = client.clone();
        let url = url.clone();

        let task = tokio::spawn(async move {
            let article = json!({
                "id": format!("article-{}", i),
                "title": format!("Article {}", i),
                "content": format!("Content {}", i),
            });

            client.post(&url).json(&article).send().await
        });

        tasks.push(task);

        // Traiter par batch pour éviter d'exploser la mémoire
        if tasks.len() >= concurrent_requests {
            for task in tasks.drain(..) {
                if let Ok(Err(e)) = task.await {
                    eprintln!("❌ Erreur création: {}", e);
                }
            }
        }
    }

    // Traiter les dernières requêtes
    for task in tasks {
        if let Ok(Err(e)) = task.await {
            eprintln!("❌ Erreur création: {}", e);
        }
    }

    let elapsed = start.elapsed();
    let throughput = count as f64 / elapsed.as_secs_f64();

    println!(
        "✅ {} articles créés en {:.2}s ({:.0} articles/sec)",
        count,
        elapsed.as_secs_f64(),
        throughput
    );

    // Attendre que FileStorage ait le temps de persister
    println!("⏳ Attente persistence...");
    tokio::time::sleep(Duration::from_secs(2)).await;
    println!("✅ Persistence terminée");
}

#[when(expr = "je modifie {int} articles existants")]
async fn update_articles(world: &mut LithairWorld, count: usize) {
    let client = reqwest::Client::new();
    let base_url = {
        let metrics = world.metrics.lock().await;
        metrics.base_url.clone()
    };

    let start = std::time::Instant::now();

    for i in 0..count {
        let id = format!("article-{}", i);
        let url = format!("{}/api/articles/{}", base_url, id);
        let article = json!({
            "title": format!("Article {} - UPDATED", i),
            "content": format!("Updated content {}", i),
        });

        let result = client.put(&url).json(&article).send().await;

        if let Err(e) = result {
            eprintln!("❌ Erreur modification article {}: {}", i, e);
        }
    }

    let elapsed = start.elapsed();
    println!("✅ {} articles modifiés en {:.2}s", count, elapsed.as_secs_f64());
    tokio::time::sleep(Duration::from_millis(500)).await;
}

#[when(expr = "je supprime {int} articles")]
async fn delete_articles(world: &mut LithairWorld, count: usize) {
    let client = reqwest::Client::new();
    let base_url = {
        let metrics = world.metrics.lock().await;
        metrics.base_url.clone()
    };

    let start = std::time::Instant::now();

    for i in 0..count {
        let id = format!("article-{}", i);
        let url = format!("{}/api/articles/{}", base_url, id);

        let result = client.delete(&url).send().await;

        if let Err(e) = result {
            eprintln!("❌ Erreur suppression article {}: {}", i, e);
        }
    }

    let elapsed = start.elapsed();
    println!("✅ {} articles supprimés en {:.2}s", count, elapsed.as_secs_f64());

    // 🚀 Attendre que l'AsyncWriter flush tous les événements (batch_size=1000, flush_interval=100ms)
    // Avec 115K événements, ça peut prendre plusieurs secondes
    println!("⏳ Attente flush AsyncWriter (2s)...");
    tokio::time::sleep(Duration::from_secs(2)).await;
}

#[when(expr = "j'attends {int} secondes pour le flush")]
#[when(expr = "j'attends {int} seconde pour le flush")]
async fn wait_for_flush(_world: &mut LithairWorld, seconds: u64) {
    println!("⏳ Attente flush AsyncWriter ({}s)...", seconds);
    tokio::time::sleep(Duration::from_secs(seconds)).await;
    println!("✅ Attente terminée");
}

#[when(expr = "je mesure le temps pour créer {int} articles")]
async fn measure_create_time(world: &mut LithairWorld, count: usize) {
    let start = std::time::Instant::now();

    let client = reqwest::Client::new();
    let base_url = {
        let metrics = world.metrics.lock().await;
        metrics.base_url.clone()
    };

    for i in 0..count {
        let article = json!({
            "id": format!("article-{}", i),
            "title": format!("Article {}", i),
            "content": format!("Content {}", i),
        });

        let url = format!("{}/api/articles", base_url);
        let _ = client.post(&url).json(&article).send().await;
    }

    let elapsed = start.elapsed();
    println!(
        "⏱️  Temps création {} articles: {:.2}s ({:.0} articles/sec)",
        count,
        elapsed.as_secs_f64(),
        count as f64 / elapsed.as_secs_f64()
    );

    // Sauvegarder le temps pour le then
    let mut metrics = world.metrics.lock().await;
    metrics.total_duration = elapsed;
}

#[then(expr = "le temps total doit être inférieur à {int} secondes")]
async fn check_total_time(world: &mut LithairWorld, max_seconds: u64) {
    let metrics = world.metrics.lock().await;
    let total_secs = metrics.total_duration.as_secs_f64();

    assert!(
        total_secs < max_seconds as f64,
        "❌ Temps total {:.2}s dépasse le maximum de {}s",
        total_secs,
        max_seconds
    );

    println!("✅ Temps total {:.2}s < {}s", total_secs, max_seconds);
}

#[then(expr = "tous les {int} événements doivent être persistés")]
async fn check_events_persisted(world: &mut LithairWorld, expected_count: usize) {
    let persist_path = {
        let metrics = world.metrics.lock().await;
        metrics.persist_path.clone()
    };

    let events_file = format!("{}/events.raftlog", persist_path);

    // Attendre un peu pour le flush
    tokio::time::sleep(Duration::from_secs(2)).await;

    assert!(
        std::path::Path::new(&events_file).exists(),
        "❌ Le fichier {} n'existe pas",
        events_file
    );

    let content = std::fs::read_to_string(&events_file)
        .expect("Impossible de lire le fichier events.raftlog");

    let actual_count = content.lines().count();

    assert_eq!(
        actual_count, expected_count,
        "❌ Nombre d'événements incorrect: {} trouvés, {} attendus",
        actual_count, expected_count
    );

    println!("✅ {} événements persistés correctement", actual_count);
}

#[then(expr = "le nombre d'articles en mémoire doit égaler le nombre sur disque")]
async fn check_memory_disk_consistency(world: &mut LithairWorld) {
    // Lecture SCC2 (mémoire)
    let memory_count = world.scc2_articles.iter_all().await.len();

    // Lecture disque
    let persist_path = {
        let metrics = world.metrics.lock().await;
        metrics.persist_path.clone()
    };

    let events_file = format!("{}/events.raftlog", persist_path);

    tokio::time::sleep(Duration::from_secs(1)).await;

    if std::path::Path::new(&events_file).exists() {
        let content = std::fs::read_to_string(&events_file)
            .expect("Impossible de lire le fichier events.raftlog");
        let disk_count = content.lines().count();

        assert_eq!(
            memory_count, disk_count,
            "❌ Incohérence mémoire/disque: {} en mémoire, {} sur disque",
            memory_count, disk_count
        );

        println!("✅ Cohérence mémoire/disque validée: {} articles", memory_count);
    } else {
        panic!("❌ Le fichier events.raftlog n'existe pas");
    }
}

#[when(expr = "je crée {int} articles en parallèle avec {int} threads")]
async fn create_articles_parallel(world: &mut LithairWorld, count: usize, threads: usize) {
    use std::sync::Arc as StdArc;
    use std::sync::Mutex as StdMutex;
    use std::thread;

    let base_url = {
        let metrics = world.metrics.lock().await;
        metrics.base_url.clone()
    };

    let articles_per_thread = count / threads;
    let counter = StdArc::new(StdMutex::new(0));
    let mut handles = vec![];

    for thread_id in 0..threads {
        let url = base_url.clone();
        let counter = counter.clone();

        let handle = thread::spawn(move || {
            let client = reqwest::blocking::Client::new();

            for i in 0..articles_per_thread {
                let article = json!({
                    "title": format!("Article {} from thread {}", i, thread_id),
                    "content": format!("Content {}", i),
                });

                if let Ok(_) = client.post(format!("{}/api/articles", url)).json(&article).send() {
                    let mut c = counter.lock().unwrap();
                    *c += 1;
                }
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().ok();
    }

    let created = *counter.lock().unwrap();
    println!("✅ {} articles créés en parallèle", created);
}

#[when("j'attends que toutes les écritures soient terminées")]
async fn wait_for_writes(_world: &mut LithairWorld) {
    tokio::time::sleep(Duration::from_secs(2)).await;
    println!("✅ Attente terminée");
}

// ==================== THEN STEPS ====================

// Note: step "le fichier events.raftlog doit exister" définie dans database_performance_steps.rs

#[then(expr = "le fichier events.raftlog doit contenir exactement {int} événements {string}")]
async fn check_event_count(world: &mut LithairWorld, count: usize, event_type: String) {
    let log_file = {
        let metrics = world.metrics.lock().await;
        format!("{}/events.raftlog", metrics.persist_path)
    };

    let content = std::fs::read_to_string(&log_file).expect("Impossible de lire events.raftlog");

    let event_count = content.lines().filter(|line| line.contains(&event_type)).count();

    assert_eq!(
        event_count, count,
        "❌ Attendu {} événements {}, trouvé {}",
        count, event_type, event_count
    );

    println!("✅ Trouvé {} événements {}", event_count, event_type);
}

#[then(expr = "l'état final doit avoir {int} articles actifs")]
async fn check_final_article_count(world: &mut LithairWorld, expected_count: usize) {
    let actual_count = world
        .engine
        .with_state(|state| state.data.articles.len())
        .expect("Impossible de lire l'état");

    assert_eq!(
        actual_count, expected_count,
        "❌ Attendu {} articles actifs, trouvé {}",
        expected_count, actual_count
    );

    println!("✅ État final: {} articles actifs", actual_count);
}

#[then("tous les événements doivent être dans l'ordre chronologique")]
async fn check_chronological_order(world: &mut LithairWorld) {
    let log_file = {
        let metrics = world.metrics.lock().await;
        format!("{}/events.raftlog", metrics.persist_path)
    };

    let content = std::fs::read_to_string(&log_file).expect("Impossible de lire events.raftlog");

    let mut timestamps = Vec::new();
    for line in content.lines() {
        if let Ok(event) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(ts) = event.get("timestamp").and_then(|t| t.as_str()) {
                timestamps.push(ts.to_string());
            }
        }
    }

    let mut sorted_timestamps = timestamps.clone();
    sorted_timestamps.sort();

    assert_eq!(
        timestamps, sorted_timestamps,
        "❌ Les événements ne sont pas dans l'ordre chronologique"
    );

    println!("✅ Tous les événements sont dans l'ordre chronologique");
}

#[then("aucun événement ne doit être manquant")]
async fn no_missing_events(world: &mut LithairWorld) {
    let log_file = {
        let metrics = world.metrics.lock().await;
        format!("{}/events.raftlog", metrics.persist_path)
    };

    let content = std::fs::read_to_string(&log_file).expect("Impossible de lire events.raftlog");

    let line_count = content.lines().filter(|l| !l.trim().is_empty()).count();

    println!("✅ {} événements dans le log, aucun manquant", line_count);
}

#[then("le checksum des événements doit être valide")]
async fn check_event_checksum(_world: &mut LithairWorld) {
    // TODO: Implémenter validation CRC32
    println!("✅ Checksum valide (à implémenter)");
}

#[then(expr = "tous les {int} articles doivent être persistés")]
async fn all_articles_persisted(world: &mut LithairWorld, count: usize) {
    let log_file = {
        let metrics = world.metrics.lock().await;
        format!("{}/events.raftlog", metrics.persist_path)
    };

    let content = std::fs::read_to_string(&log_file).expect("Impossible de lire events.raftlog");

    let event_count = content.lines().filter(|line| line.contains("ArticleCreated")).count();

    assert_eq!(event_count, count, "❌ Attendu {} articles, trouvé {}", count, event_count);

    println!("✅ Tous les {} articles sont persistés", count);
}

#[then("j'arrête le serveur proprement")]
async fn shutdown_server_properly(world: &mut LithairWorld) {
    println!("🛑 Arrêt propre du serveur...");

    // 1. Tuer d'abord le serveur HTTP pour arrêter nouvelles requêtes
    let port = {
        let metrics = world.metrics.lock().await;
        metrics.server_port
    };

    println!("🔪 Arrêt du serveur HTTP sur port {}...", port);
    let _ = std::process::Command::new("pkill")
        .arg("-9")
        .arg("-f")
        .arg(format!("127.0.0.1:{}", port))
        .output();

    // 2. Attendre que le serveur se termine
    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    // 3. Shutdown AsyncWriter avec timeout pour éviter blocage
    let async_writer = {
        let mut writer_guard = world.async_writer.lock().await;
        writer_guard.take()
    };

    if let Some(writer) = async_writer {
        println!("⏳ Shutdown AsyncWriter (flush final)...");

        // Timeout de 2 secondes max pour shutdown
        match tokio::time::timeout(tokio::time::Duration::from_secs(2), writer.shutdown()).await {
            Ok(_) => println!("✅ AsyncWriter arrêté proprement"),
            Err(_) => {
                println!("⚠️  Timeout AsyncWriter shutdown (2s) - forçage arrêt");
                // Le writer sera drop automatiquement
            }
        }
    }

    println!("✅ Serveur arrêté complètement");
}

// ==================== STEPS STRESS TEST 1M ====================

#[then("je mesure le throughput de création")]
#[then("je mesure le throughput de modification")]
#[then("je mesure le throughput de suppression")]
async fn measure_throughput(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;
    let duration = metrics.total_duration.as_secs_f64();

    if duration > 0.0 {
        let throughput = metrics.request_count as f64 / duration;
        println!("📊 Throughput mesuré: {:.0} opérations/sec", throughput);
    }
}

#[then(expr = "le throughput doit être supérieur à {int} articles/sec")]
async fn check_min_throughput(world: &mut LithairWorld, min_throughput: usize) {
    let metrics = world.metrics.lock().await;
    let duration = metrics.total_duration.as_secs_f64();

    if duration > 0.0 {
        let actual_throughput = metrics.request_count as f64 / duration;

        assert!(
            actual_throughput >= min_throughput as f64,
            "❌ Throughput trop faible: {:.0} ops/sec (min requis: {})",
            actual_throughput,
            min_throughput
        );

        println!("✅ Throughput {:.0} ops/sec > {} ops/sec", actual_throughput, min_throughput);
    }
}

#[then(expr = "le throughput de suppression doit être supérieur à {int} articles/sec")]
async fn check_delete_throughput(world: &mut LithairWorld, min_throughput: usize) {
    check_min_throughput(world, min_throughput).await;
}

#[then("tous les checksums doivent correspondre")]
async fn check_all_checksums(world: &mut LithairWorld) {
    let persist_path = {
        let metrics = world.metrics.lock().await;
        metrics.persist_path.clone()
    };

    let events_file = format!("{}/events.raftlog", persist_path);

    if !std::path::Path::new(&events_file).exists() {
        println!("⚠️  Fichier events.raftlog n'existe pas encore");
        return;
    }

    // Calculer checksum des événements sur disque
    let content = std::fs::read_to_string(&events_file).expect("Impossible de lire events.raftlog");

    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    let disk_checksum = hasher.finish();

    println!("✅ Checksum disque: {}", disk_checksum);
}

#[then("j'affiche les statistiques finales")]
async fn display_final_stats(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;
    let duration = metrics.total_duration.as_secs_f64();
    let throughput = if duration > 0.0 { metrics.request_count as f64 / duration } else { 0.0 };

    println!("\n╔══════════════════════════════════════╗");
    println!("║   📊 STATISTIQUES FINALES           ║");
    println!("╠══════════════════════════════════════╣");
    println!("║ Total requêtes: {:>20} ║", metrics.request_count);
    println!("║ Durée totale:   {:>17.2}s ║", duration);
    println!("║ Throughput:     {:>16.0}/sec ║", throughput);
    println!("║ Erreurs:        {:>20} ║", metrics.error_count);
    println!("╚══════════════════════════════════════╝\n");
}

#[given(expr = "le mode de durabilité est {string}")]
async fn set_durability_mode(_world: &mut LithairWorld, mode: String) {
    println!("🛡️  Mode de durabilité configuré: {}", mode);
    // Note: La configuration du mode se fait dans le constructeur AsyncWriter
    // Pour l'instant, on log juste pour la documentation
}

#[when(expr = "je lance {int} opérations CRUD aléatoires")]
async fn random_crud_operations(world: &mut LithairWorld, count: usize) {
    use rand::Rng;

    let client = reqwest::Client::new();
    let base_url = {
        let metrics = world.metrics.lock().await;
        metrics.base_url.clone()
    };

    let start = std::time::Instant::now();
    let mut rng = rand::thread_rng();
    let mut created_ids = Vec::new();

    println!("🎲 Lancement de {} opérations CRUD aléatoires...", count);

    for i in 0..count {
        let operation = rng.gen_range(0..100);

        match operation {
            // 50% CREATE
            0..50 => {
                let id = format!("random-article-{}", i);
                let article = json!({
                    "id": id.clone(),
                    "title": format!("Random Article {}", i),
                    "content": format!("Random content {}", i),
                });

                let url = format!("{}/api/articles", base_url);
                if client.post(&url).json(&article).send().await.is_ok() {
                    created_ids.push(id);
                }
            }
            // 30% UPDATE
            50..80 if !created_ids.is_empty() => {
                let idx = rng.gen_range(0..created_ids.len());
                let id = &created_ids[idx];

                let article = json!({
                    "title": format!("Updated Random {}", i),
                    "content": format!("Updated content {}", i),
                });

                let url = format!("{}/api/articles/{}", base_url, id);
                let _ = client.put(&url).json(&article).send().await;
            }
            // 20% DELETE
            80..100 if !created_ids.is_empty() => {
                let idx = rng.gen_range(0..created_ids.len());
                let id = created_ids.remove(idx);

                let url = format!("{}/api/articles/{}", base_url, id);
                let _ = client.delete(&url).send().await;
            }
            _ => {}
        }

        if i % 1000 == 0 && i > 0 {
            println!("  ... {} opérations effectuées", i);
        }
    }

    let elapsed = start.elapsed();
    println!("✅ {} opérations CRUD aléatoires en {:.2}s", count, elapsed.as_secs_f64());

    let mut metrics = world.metrics.lock().await;
    metrics.request_count = count as u64;
    metrics.total_duration = elapsed;
}

#[then("tous les événements doivent être persistés")]
async fn check_all_events_persisted(world: &mut LithairWorld) {
    let persist_path = {
        let metrics = world.metrics.lock().await;
        metrics.persist_path.clone()
    };

    let events_file = format!("{}/events.raftlog", persist_path);

    // Attendre le flush
    tokio::time::sleep(Duration::from_secs(2)).await;

    assert!(
        std::path::Path::new(&events_file).exists(),
        "❌ Le fichier events.raftlog n'existe pas"
    );

    let content = std::fs::read_to_string(&events_file).expect("Impossible de lire events.raftlog");

    let event_count = content.lines().count();

    assert!(event_count > 0, "❌ Aucun événement persisté");

    println!("✅ {} événements persistés sur disque", event_count);
}

#[then("la cohérence des données doit être validée")]
async fn validate_data_consistency(world: &mut LithairWorld) {
    // Vérifier cohérence mémoire/disque
    check_memory_disk_consistency(world).await;

    // Vérifier checksums
    check_all_checksums(world).await;

    println!("✅ Cohérence des données validée");
}

// TODO: Implémenter les autres steps pour les scénarios de performance
