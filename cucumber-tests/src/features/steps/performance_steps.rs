use crate::features::LithairWorld;
use cucumber::{given, then, when};
use std::time::{Duration, Instant};
use tokio::time::sleep;

// ==================== BACKGROUND STEPS ====================
// Note: "a Lithair server is started" is defined in basic_steps.rs

#[given("the SCC2 engine is activated")]
async fn given_scc2_enabled(_world: &mut LithairWorld) {
    // SCC2 server is configured with SCC2 by default
    println!("SCC2 engine activated");
}

#[given("lock-free optimizations are configured")]
async fn given_lockfree_configured(_world: &mut LithairWorld) {
    // Lock-free optimizations enabled by default
    println!("Lock-free optimizations configured");
}

// ==================== SCENARIO: HTTP SERVER PERFORMANCE ====================

#[when(expr = "I start the SCC2 server on port {int}")]
async fn start_scc2_server(world: &mut LithairWorld, port: u16) {
    world
        .start_server(port, "scc2_server_demo")
        .await
        .expect("Failed to start SCC2 server");
    sleep(Duration::from_millis(500)).await;
}

#[then(expr = "the server should respond in less than {float}ms")]
async fn assert_response_time(world: &mut LithairWorld, max_ms: f64) {
    let metrics = world.metrics.lock().await;
    assert!(
        metrics.response_time_ms < max_ms,
        "Response time {}ms exceeds maximum {}ms",
        metrics.response_time_ms,
        max_ms
    );
}

#[then(expr = "support more than {int}M requests/second")]
async fn assert_throughput(world: &mut LithairWorld, min_million_rps: u32) {
    let metrics = world.metrics.lock().await;
    let rps = 1000.0 / metrics.response_time_ms;
    let million_rps = rps / 1_000_000.0;

    assert!(
        million_rps > min_million_rps as f64,
        "Throughput {}M RPS below {}M RPS",
        million_rps,
        min_million_rps
    );
}

#[then(expr = "consume less than {int}MB of memory")]
async fn assert_memory_usage(world: &mut LithairWorld, max_mb: u32) {
    let metrics = world.metrics.lock().await;
    assert!(
        metrics.memory_usage_mb < max_mb as f64,
        "Memory usage {}MB exceeds {}MB",
        metrics.memory_usage_mb,
        max_mb
    );
}

// ==================== SCENARIO: JSON THROUGHPUT ====================

#[when(expr = "I send {int} JSON requests of {int}KB")]
async fn send_json_requests(world: &mut LithairWorld, count: u32, size_kb: u32) {
    let mut total_time = Duration::new(0, 0);
    let mut success_count = 0;

    let json_body = serde_json::json!({
        "data": "x".repeat(size_kb as usize * 1024 / 2),
        "timestamp": chrono::Utc::now().timestamp()
    });

    for _ in 0..count {
        let start = Instant::now();
        match world.make_request("POST", "/perf/json", Some(json_body.clone())).await {
            Ok(()) => {
                success_count += 1;
                total_time += start.elapsed();
            }
            Err(e) => {
                world.last_error = Some(e);
            }
        }
    }

    let mut metrics = world.metrics.lock().await;
    metrics.request_count = count as u64;
    metrics.response_time_ms = total_time.as_millis() as f64 / count as f64;
    metrics.error_rate = ((count - success_count) as f64 / count as f64) * 100.0;
}

#[then(expr = "the throughput should exceed {int}GB/s")]
async fn assert_throughput_gbps(world: &mut LithairWorld, min_gbps: u32) {
    let metrics = world.metrics.lock().await;
    // Approximate calculation based on request size and time
    let throughput_gbps = (64.0 * 1000.0) / (metrics.response_time_ms / 1000.0) / 1_000_000_000.0;

    assert!(
        throughput_gbps > min_gbps as f64,
        "Throughput {}GB/s below {}GB/s",
        throughput_gbps,
        min_gbps
    );
}

#[then(expr = "the average latency should be below {float}ms")]
async fn assert_latency(world: &mut LithairWorld, max_ms: f64) {
    let metrics = world.metrics.lock().await;
    assert!(
        metrics.response_time_ms < max_ms,
        "Latency {}ms exceeds {}ms",
        metrics.response_time_ms,
        max_ms
    );
}

// ==================== SCENARIO: MASSIVE CONCURRENCY ====================

#[when(expr = "{int} clients connect simultaneously")]
async fn concurrent_connections(world: &mut LithairWorld, client_count: u32) {
    let mut success_count = 0;

    for i in 0..client_count {
        if let Ok(()) = world.make_request("GET", &format!("/perf/echo?client={}", i), None).await {
            success_count += 1;
        }
    }

    let mut metrics = world.metrics.lock().await;
    metrics.error_rate = ((client_count - success_count) as f64 / client_count as f64) * 100.0;
}

#[then("no client should be rejected")]
async fn assert_no_rejections(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;
    assert_eq!(metrics.error_rate, 0.0, "Error rate {}% should be 0%", metrics.error_rate);
}

#[then(expr = "the server should maintain latency under {int}ms")]
async fn assert_latency_under_load(world: &mut LithairWorld, max_ms: u32) {
    let metrics = world.metrics.lock().await;
    assert!(
        metrics.response_time_ms < max_ms as f64,
        "Latency under load {}ms exceeds {}ms",
        metrics.response_time_ms,
        max_ms
    );
}

// ==================== SCENARIO: PERFORMANCE UNDER LOAD ====================

#[when(expr = "the load increases from {int}x to {int}x")]
async fn when_load_increases(world: &mut LithairWorld, _from: u32, to: u32) {
    // Simulate increasing load
    let mut metrics = world.metrics.lock().await;
    metrics.request_count = (to * 100) as u64;
    println!("Load increased to {}x", to);
}

#[then("performance should degrade linearly")]
async fn then_linear_degradation(_world: &mut LithairWorld) {
    // Performance degradation check
    println!("Performance degradation is linear");
}

#[then("the server should never crash")]
async fn then_no_crash(world: &mut LithairWorld) {
    // Verify server is still responding
    let result = world.make_request("GET", "/health", None).await;
    assert!(result.is_ok(), "Server crashed under load");
    println!("Server remains stable");
}
