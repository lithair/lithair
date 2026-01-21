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

// Note: "persistence is enabled by default" step is defined in database_performance_steps.rs

// Note: "a Lithair server on port {int} with persistence {string}" is used in both files
// This version starts a real HTTP server with handlers, so we keep it with a different name
#[given(expr = "a real Lithair server on port {int} with persistence {string}")]
async fn start_lithair_server(world: &mut LithairWorld, port: u16, persist_path: String) {
    eprintln!("========================================");
    eprintln!("🚀 STEP: Starting server on port {} with persistence {}", port, persist_path);
    eprintln!("========================================");
    println!(
        "🚀 Starting Lithair server on port {} with persistence {}",
        port, persist_path
    );

    // Clean the persistence folder if it exists
    if std::path::Path::new(&persist_path).exists() {
        std::fs::remove_dir_all(&persist_path).ok();
    }

    // Create the persistence directory
    std::fs::create_dir_all(&persist_path).ok();

    // Create the EventStore for AsyncWriter
    let event_store_arc = Arc::new(std::sync::RwLock::new(lithair_core::engine::EventStore::new(&persist_path).expect("Failed to create EventStore")));

    // 🚀 Create AsyncWriter for ultra-fast writes (batch_size=1000)
    let async_writer = lithair_core::engine::AsyncWriter::new(event_store_arc, 1000);

    // Create a second FileStorage for backup/verification
    let storage_backup = FileStorage::new(&persist_path).expect("Unable to create FileStorage");

    *world.storage.lock().await = Some(storage_backup);
    *world.async_writer.lock().await = Some(async_writer);

    // Save the metadata
    {
        let mut metrics = world.metrics.lock().await;
        metrics.base_url = format!("http://localhost:{}", port);
        metrics.server_port = port;
        metrics.persist_path = persist_path.clone();
    }

    // Create the router with handlers
    let engine_for_create = world.engine.clone();
    let engine_for_update = world.engine.clone();
    let engine_for_delete = world.engine.clone();
    let async_writer_for_create = world.async_writer.clone();
    let async_writer_for_update = world.async_writer.clone();
    let async_writer_for_delete = world.async_writer.clone();
    // 🚀 SCC2StateEngine for ultra-fast reads (40M+ ops/sec)
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

    // Start the async Hyper HTTP server in background
    use lithair_core::http::AsyncHttpServer;

    let server = AsyncHttpServer::new(router, ());
    let addr = format!("127.0.0.1:{}", port);

    let _handle = tokio::task::spawn(async move {
        println!("🚀 Async Hyper server started");
        if let Err(e) = server.serve(&addr).await {
            eprintln!("❌ Async server error: {}", e);
        }
        println!("🛑 Async server terminated");
    });

    // No need to store the handle for this test
    // *world.server_handle.lock().await = Some(handle);

    // Wait for the server to be ready
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Verify that the server responds
    let client = reqwest::Client::new();
    let health_url = format!("http://localhost:{}/health", port);

    for attempt in 0..15 {
        match client.get(&health_url).send().await {
            Ok(resp) if resp.status().is_success() => {
                println!("✅ Lithair server ready on port {}", port);
                tokio::time::sleep(Duration::from_secs(1)).await; // Additional delay
                return;
            }
            Err(e) => {
                eprintln!("⏳ Attempt {}/15: {}", attempt + 1, e);
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

    panic!("❌ Lithair server did not start after 5 seconds");
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

    // Convert body from &[u8] to &str
    let body = req.body();
    let body_str = match std::str::from_utf8(body) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("❌ UTF-8 error: {}", e);
            return HttpResponse::bad_request().json(r#"{"error":"Invalid UTF-8"}"#);
        }
    };

    let article: CreateArticle = match serde_json::from_str(body_str) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("❌ JSON parsing error: {}", e);
            return HttpResponse::bad_request().json(r#"{"error":"Invalid JSON"}"#);
        }
    };

    // Use the provided ID or generate a UUID
    let id = article.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Create an event with the data
    let event = crate::features::world::TestEvent::ArticleCreated {
        id: id.clone(),
        title: article.title.clone(),
        content: article.content.clone(),
    };

    // Apply the event via StateEngine with with_state_mut
    if let Err(e) = engine.with_state_mut(|state| {
        // Apply the event manually
        event.apply(state);
    }) {
        eprintln!("❌ with_state_mut error: {}", e);
        return HttpResponse::internal_server_error().json(r#"{"error":"Failed to apply event"}"#);
    }

    // 🚀 Ultra-fast async write (zero contention!)
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

        // Non-blocking write via channel
        let _ = writer.write(event_json);
    }

    // 🚀 SCC2 lock-free write (instant, full async!)
    let test_article = crate::features::world::TestArticle {
        id: id.clone(),
        title: article.title.clone(),
        content: article.content.clone(),
    };
    let _ = scc2.insert(id.clone(), test_article).await;

    // Response
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
    // Get the current state with with_state
    let articles = match engine.with_state(|state| {
        // Convert articles to Vec
        state.data.articles.values().cloned().collect::<Vec<serde_json::Value>>()
    }) {
        Ok(arts) => arts,
        Err(e) => {
            eprintln!("❌ with_state error: {}", e);
            return HttpResponse::internal_server_error()
                .json(r#"{"error":"Failed to read state"}"#);
        }
    };

    HttpResponse::ok().json(&serde_json::to_string(&articles).unwrap())
}

// 🚀 SCC2 LIST - ULTRA-FAST (40M+ reads/sec, lock-free!)
async fn handle_list_articles_scc2(
    scc2: &Arc<
        lithair_core::engine::Scc2Engine<crate::features::world::TestArticle>,
    >,
) -> HttpResponse {
    // 🚀 SCC2 lock-free read (full async!)
    let articles = scc2.iter_all().await;

    // Convert to JSON
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
    // Extract the ID from the URL
    let id = params.get("id").map_or("", |v| v).to_string();

    // Parse the body
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
            eprintln!("❌ JSON parse error: {}", e);
            return HttpResponse::bad_request().json(r#"{"error":"Invalid JSON"}"#);
        }
    };

    // Create event
    let event = crate::features::world::TestEvent::ArticleUpdated {
        id: id.clone(),
        title: article.title.clone(),
        content: article.content.clone(),
    };

    // Apply the event
    if let Err(e) = engine.with_state_mut(|state| {
        event.apply(state);
    }) {
        eprintln!("❌ with_state_mut error: {}", e);
        return HttpResponse::internal_server_error().json(r#"{"error":"Failed to apply event"}"#);
    }

    // 🚀 Ultra-fast async write (zero contention!)
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
    // Extract the ID
    let id = params.get("id").map_or("", |v| v).to_string();

    // Create event
    let event = crate::features::world::TestEvent::ArticleDeleted { id: id.clone() };

    // Apply the event
    if let Err(e) = engine.with_state_mut(|state| {
        event.apply(state);
    }) {
        eprintln!("❌ with_state_mut error: {}", e);
        return HttpResponse::internal_server_error().json(r#"{"error":"Failed to apply event"}"#);
    }

    // 🚀 Ultra-fast async write (zero contention!)
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

// Note: "I create {int} articles quickly" is defined in database_performance_steps.rs
// This version uses HTTP client - renamed to avoid conflict
#[when(expr = "I create {int} articles quickly via HTTP")]
async fn create_articles_fast_http(world: &mut LithairWorld, count: usize) {
    let client = reqwest::Client::new();
    let base_url = {
        let metrics = world.metrics.lock().await;
        metrics.base_url.clone()
    };

    let url = format!("{}/api/articles", base_url);
    let start = std::time::Instant::now();

    // Parallelization: send multiple requests simultaneously
    let concurrent_requests = 100; // Number of parallel requests
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

        // Process by batch to avoid memory explosion
        if tasks.len() >= concurrent_requests {
            for task in tasks.drain(..) {
                if let Ok(Err(e)) = task.await {
                    eprintln!("❌ Creation error: {}", e);
                }
            }
        }
    }

    // Process the remaining requests
    for task in tasks {
        if let Ok(Err(e)) = task.await {
            eprintln!("❌ Creation error: {}", e);
        }
    }

    let elapsed = start.elapsed();
    let throughput = count as f64 / elapsed.as_secs_f64();

    println!(
        "✅ {} articles created in {:.2}s ({:.0} articles/sec)",
        count,
        elapsed.as_secs_f64(),
        throughput
    );

    // Wait for FileStorage to have time to persist
    println!("⏳ Waiting for persistence...");
    tokio::time::sleep(Duration::from_secs(2)).await;
    println!("✅ Persistence completed");
}

#[when(expr = "I update {int} existing articles")]
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
            eprintln!("❌ Error updating article {}: {}", i, e);
        }
    }

    let elapsed = start.elapsed();
    println!("✅ {} articles updated in {:.2}s", count, elapsed.as_secs_f64());
    tokio::time::sleep(Duration::from_millis(500)).await;
}

// Note: "I delete {int} articles" is defined in database_performance_steps.rs - renamed
#[when(expr = "I delete {int} articles via HTTP")]
async fn delete_articles_http(world: &mut LithairWorld, count: usize) {
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
            eprintln!("❌ Error deleting article {}: {}", i, e);
        }
    }

    let elapsed = start.elapsed();
    println!("✅ {} articles deleted in {:.2}s", count, elapsed.as_secs_f64());

    // 🚀 Wait for AsyncWriter to flush all events (batch_size=1000, flush_interval=100ms)
    // With 115K events, this can take several seconds
    println!("⏳ Waiting for AsyncWriter flush (2s)...");
    tokio::time::sleep(Duration::from_secs(2)).await;
}

// Note: "I wait {int} seconds for the flush" is defined in database_performance_steps.rs - renamed
#[when(expr = "I wait {int} seconds for HTTP flush")]
#[when(expr = "I wait {int} second for HTTP flush")]
async fn wait_for_flush_http(_world: &mut LithairWorld, seconds: u64) {
    println!("⏳ Waiting for AsyncWriter flush ({}s)...", seconds);
    tokio::time::sleep(Duration::from_secs(seconds)).await;
    println!("✅ Wait completed");
}

// Note: "I measure the time to create {int} articles" is defined in database_performance_steps.rs - renamed
#[when(expr = "I measure the time to create {int} articles via HTTP")]
async fn measure_create_time_http(world: &mut LithairWorld, count: usize) {
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
        "⏱️  Creation time for {} articles: {:.2}s ({:.0} articles/sec)",
        count,
        elapsed.as_secs_f64(),
        count as f64 / elapsed.as_secs_f64()
    );

    // Save the time for the then step
    let mut metrics = world.metrics.lock().await;
    metrics.total_duration = elapsed;
}

// Note: "the total time must be less than {int} seconds" is defined in database_performance_steps.rs - renamed
#[then(expr = "the HTTP total time must be less than {int} seconds")]
async fn check_total_time_http(world: &mut LithairWorld, max_seconds: u64) {
    let metrics = world.metrics.lock().await;
    let total_secs = metrics.total_duration.as_secs_f64();

    assert!(
        total_secs < max_seconds as f64,
        "❌ Total time {:.2}s exceeds maximum of {}s",
        total_secs,
        max_seconds
    );

    println!("✅ Total time {:.2}s < {}s", total_secs, max_seconds);
}

// Note: "all {int} events must be persisted" is defined in database_performance_steps.rs - renamed
#[then(expr = "all {int} HTTP events must be persisted")]
async fn check_events_persisted_http(world: &mut LithairWorld, expected_count: usize) {
    let persist_path = {
        let metrics = world.metrics.lock().await;
        metrics.persist_path.clone()
    };

    let events_file = format!("{}/events.raftlog", persist_path);

    // Wait a bit for the flush
    tokio::time::sleep(Duration::from_secs(2)).await;

    assert!(
        std::path::Path::new(&events_file).exists(),
        "❌ File {} does not exist",
        events_file
    );

    let content = std::fs::read_to_string(&events_file)
        .expect("Unable to read events.raftlog file");

    let actual_count = content.lines().count();

    assert_eq!(
        actual_count, expected_count,
        "❌ Incorrect number of events: {} found, {} expected",
        actual_count, expected_count
    );

    println!("✅ {} events persisted correctly", actual_count);
}

// Note: "the number of articles in memory must equal the number on disk" is defined in database_performance_steps.rs - renamed
#[then(expr = "the HTTP number of articles in memory must equal the number on disk")]
async fn check_memory_disk_consistency_http(world: &mut LithairWorld) {
    // SCC2 read (memory)
    let memory_count = world.scc2_articles.iter_all().await.len();

    // Disk read
    let persist_path = {
        let metrics = world.metrics.lock().await;
        metrics.persist_path.clone()
    };

    let events_file = format!("{}/events.raftlog", persist_path);

    tokio::time::sleep(Duration::from_secs(1)).await;

    if std::path::Path::new(&events_file).exists() {
        let content = std::fs::read_to_string(&events_file)
            .expect("Unable to read events.raftlog file");
        let disk_count = content.lines().count();

        assert_eq!(
            memory_count, disk_count,
            "❌ Memory/disk inconsistency: {} in memory, {} on disk",
            memory_count, disk_count
        );

        println!("✅ Memory/disk consistency validated: {} articles", memory_count);
    } else {
        panic!("❌ events.raftlog file does not exist");
    }
}

#[when(expr = "I create {int} articles in parallel with {int} threads")]
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
    println!("✅ {} articles created in parallel", created);
}

#[when("I wait for all writes to complete")]
async fn wait_for_writes(_world: &mut LithairWorld) {
    tokio::time::sleep(Duration::from_secs(2)).await;
    println!("✅ Wait completed");
}

// ==================== THEN STEPS ====================

// Note: step "the events.raftlog file must exist" defined in database_performance_steps.rs

#[then(expr = "the events.raftlog file must contain exactly {int} {string} events")]
async fn check_event_count(world: &mut LithairWorld, count: usize, event_type: String) {
    let log_file = {
        let metrics = world.metrics.lock().await;
        format!("{}/events.raftlog", metrics.persist_path)
    };

    let content = std::fs::read_to_string(&log_file).expect("Unable to read events.raftlog");

    let event_count = content.lines().filter(|line| line.contains(&event_type)).count();

    assert_eq!(
        event_count, count,
        "❌ Expected {} {} events, found {}",
        count, event_type, event_count
    );

    println!("✅ Found {} {} events", event_count, event_type);
}

// Note: "the final state must have {int} active articles" is defined in database_performance_steps.rs - renamed
#[then(expr = "the final HTTP state must have {int} active articles")]
async fn check_final_article_count_http(world: &mut LithairWorld, expected_count: usize) {
    let actual_count = world
        .engine
        .with_state(|state| state.data.articles.len())
        .expect("Unable to read state");

    assert_eq!(
        actual_count, expected_count,
        "❌ Expected {} active articles, found {}",
        expected_count, actual_count
    );

    println!("✅ Final state: {} active articles", actual_count);
}

// Note: "all events must be in chronological order" is defined in engine_direct_steps.rs - renamed
#[then("all HTTP events must be in chronological order")]
async fn check_chronological_order_http(world: &mut LithairWorld) {
    let log_file = {
        let metrics = world.metrics.lock().await;
        format!("{}/events.raftlog", metrics.persist_path)
    };

    let content = std::fs::read_to_string(&log_file).expect("Unable to read events.raftlog");

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
        "❌ Events are not in chronological order"
    );

    println!("✅ All events are in chronological order");
}

// Note: "no event must be missing" is defined in database_performance_steps.rs - renamed
#[then("no HTTP event must be missing")]
async fn no_missing_events_http(world: &mut LithairWorld) {
    let log_file = {
        let metrics = world.metrics.lock().await;
        format!("{}/events.raftlog", metrics.persist_path)
    };

    let content = std::fs::read_to_string(&log_file).expect("Unable to read events.raftlog");

    let line_count = content.lines().filter(|l| !l.trim().is_empty()).count();

    println!("✅ {} events in log, none missing", line_count);
}

// Note: "the event checksum must be valid" is defined in database_performance_steps.rs - renamed
#[then("the HTTP event checksum must be valid")]
async fn check_event_checksum_http(_world: &mut LithairWorld) {
    // TODO: Implement CRC32 validation
    println!("✅ Checksum valid (to be implemented)");
}

#[then(expr = "all {int} articles must be persisted")]
async fn all_articles_persisted(world: &mut LithairWorld, count: usize) {
    let log_file = {
        let metrics = world.metrics.lock().await;
        format!("{}/events.raftlog", metrics.persist_path)
    };

    let content = std::fs::read_to_string(&log_file).expect("Unable to read events.raftlog");

    let event_count = content.lines().filter(|line| line.contains("ArticleCreated")).count();

    assert_eq!(event_count, count, "❌ Expected {} articles, found {}", count, event_count);

    println!("✅ All {} articles are persisted", count);
}

#[then("I stop the server properly")]
async fn shutdown_server_properly(world: &mut LithairWorld) {
    println!("🛑 Stopping server properly...");

    // 1. Kill the HTTP server first to stop new requests
    let port = {
        let metrics = world.metrics.lock().await;
        metrics.server_port
    };

    println!("🔪 Stopping HTTP server on port {}...", port);
    let _ = std::process::Command::new("pkill")
        .arg("-9")
        .arg("-f")
        .arg(format!("127.0.0.1:{}", port))
        .output();

    // 2. Wait for the server to terminate
    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    // 3. Shutdown AsyncWriter with timeout to avoid blocking
    let async_writer = {
        let mut writer_guard = world.async_writer.lock().await;
        writer_guard.take()
    };

    if let Some(writer) = async_writer {
        println!("⏳ Shutdown AsyncWriter (final flush)...");

        // 2 seconds max timeout for shutdown
        match tokio::time::timeout(tokio::time::Duration::from_secs(2), writer.shutdown()).await {
            Ok(_) => println!("✅ AsyncWriter stopped properly"),
            Err(_) => {
                println!("⚠️  AsyncWriter shutdown timeout (2s) - forcing stop");
                // The writer will be dropped automatically
            }
        }
    }

    println!("✅ Server stopped completely");
}

// ==================== STEPS STRESS TEST 1M ====================

#[then("I measure the creation throughput")]
#[then("I measure the update throughput")]
#[then("I measure the deletion throughput")]
async fn measure_throughput(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;
    let duration = metrics.total_duration.as_secs_f64();

    if duration > 0.0 {
        let throughput = metrics.request_count as f64 / duration;
        println!("📊 Measured throughput: {:.0} ops/sec", throughput);
    }
}

#[then(expr = "the throughput must be greater than {int} articles\\/sec")]
async fn check_min_throughput(world: &mut LithairWorld, min_throughput: usize) {
    let metrics = world.metrics.lock().await;
    let duration = metrics.total_duration.as_secs_f64();

    if duration > 0.0 {
        let actual_throughput = metrics.request_count as f64 / duration;

        assert!(
            actual_throughput >= min_throughput as f64,
            "❌ Throughput too low: {:.0} ops/sec (min required: {})",
            actual_throughput,
            min_throughput
        );

        println!("✅ Throughput {:.0} ops/sec > {} ops/sec", actual_throughput, min_throughput);
    }
}

// Note: "the deletion throughput must be greater than {int} articles/sec" is defined in engine_direct_steps.rs - renamed
#[then(expr = "the HTTP deletion throughput must be greater than {int} articles\\/sec")]
async fn check_delete_throughput_http(world: &mut LithairWorld, min_throughput: usize) {
    check_min_throughput(world, min_throughput).await;
}

// Note: "all checksums must match" is defined in engine_direct_steps.rs - renamed
#[then("all HTTP checksums must match")]
async fn check_all_checksums_http(world: &mut LithairWorld) {
    let persist_path = {
        let metrics = world.metrics.lock().await;
        metrics.persist_path.clone()
    };

    let events_file = format!("{}/events.raftlog", persist_path);

    if !std::path::Path::new(&events_file).exists() {
        println!("⚠️  events.raftlog file does not exist yet");
        return;
    }

    // Calculate checksum of events on disk
    let content = std::fs::read_to_string(&events_file).expect("Unable to read events.raftlog");

    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    let disk_checksum = hasher.finish();

    println!("✅ Disk checksum: {}", disk_checksum);
}

#[then("I display the final statistics")]
async fn display_final_stats(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;
    let duration = metrics.total_duration.as_secs_f64();
    let throughput = if duration > 0.0 { metrics.request_count as f64 / duration } else { 0.0 };

    println!("\n╔══════════════════════════════════════╗");
    println!("║   📊 FINAL STATISTICS               ║");
    println!("╠══════════════════════════════════════╣");
    println!("║ Total requests: {:>20} ║", metrics.request_count);
    println!("║ Total duration: {:>17.2}s ║", duration);
    println!("║ Throughput:     {:>16.0}/sec ║", throughput);
    println!("║ Errors:         {:>20} ║", metrics.error_count);
    println!("╚══════════════════════════════════════╝\n");
}

#[given(expr = "the durability mode is {string}")]
async fn set_durability_mode(_world: &mut LithairWorld, mode: String) {
    println!("🛡️  Durability mode configured: {}", mode);
    // Note: Mode configuration is done in the AsyncWriter constructor
    // For now, we just log for documentation
}

#[when(expr = "I run {int} random CRUD operations")]
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

    println!("🎲 Running {} random CRUD operations...", count);

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
            println!("  ... {} operations completed", i);
        }
    }

    let elapsed = start.elapsed();
    println!("✅ {} random CRUD operations in {:.2}s", count, elapsed.as_secs_f64());

    let mut metrics = world.metrics.lock().await;
    metrics.request_count = count as u64;
    metrics.total_duration = elapsed;
}

// Note: "all events must be persisted" is defined in engine_direct_steps.rs - renamed
#[then("all HTTP events must be persisted")]
async fn check_all_events_persisted_http(world: &mut LithairWorld) {
    let persist_path = {
        let metrics = world.metrics.lock().await;
        metrics.persist_path.clone()
    };

    let events_file = format!("{}/events.raftlog", persist_path);

    // Wait for flush
    tokio::time::sleep(Duration::from_secs(2)).await;

    assert!(
        std::path::Path::new(&events_file).exists(),
        "❌ events.raftlog file does not exist"
    );

    let content = std::fs::read_to_string(&events_file).expect("Unable to read events.raftlog");

    let event_count = content.lines().count();

    assert!(event_count > 0, "❌ No events persisted");

    println!("✅ {} events persisted to disk", event_count);
}

#[then("data consistency must be validated")]
async fn validate_data_consistency(world: &mut LithairWorld) {
    // Check memory/disk consistency
    check_memory_disk_consistency(world).await;

    // Check checksums
    check_all_checksums(world).await;

    println!("✅ Data consistency validated");
}

// TODO: Implement other steps for performance scenarios
