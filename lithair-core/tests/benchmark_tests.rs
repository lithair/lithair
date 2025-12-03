//! Benchmark tests for Lithair framework
//!
//! These tests measure performance characteristics of the framework
//! to ensure we meet our performance goals.

use lithair_core::engine::StateEngine;
use lithair_core::http::{HttpMethod, HttpRequest, HttpResponse, HttpVersion, Router};
use lithair_core::serialization::{parse_json, stringify_json, JsonValue};
use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Default, Clone)]
struct BenchmarkApp {
    items: Vec<String>,
    counter: u64,
}

impl BenchmarkApp {
    fn add_item(&mut self, item: String) {
        self.items.push(item);
        self.counter += 1;
    }

    fn get_items(&self) -> &[String] {
        &self.items
    }

    fn get_count(&self) -> u64 {
        self.counter
    }
}

#[test]
fn benchmark_state_engine_read_performance() {
    let mut app = BenchmarkApp::default();

    // Populate with test data
    for i in 0..1000 {
        app.add_item(format!("Item {}", i));
    }

    let engine = Arc::new(StateEngine::new(app));

    // Benchmark concurrent reads
    let start = Instant::now();
    let mut handles = vec![];

    for _ in 0..10 {
        let engine_clone = Arc::clone(&engine);
        let handle = thread::spawn(move || {
            for _ in 0..1000 {
                let _count = engine_clone.with_state(|state| state.get_count()).unwrap();
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let duration = start.elapsed();
    let ops_per_sec = (10 * 1000) as f64 / duration.as_secs_f64();

    println!("🚀 State Engine Read Performance:");
    println!("   📊 10,000 concurrent reads in {:?}", duration);
    println!("   ⚡ {:.0} ops/second", ops_per_sec);
    println!("   🎯 Target: >100,000 ops/sec");

    // Should be very fast - aim for > 100k ops/sec
    assert!(ops_per_sec > 10000.0, "Read performance too slow: {:.0} ops/sec", ops_per_sec);
    assert!(duration < Duration::from_millis(100), "Total time too slow: {:?}", duration);
}

#[test]
fn benchmark_json_parsing_performance() {
    let json_samples = vec![
        r#"{"id":1,"name":"Test","active":true}"#,
        r#"{"users":[{"id":1,"name":"Alice"},{"id":2,"name":"Bob"}],"count":2}"#,
        r#"{"data":{"nested":{"deep":{"value":42}}}}"#,
        r#"{"array":[1,2,3,4,5,6,7,8,9,10],"sum":55}"#,
    ];

    let iterations = 1000;
    let start = Instant::now();

    for _ in 0..iterations {
        for sample in &json_samples {
            let parsed = parse_json(sample).unwrap();
            let _serialized = stringify_json(&parsed);
        }
    }

    let duration = start.elapsed();
    let ops_per_sec = (iterations * json_samples.len()) as f64 / duration.as_secs_f64();

    println!("📄 JSON Performance:");
    println!(
        "   📊 {} parse+serialize cycles in {:?}",
        iterations * json_samples.len(),
        duration
    );
    println!("   ⚡ {:.0} ops/second", ops_per_sec);
    println!("   🎯 Target: >10,000 ops/sec");

    assert!(ops_per_sec > 1000.0, "JSON performance too slow: {:.0} ops/sec", ops_per_sec);
}

#[test]
fn benchmark_http_request_parsing() {
    let request_samples = vec![
        b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n".as_slice(),
        b"POST /api/users HTTP/1.1\r\nHost: api.example.com\r\nContent-Type: application/json\r\nContent-Length: 25\r\n\r\n{\"name\":\"John\",\"age\":30}".as_slice(),
        b"PUT /posts/123 HTTP/1.1\r\nHost: blog.example.com\r\nAuthorization: Bearer token123\r\nContent-Type: application/json\r\n\r\n".as_slice(),
    ];

    let iterations = 1000;
    let start = Instant::now();

    for _ in 0..iterations {
        for sample in &request_samples {
            let _request = HttpRequest::parse(sample).unwrap();
        }
    }

    let duration = start.elapsed();
    let ops_per_sec = (iterations * request_samples.len()) as f64 / duration.as_secs_f64();

    println!("🌐 HTTP Request Parsing Performance:");
    println!("   📊 {} requests parsed in {:?}", iterations * request_samples.len(), duration);
    println!("   ⚡ {:.0} requests/second", ops_per_sec);
    println!("   🎯 Target: >50,000 requests/sec");

    assert!(ops_per_sec > 5000.0, "HTTP parsing too slow: {:.0} requests/sec", ops_per_sec);
}

#[test]
fn benchmark_http_response_building() {
    let iterations = 10000;
    let start = Instant::now();

    for i in 0..iterations {
        let _response = HttpResponse::ok()
            .header("Content-Type", "application/json")
            .header("X-Request-ID", &format!("req-{}", i))
            .json(&format!(r#"{{"id": {}, "message": "Hello"}}"#, i));
    }

    let duration = start.elapsed();
    let ops_per_sec = iterations as f64 / duration.as_secs_f64();

    println!("📤 HTTP Response Building Performance:");
    println!("   📊 {} responses built in {:?}", iterations, duration);
    println!("   ⚡ {:.0} responses/second", ops_per_sec);
    println!("   🎯 Target: >100,000 responses/sec");

    assert!(
        ops_per_sec > 10000.0,
        "Response building too slow: {:.0} responses/sec",
        ops_per_sec
    );
}

#[test]
fn benchmark_routing_performance() {
    let router = Router::<()>::new()
        .get("/", |_req, _params, _state| HttpResponse::ok().text("home"))
        .get("/users", |_req, _params, _state| HttpResponse::ok().text("users"))
        .get("/users/:id", |_req, params, _state| {
            let id = params.get("id").map(|s| s.as_str()).unwrap_or("unknown");
            HttpResponse::ok().text(&format!("user {}", id))
        })
        .post("/users", |_req, _params, _state| HttpResponse::created().text("created"))
        .put("/users/:id", |_req, params, _state| {
            let id = params.get("id").map(|s| s.as_str()).unwrap_or("unknown");
            HttpResponse::ok().text(&format!("updated {}", id))
        });

    let test_requests = vec![
        HttpRequest::new(
            HttpMethod::GET,
            "/".to_string(),
            HttpVersion::Http1_1,
            HashMap::new(),
            Vec::new(),
        ),
        HttpRequest::new(
            HttpMethod::GET,
            "/users".to_string(),
            HttpVersion::Http1_1,
            HashMap::new(),
            Vec::new(),
        ),
        HttpRequest::new(
            HttpMethod::GET,
            "/users/123".to_string(),
            HttpVersion::Http1_1,
            HashMap::new(),
            Vec::new(),
        ),
        HttpRequest::new(
            HttpMethod::POST,
            "/users".to_string(),
            HttpVersion::Http1_1,
            HashMap::new(),
            Vec::new(),
        ),
        HttpRequest::new(
            HttpMethod::PUT,
            "/users/456".to_string(),
            HttpVersion::Http1_1,
            HashMap::new(),
            Vec::new(),
        ),
    ];

    let iterations = 1000;
    let start = Instant::now();

    for _ in 0..iterations {
        for request in &test_requests {
            let _response = router.handle_request_stateless(request);
        }
    }

    let duration = start.elapsed();
    let ops_per_sec = (iterations * test_requests.len()) as f64 / duration.as_secs_f64();

    println!("🛣️  Routing Performance:");
    println!("   📊 {} route resolutions in {:?}", iterations * test_requests.len(), duration);
    println!("   ⚡ {:.0} routes/second", ops_per_sec);
    println!("   🎯 Target: >50,000 routes/sec");

    assert!(ops_per_sec > 5000.0, "Routing too slow: {:.0} routes/sec", ops_per_sec);
}

#[test]
fn benchmark_memory_usage() {
    // This test checks that our structures don't use excessive memory
    use std::mem::size_of;

    println!("💾 Memory Usage Analysis:");
    println!("   📏 HttpRequest: {} bytes", size_of::<HttpRequest>());
    println!("   📏 HttpResponse: {} bytes", size_of::<HttpResponse>());
    println!("   📏 JsonValue: {} bytes", size_of::<JsonValue>());
    println!("   📏 StateEngine<()>: {} bytes", size_of::<StateEngine<()>>());

    // Test memory efficiency with large states
    let mut large_app = BenchmarkApp::default();
    for i in 0..10000 {
        large_app.add_item(format!("Large item with lots of text {}", i));
    }

    let engine = StateEngine::new(large_app);

    // Memory usage should be reasonable even with large states
    let start = Instant::now();
    let snapshot = engine.snapshot().unwrap();
    let snapshot_duration = start.elapsed();

    println!("   📊 Snapshot of 10k items: {:?}", snapshot_duration);
    println!("   📏 Snapshot size: {} items", snapshot.get_items().len());

    assert!(
        snapshot_duration < Duration::from_millis(10),
        "Snapshot too slow: {:?}",
        snapshot_duration
    );
    assert_eq!(snapshot.get_items().len(), 10000);
}

#[test]
fn benchmark_scalability_stress_test() {
    // Test the framework under high concurrent load
    let app = BenchmarkApp::default();
    let engine = Arc::new(StateEngine::new(app));

    let num_threads = 20;
    let operations_per_thread = 500;

    let start = Instant::now();
    let mut handles = vec![];

    for thread_id in 0..num_threads {
        let engine_clone = Arc::clone(&engine);
        let handle = thread::spawn(move || {
            let mut local_ops = 0;

            for i in 0..operations_per_thread {
                // Mix of read and write operations
                if i % 10 == 0 {
                    // Write operation (10% of operations)
                    engine_clone
                        .with_state_mut(|state| {
                            state.add_item(format!("Thread {} item {}", thread_id, i));
                        })
                        .unwrap();
                } else {
                    // Read operation (90% of operations)
                    let _count = engine_clone.with_state(|state| state.get_count()).unwrap();
                }
                local_ops += 1;
            }

            local_ops
        });
        handles.push(handle);
    }

    let mut total_ops = 0;
    for handle in handles {
        total_ops += handle.join().unwrap();
    }

    let duration = start.elapsed();
    let ops_per_sec = total_ops as f64 / duration.as_secs_f64();

    println!("🏋️  Scalability Stress Test:");
    println!("   🧵 {} threads", num_threads);
    println!("   📊 {} total operations in {:?}", total_ops, duration);
    println!("   ⚡ {:.0} ops/second", ops_per_sec);
    println!("   🎯 Target: >10,000 ops/sec under load");

    // Verify final state
    let final_state = engine.with_state(|state| (state.get_count(), state.items.len())).unwrap();
    println!("   📈 Final state: {} counter, {} items", final_state.0, final_state.1);

    assert!(ops_per_sec > 1000.0, "Scalability too low: {:.0} ops/sec", ops_per_sec);
    assert!(duration < Duration::from_secs(5), "Stress test took too long: {:?}", duration);
}
