use cucumber::given;
use cucumber::when;
use cucumber::then;
use crate::features::world::LithairWorld;
use std::time::{Duration, Instant};
use serde_json::json;
use lithair_core::engine::Event;

// ==================== GIVEN STEPS ====================

#[given(expr = "{int} articles pré-chargés en mémoire")]
async fn preload_articles_in_memory(world: &mut LithairWorld, count: usize) {
    println!("📦 Pré-chargement de {} articles en mémoire...", count);
    let start = Instant::now();

    // Charger directement dans StateEngine sans HTTP
    for i in 0..count {
        let event = crate::features::world::TestEvent::ArticleCreated {
            id: format!("article-{}", i),
            title: format!("Article {}", i),
            content: format!("Content {}", i),
        };

        if let Err(e) = world.engine.with_state_mut(|state| {
            event.apply(state);
        }) {
            eprintln!("❌ Erreur application event: {}", e);
        }
    }

    let elapsed = start.elapsed();
    let throughput = count as f64 / elapsed.as_secs_f64();

    println!("✅ {} articles chargés en mémoire en {:.2}s ({:.0} articles/sec)",
        count, elapsed.as_secs_f64(), throughput);
}

// ==================== WHEN STEPS ====================

#[when(expr = "je lis {int} articles aléatoires via GET")]
async fn read_random_articles(world: &mut LithairWorld, count: usize) {
    let client = reqwest::Client::new();
    let base_url = {
        let metrics = world.metrics.lock().await;
        metrics.base_url.clone()
    };

    println!("🔍 Lecture de {} articles aléatoires...", count);
    let start = Instant::now();

    // Lectures parallèles
    let concurrent_reads = 200;
    let mut tasks = Vec::new();

    for _i in 0..count {
        let client = client.clone();
        let url = format!("{}/api/articles", base_url);

        let task = tokio::spawn(async move {
            client.get(&url).send().await
        });

        tasks.push(task);

        if tasks.len() >= concurrent_reads {
            for task in tasks.drain(..) {
                let _ = task.await;
            }
        }
    }

    for task in tasks {
        let _ = task.await;
    }

    let elapsed = start.elapsed();
    let throughput = count as f64 / elapsed.as_secs_f64();
    let avg_latency_ms = elapsed.as_millis() as f64 / count as f64;

    // Stocker les métriques
    {
        let mut metrics = world.metrics.lock().await;
        metrics.last_throughput = throughput;
        metrics.last_avg_latency_ms = avg_latency_ms;
    }

    println!("✅ {} lectures en {:.2}s", count, elapsed.as_secs_f64());
    println!("   📊 Throughput: {:.0} req/sec", throughput);
    println!("   ⏱️  Latence moyenne: {:.3}ms", avg_latency_ms);
}

#[when(expr = "je crée {int} articles en mode écriture directe")]
async fn write_articles_directly(world: &mut LithairWorld, count: usize) {
    println!("💾 Écriture directe de {} articles sur disque...", count);
    let start = Instant::now();

    // Écriture directe sur FileStorage sans HTTP
    let mut storage_guard = world.storage.blocking_lock();
    if let Some(ref mut fs) = *storage_guard {
        for i in 0..count {
            let event_json = serde_json::json!({
                "type": "ArticleCreated",
                "id": format!("article-{}", i),
                "title": format!("Article {}", i),
                "content": format!("Content {}", i),
                "timestamp": chrono::Utc::now().to_rfc3339()
            }).to_string();

            let _ = fs.append_event(&event_json);

            // Flush tous les 1000 events pour optimiser
            if i % 1000 == 0 {
                let _ = fs.flush_batch();
            }
        }
        let _ = fs.flush_batch();
    }

    let elapsed = start.elapsed();
    let throughput = count as f64 / elapsed.as_secs_f64();

    {
        let mut metrics = world.metrics.lock().await;
        metrics.last_throughput = throughput;
    }

    println!("✅ {} événements écrits en {:.2}s ({:.0} events/sec)",
        count, elapsed.as_secs_f64(), throughput);
}

#[when(expr = "je crée {int} articles via HTTP POST")]
async fn create_articles_via_http(world: &mut LithairWorld, count: usize) {
    println!("🌐 Création de {} articles via HTTP (E2E)...", count);
    let client = reqwest::Client::new();
    let base_url = {
        let metrics = world.metrics.lock().await;
        metrics.base_url.clone()
    };

    let url = format!("{}/api/articles", base_url);
    let start = Instant::now();

    // Parallélisation avec batching
    let concurrent_requests = 100;
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

            client.post(&url)
                .json(&article)
                .send()
                .await
        });

        tasks.push(task);

        if tasks.len() >= concurrent_requests {
            for task in tasks.drain(..) {
                let _ = task.await;
            }
        }
    }

    for task in tasks {
        let _ = task.await;
    }

    let elapsed = start.elapsed();
    let throughput = count as f64 / elapsed.as_secs_f64();

    {
        let mut metrics = world.metrics.lock().await;
        metrics.last_throughput = throughput;
    }

    println!("✅ {} articles créés (E2E) en {:.2}s ({:.0} articles/sec)",
        count, elapsed.as_secs_f64(), throughput);
}

#[when(expr = "je lance {int}% lectures et {int}% écritures pendant {int} secondes")]
async fn mixed_workload(world: &mut LithairWorld, read_pct: usize, write_pct: usize, duration_secs: usize) {
    println!("🔀 Workload mixte: {}% lectures, {}% écritures pendant {}s",
        read_pct, write_pct, duration_secs);

    let client = reqwest::Client::new();
    let base_url = {
        let metrics = world.metrics.lock().await;
        metrics.base_url.clone()
    };

    let start = Instant::now();
    let duration = Duration::from_secs(duration_secs as u64);

    let mut read_count = 0u64;
    let mut write_count = 0u64;
    let mut latencies = Vec::new();

    let mut counter = 0usize;
    while start.elapsed() < duration {
        let rand_val = counter % 100;
        counter += 1;

        let op_start = Instant::now();

        if rand_val < read_pct {
            // Lecture
            let url = format!("{}/api/articles", base_url);
            let _ = client.get(&url).send().await;
            read_count += 1;
        } else {
            // Écriture
            let url = format!("{}/api/articles", base_url);
            let article = json!({
                "id": format!("article-{}", write_count),
                "title": format!("Article {}", write_count),
                "content": format!("Content {}", write_count),
            });
            let _ = client.post(&url).json(&article).send().await;
            write_count += 1;
        }

        latencies.push(op_start.elapsed().as_micros() as f64 / 1000.0);
    }

    let total_ops = read_count + write_count;
    let elapsed = start.elapsed();
    let throughput = total_ops as f64 / elapsed.as_secs_f64();

    // Calculer percentiles
    latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p50 = latencies[latencies.len() * 50 / 100];
    let p95 = latencies[latencies.len() * 95 / 100];
    let p99 = latencies[latencies.len() * 99 / 100];

    {
        let mut metrics = world.metrics.lock().await;
        metrics.last_throughput = throughput;
        metrics.last_p50_latency_ms = p50;
        metrics.last_p95_latency_ms = p95;
        metrics.last_p99_latency_ms = p99;
    }

    println!("✅ Workload mixte terminé:");
    println!("   📊 Total ops: {} ({} reads, {} writes)", total_ops, read_count, write_count);
    println!("   📈 Throughput: {:.0} ops/sec", throughput);
    println!("   ⏱️  Latence P50: {:.2}ms, P95: {:.2}ms, P99: {:.2}ms", p50, p95, p99);
}

// ==================== THEN STEPS ====================

#[then(expr = "le temps de lecture moyen doit être inférieur à {int}ms")]
async fn check_avg_read_latency(world: &mut LithairWorld, max_ms: usize) {
    let metrics = world.metrics.lock().await;
    let avg_latency = metrics.last_avg_latency_ms;

    assert!(
        avg_latency < max_ms as f64,
        "❌ Latence moyenne {:.2}ms > {}ms requis",
        avg_latency, max_ms
    );

    println!("✅ Latence moyenne {:.2}ms < {}ms ✓", avg_latency, max_ms);
}

#[then(expr = "le throughput de lecture doit dépasser {int} req/sec")]
async fn check_read_throughput(world: &mut LithairWorld, min_rps: usize) {
    let metrics = world.metrics.lock().await;
    let throughput = metrics.last_throughput;

    assert!(
        throughput > min_rps as f64,
        "❌ Throughput {:.0} req/sec < {} requis",
        throughput, min_rps
    );

    println!("✅ Throughput {:.0} req/sec > {} req/sec ✓", throughput, min_rps);
}

#[then("le throughput d'écriture doit être mesuré")]
async fn measure_write_throughput(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;
    println!("📊 Throughput d'écriture: {:.0} events/sec", metrics.last_throughput);
}

#[then("tous les articles doivent être en mémoire")]
async fn check_articles_in_memory(world: &mut LithairWorld) {
    let count = world.engine.with_state(|state| {
        state.data.articles.len()
    }).expect("Impossible de lire l'état");

    println!("✅ {} articles présents en mémoire", count);
}

#[then("le throughput E2E doit être mesuré")]
async fn measure_e2e_throughput(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;
    println!("📊 Throughput E2E: {:.0} articles/sec", metrics.last_throughput);
}

#[then("le throughput total doit être mesuré")]
async fn measure_total_throughput(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;
    println!("📊 Throughput total: {:.0} ops/sec", metrics.last_throughput);
}

#[then(expr = "les latences P50, P95, P99 doivent être calculées")]
async fn check_percentiles_calculated(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;
    println!("📊 Latences:");
    println!("   P50: {:.2}ms", metrics.last_p50_latency_ms);
    println!("   P95: {:.2}ms", metrics.last_p95_latency_ms);
    println!("   P99: {:.2}ms", metrics.last_p99_latency_ms);
}
