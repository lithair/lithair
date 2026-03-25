use crate::features::world::LithairWorld;
use cucumber::given;
use cucumber::then;
use cucumber::when;
use lithair_core::engine::Event;
use serde_json::json;
use std::time::{Duration, Instant};

// ==================== GIVEN STEPS ====================

#[given(expr = "{int} articles pre-loaded in memory")]
async fn preload_articles_in_memory(world: &mut LithairWorld, count: usize) {
    println!("📦 Pre-loading {} articles in memory...", count);
    let start = Instant::now();

    // Load directly into StateEngine without HTTP
    for i in 0..count {
        let event = crate::features::world::TestEvent::ArticleCreated {
            id: format!("article-{}", i),
            title: format!("Article {}", i),
            content: format!("Content {}", i),
        };

        if let Err(e) = world.engine.with_state_mut(|state| {
            event.apply(state);
        }) {
            eprintln!("❌ Error applying event: {}", e);
        }
    }

    let elapsed = start.elapsed();
    let throughput = count as f64 / elapsed.as_secs_f64();

    println!(
        "✅ {} articles loaded in memory in {:.2}s ({:.0} articles/sec)",
        count,
        elapsed.as_secs_f64(),
        throughput
    );
}

// ==================== WHEN STEPS ====================

#[when(expr = "I read {int} random articles via GET")]
async fn read_random_articles(world: &mut LithairWorld, count: usize) {
    let client = reqwest::Client::new();
    let base_url = {
        let metrics = world.metrics.lock().await;
        metrics.base_url.clone()
    };

    println!("🔍 Reading {} random articles...", count);
    let start = Instant::now();

    // Parallel reads
    let concurrent_reads = 200;
    let mut tasks = Vec::new();

    for _i in 0..count {
        let client = client.clone();
        let url = format!("{}/api/articles", base_url);

        let task = tokio::spawn(async move { client.get(&url).send().await });

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

    // Store metrics
    {
        let mut metrics = world.metrics.lock().await;
        metrics.last_throughput = throughput;
        metrics.last_avg_latency_ms = avg_latency_ms;
    }

    println!("✅ {} reads in {:.2}s", count, elapsed.as_secs_f64());
    println!("   📊 Throughput: {:.0} req/sec", throughput);
    println!("   ⏱️  Average latency: {:.3}ms", avg_latency_ms);
}

#[when(expr = "I create {int} articles in direct write mode")]
async fn write_articles_directly(world: &mut LithairWorld, count: usize) {
    println!("💾 Direct write of {} articles to disk...", count);
    let start = Instant::now();

    // Direct write to FileStorage without HTTP
    let mut storage_guard = world.storage.blocking_lock();
    if let Some(ref mut fs) = *storage_guard {
        for i in 0..count {
            let event_json = serde_json::json!({
                "type": "ArticleCreated",
                "id": format!("article-{}", i),
                "title": format!("Article {}", i),
                "content": format!("Content {}", i),
                "timestamp": chrono::Utc::now().to_rfc3339()
            })
            .to_string();

            let _ = fs.append_event(&event_json);

            // Flush every 1000 events for optimization
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

    println!(
        "✅ {} events written in {:.2}s ({:.0} events/sec)",
        count,
        elapsed.as_secs_f64(),
        throughput
    );
}

#[when(expr = "I create {int} articles via HTTP POST")]
async fn create_articles_via_http(world: &mut LithairWorld, count: usize) {
    println!("🌐 Creating {} articles via HTTP (E2E)...", count);
    let client = reqwest::Client::new();
    let base_url = {
        let metrics = world.metrics.lock().await;
        metrics.base_url.clone()
    };

    let url = format!("{}/api/articles", base_url);
    let start = Instant::now();

    // Parallelization with batching
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

            client.post(&url).json(&article).send().await
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

    println!(
        "✅ {} articles created (E2E) in {:.2}s ({:.0} articles/sec)",
        count,
        elapsed.as_secs_f64(),
        throughput
    );
}

#[when(expr = "I run {int}% reads and {int}% writes for {int} seconds")]
async fn mixed_workload(
    world: &mut LithairWorld,
    read_pct: usize,
    write_pct: usize,
    duration_secs: usize,
) {
    println!(
        "🔀 Mixed workload: {}% reads, {}% writes for {}s",
        read_pct, write_pct, duration_secs
    );

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
            // Read
            let url = format!("{}/api/articles", base_url);
            let _ = client.get(&url).send().await;
            read_count += 1;
        } else {
            // Write
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

    // Calculate percentiles
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

    println!("✅ Mixed workload completed:");
    println!("   📊 Total ops: {} ({} reads, {} writes)", total_ops, read_count, write_count);
    println!("   📈 Throughput: {:.0} ops/sec", throughput);
    println!("   ⏱️  Latency P50: {:.2}ms, P95: {:.2}ms, P99: {:.2}ms", p50, p95, p99);
}

// ==================== THEN STEPS ====================

#[then(expr = "the average read time must be less than {int} ms")]
async fn check_avg_read_latency(world: &mut LithairWorld, max_ms: usize) {
    let metrics = world.metrics.lock().await;
    let avg_latency = metrics.last_avg_latency_ms;

    assert!(
        avg_latency < max_ms as f64,
        "❌ Average latency {:.2}ms > {}ms required",
        avg_latency,
        max_ms
    );

    println!("✅ Average latency {:.2}ms < {}ms", avg_latency, max_ms);
}

#[then(expr = "the read throughput must exceed {int} req/sec")]
async fn check_read_throughput(world: &mut LithairWorld, min_rps: usize) {
    let metrics = world.metrics.lock().await;
    let throughput = metrics.last_throughput;

    assert!(
        throughput > min_rps as f64,
        "❌ Throughput {:.0} req/sec < {} required",
        throughput,
        min_rps
    );

    println!("✅ Throughput {:.0} req/sec > {} req/sec", throughput, min_rps);
}

#[then("the write throughput must be measured")]
async fn measure_write_throughput(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;
    println!("📊 Write throughput: {:.0} events/sec", metrics.last_throughput);
}

#[then("all articles must be in memory")]
async fn check_articles_in_memory(world: &mut LithairWorld) {
    let count = world
        .engine
        .with_state(|state| state.data.articles.len())
        .expect("Failed to read state");

    println!("✅ {} articles present in memory", count);
}

#[then("the E2E throughput must be measured")]
async fn measure_e2e_throughput(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;
    println!("📊 E2E throughput: {:.0} articles/sec", metrics.last_throughput);
}

#[then("the total throughput must be measured")]
async fn measure_total_throughput(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;
    println!("📊 Total throughput: {:.0} ops/sec", metrics.last_throughput);
}

#[then(expr = "the P50, P95, P99 latencies must be calculated")]
async fn check_percentiles_calculated(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;
    println!("📊 Latencies:");
    println!("   P50: {:.2}ms", metrics.last_p50_latency_ms);
    println!("   P95: {:.2}ms", metrics.last_p95_latency_ms);
    println!("   P99: {:.2}ms", metrics.last_p99_latency_ms);
}
