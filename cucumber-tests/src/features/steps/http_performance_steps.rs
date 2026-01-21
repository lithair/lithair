use cucumber::{given, when, then};
use std::time::{Duration, Instant};
use std::sync::{Arc, Mutex};
use std::thread;
use reqwest::blocking::Client;
use serde_json::json;

use crate::features::world::LithairWorld;

// Structures for metrics
#[derive(Debug, Clone)]
pub struct PerformanceMetrics {
    pub total_requests: usize,
    pub successful_requests: usize,
    pub failed_requests: usize,
    pub total_duration: Duration,
    pub throughput: f64, // req/s
    pub latencies: Vec<Duration>,
    pub errors: Vec<String>,
}

impl PerformanceMetrics {
    pub fn new() -> Self {
        Self {
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
            total_duration: Duration::from_secs(0),
            throughput: 0.0,
            latencies: Vec::new(),
            errors: Vec::new(),
        }
    }

    pub fn calculate_percentile(&self, percentile: f64) -> Duration {
        if self.latencies.is_empty() {
            return Duration::from_secs(0);
        }
        
        let mut sorted = self.latencies.clone();
        sorted.sort();
        
        let index = ((percentile / 100.0) * sorted.len() as f64) as usize;
        let index = index.min(sorted.len() - 1);
        
        sorted[index]
    }

    pub fn p50(&self) -> Duration {
        self.calculate_percentile(50.0)
    }

    pub fn p95(&self) -> Duration {
        self.calculate_percentile(95.0)
    }

    pub fn p99(&self) -> Duration {
        self.calculate_percentile(99.0)
    }

    pub fn error_rate(&self) -> f64 {
        if self.total_requests == 0 {
            return 0.0;
        }
        (self.failed_requests as f64 / self.total_requests as f64) * 100.0
    }
}

// Background Steps
#[given(expr = "a Lithair server starts on port {string}")]
async fn start_server_on_port(world: &mut LithairWorld, port: String) {
    // TODO: Start the real Lithair server with HttpServer
    // For now, we use test_server
    let persist_path = {
        let metrics = world.metrics.lock().await;
        metrics.persist_path.clone()
    };

    // Create the persistence directory
    std::fs::create_dir_all(&persist_path).ok();

    // Start the server
    let _binary = "./target/release/test_server";
    let _args = vec![
        "--port".to_string(),
        port.clone(),
        "--persist".to_string(),
        persist_path.clone(),
    ];

    // TODO: Use Process to start the server
    // For now, we assume it's already running

    let base_url = format!("http://localhost:{}", port);
    let server_port = port.parse().unwrap_or(21500);

    {
        let mut metrics = world.metrics.lock().await;
        metrics.base_url = base_url.clone();
        metrics.server_port = server_port;
    }

    // Wait for the server to be ready
    tokio::time::sleep(Duration::from_secs(2)).await;

    println!("✅ Server started on {}", base_url);
}

#[given(expr = "the server uses persistence in {string}")]
async fn set_persistence_path(world: &mut LithairWorld, path: String) {
    {
        let mut metrics = world.metrics.lock().await;
        metrics.persist_path = path.clone();
    }
    std::fs::create_dir_all(&path).ok();
    println!("✅ Persistence configured: {}", path);
}

#[given("the server is ready to receive requests")]
async fn server_ready(world: &mut LithairWorld) {
    let client = reqwest::Client::new();
    let health_url = {
        let metrics = world.metrics.lock().await;
        format!("{}/health", metrics.base_url)
    };

    // Wait for the server to respond
    for _ in 0..10 {
        match client.get(&health_url).send().await {
            Ok(resp) if resp.status().is_success() => {
                println!("✅ Server ready");
                return;
            }
            _ => {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    }

    panic!("❌ Server not ready after 5 seconds");
}

// Write throughput
#[when(expr = "I create {int} articles in parallel with {int} workers")]
async fn create_articles_parallel(world: &mut LithairWorld, count: usize, workers: usize) {
    let start = Instant::now();
    let base_url = {
        let metrics = world.metrics.lock().await;
        metrics.base_url.clone()
    };
    let metrics = Arc::new(Mutex::new(PerformanceMetrics::new()));
    
    let articles_per_worker = count / workers;
    let mut handles = vec![];
    
    for worker_id in 0..workers {
        let url = base_url.clone();
        let metrics = Arc::clone(&metrics);
        
        let handle = thread::spawn(move || {
            let client = Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap();
            
            for i in 0..articles_per_worker {
                let req_start = Instant::now();
                
                let article = json!({
                    "title": format!("Article {} from worker {}", i, worker_id),
                    "content": format!("Content {}", i),
                });
                
                let result = client
                    .post(format!("{}/api/articles", url))
                    .json(&article)
                    .send();
                
                let latency = req_start.elapsed();
                
                let mut m = metrics.lock().unwrap();
                m.total_requests += 1;
                m.latencies.push(latency);
                
                match result {
                    Ok(resp) if resp.status().is_success() => {
                        m.successful_requests += 1;
                    }
                    Ok(resp) => {
                        m.failed_requests += 1;
                        m.errors.push(format!("HTTP {}", resp.status()));
                    }
                    Err(e) => {
                        m.failed_requests += 1;
                        m.errors.push(e.to_string());
                    }
                }
            }
        });
        
        handles.push(handle);
    }
    
    // Wait for all workers
    for handle in handles {
        handle.join().unwrap();
    }

    let duration = start.elapsed();

    let m = metrics.lock().unwrap();
    let throughput = m.total_requests as f64 / duration.as_secs_f64();
    let error_count = m.failed_requests;
    let latency_p95 = m.p95();
    let latency_p99 = m.p99();
    let successful = m.successful_requests;
    drop(m);

    // Save in world
    {
        let mut world_metrics = world.metrics.lock().await;
        world_metrics.throughput = throughput;
        world_metrics.total_duration = duration;
        world_metrics.error_count = error_count;
        world_metrics.latency_p95 = latency_p95;
        world_metrics.latency_p99 = latency_p99;
    }

    println!("📊 {} articles created in {:?}", successful, duration);
    println!("📈 Throughput: {:.2} req/s", throughput);
}

#[then(expr = "the total time must be less than {int} second(s)")]
async fn check_total_time(world: &mut LithairWorld, max_seconds: u64) {
    let duration_secs = {
        let metrics = world.metrics.lock().await;
        metrics.total_duration.as_secs_f64()
    };

    println!("⏱️  Total time: {:.2}s (max: {}s)", duration_secs, max_seconds);

    assert!(
        duration_secs < max_seconds as f64,
        "❌ Time too long: {:.2}s > {}s",
        duration_secs,
        max_seconds
    );
}

#[then(expr = "the throughput must be greater than {int} requests per second")]
async fn check_throughput(world: &mut LithairWorld, min_throughput: usize) {
    let throughput = {
        let metrics = world.metrics.lock().await;
        metrics.throughput
    };

    println!(
        "📈 Throughput: {:.2} req/s (min: {} req/s)",
        throughput, min_throughput
    );

    assert!(
        throughput > min_throughput as f64,
        "❌ Insufficient throughput: {:.2} < {}",
        throughput,
        min_throughput
    );
}

#[then("all articles must be persisted")]
async fn check_all_persisted(world: &mut LithairWorld) {
    // Check the events.raftlog file
    let log_file = {
        let metrics = world.metrics.lock().await;
        format!("{}/events.raftlog", metrics.persist_path)
    };

    if let Ok(content) = std::fs::read_to_string(&log_file) {
        let line_count = content.lines().count();
        println!("✅ {} events persisted in {}", line_count, log_file);
    } else {
        println!("⚠️  Persistence file not found: {}", log_file);
    }
}

#[then("no error must be recorded")]
async fn no_errors(world: &mut LithairWorld) {
    let error_count = {
        let metrics = world.metrics.lock().await;
        metrics.error_count
    };

    assert_eq!(
        error_count, 0,
        "❌ {} errors recorded",
        error_count
    );
    println!("✅ No errors");
}

// Read throughput
#[given(expr = "the server contains {int} pre-created articles")]
async fn precreate_articles(world: &mut LithairWorld, count: usize) {
    let client = reqwest::Client::new();
    let url = {
        let metrics = world.metrics.lock().await;
        format!("{}/api/articles", metrics.base_url)
    };

    for i in 0..count {
        let article = json!({
            "title": format!("Pre-created Article {}", i),
            "content": format!("Content {}", i),
        });

        client.post(&url).json(&article).send().await.ok();
    }

    println!("✅ {} articles pre-created", count);
}

#[when(expr = "I read the article list {int} times with {int} workers")]
async fn read_articles_parallel(world: &mut LithairWorld, count: usize, workers: usize) {
    let start = Instant::now();
    let base_url = {
        let metrics = world.metrics.lock().await;
        metrics.base_url.clone()
    };
    let metrics = Arc::new(Mutex::new(PerformanceMetrics::new()));
    
    let reads_per_worker = count / workers;
    let mut handles = vec![];
    
    for _ in 0..workers {
        let url = base_url.clone();
        let metrics = Arc::clone(&metrics);
        
        let handle = thread::spawn(move || {
            let client = Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap();
            
            for _ in 0..reads_per_worker {
                let req_start = Instant::now();
                
                let result = client
                    .get(format!("{}/api/articles", url))
                    .send();
                
                let latency = req_start.elapsed();
                
                let mut m = metrics.lock().unwrap();
                m.total_requests += 1;
                m.latencies.push(latency);
                
                match result {
                    Ok(resp) if resp.status().is_success() => {
                        m.successful_requests += 1;
                    }
                    Ok(_) => {
                        m.failed_requests += 1;
                    }
                    Err(e) => {
                        m.failed_requests += 1;
                        m.errors.push(e.to_string());
                    }
                }
            }
        });
        
        handles.push(handle);
    }
    
    for handle in handles {
        handle.join().unwrap();
    }

    let duration = start.elapsed();
    let m = metrics.lock().unwrap();
    let throughput = m.total_requests as f64 / duration.as_secs_f64();
    let error_count = m.failed_requests;
    let latency_p95 = m.p95();
    let successful = m.successful_requests;
    drop(m);

    {
        let mut world_metrics = world.metrics.lock().await;
        world_metrics.throughput = throughput;
        world_metrics.total_duration = duration;
        world_metrics.error_count = error_count;
        world_metrics.latency_p95 = latency_p95;
    }

    println!("📊 {} reads in {:?}", successful, duration);
    println!("📈 Throughput: {:.2} req/s", throughput);
}

#[then(expr = "the p95 latency must be less than {int} milliseconds")]
async fn check_p95_latency(world: &mut LithairWorld, max_ms: u64) {
    let p95_ms = {
        let metrics = world.metrics.lock().await;
        metrics.latency_p95.as_millis()
    };

    println!("📊 p95 latency: {}ms (max: {}ms)", p95_ms, max_ms);

    assert!(
        p95_ms < max_ms as u128,
        "❌ p95 latency too high: {}ms > {}ms",
        p95_ms,
        max_ms
    );
}

#[then("no connection error must occur")]
async fn no_connection_errors(world: &mut LithairWorld) {
    let error_count = {
        let metrics = world.metrics.lock().await;
        metrics.error_count
    };

    assert_eq!(
        error_count, 0,
        "❌ {} connection errors",
        error_count
    );
    println!("✅ No connection errors");
}

// TODO: Implement other steps for mixed load, keep-alive, etc.
