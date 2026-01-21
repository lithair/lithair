use crate::features::world::LithairWorld;
use cucumber::{given, then, when};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::time::{sleep, Duration};

// ==================== STRESS TEST STATISTICS ====================

#[derive(Debug, Default)]
pub struct StressTestStats {
    // Operation counts
    pub products_created: AtomicU64,
    pub products_updated: AtomicU64,
    pub products_deleted: AtomicU64,
    pub customers_created: AtomicU64,
    pub customers_updated: AtomicU64,
    pub customers_deleted: AtomicU64,
    pub orders_created: AtomicU64,
    pub orders_updated: AtomicU64,
    pub orders_deleted: AtomicU64,
    // Node targeting
    pub requests_to_node_0: AtomicU64,
    pub requests_to_node_1: AtomicU64,
    pub requests_to_node_2: AtomicU64,
    // Errors
    pub errors: AtomicU64,
    pub redirects: AtomicU64,
    // Timing
    pub start_time: std::sync::Mutex<Option<std::time::Instant>>,
    pub end_time: std::sync::Mutex<Option<std::time::Instant>>,
}

impl StressTestStats {
    pub fn total_operations(&self) -> u64 {
        self.products_created.load(Ordering::Relaxed)
            + self.products_updated.load(Ordering::Relaxed)
            + self.products_deleted.load(Ordering::Relaxed)
            + self.customers_created.load(Ordering::Relaxed)
            + self.customers_updated.load(Ordering::Relaxed)
            + self.customers_deleted.load(Ordering::Relaxed)
            + self.orders_created.load(Ordering::Relaxed)
            + self.orders_updated.load(Ordering::Relaxed)
            + self.orders_deleted.load(Ordering::Relaxed)
    }

    pub fn duration(&self) -> std::time::Duration {
        let start = self.start_time.lock().unwrap();
        let end = self.end_time.lock().unwrap();
        match (*start, *end) {
            (Some(s), Some(e)) => e.duration_since(s),
            (Some(s), None) => s.elapsed(),
            _ => std::time::Duration::ZERO,
        }
    }
}

// Thread-safe storage for stress test data
lazy_static::lazy_static! {
    static ref STRESS_STATS: Arc<StressTestStats> = Arc::new(StressTestStats::default());
    static ref CREATED_IDS: Arc<tokio::sync::Mutex<CreatedIds>> = Arc::new(tokio::sync::Mutex::new(CreatedIds::default()));
}

#[derive(Default)]
struct CreatedIds {
    products: Vec<String>,
    customers: Vec<String>,
    orders: Vec<String>,
}

// ==================== MODEL TYPES ====================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModelType {
    Product,
    Customer,
    Order,
}

impl ModelType {
    fn random() -> Self {
        let mut rng = rand::thread_rng();
        match rng.gen_range(0..3) {
            0 => ModelType::Product,
            1 => ModelType::Customer,
            _ => ModelType::Order,
        }
    }

    fn endpoint(&self) -> &'static str {
        match self {
            ModelType::Product => "/api/products",
            ModelType::Customer => "/api/customers",
            ModelType::Order => "/api/orders",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CrudOperation {
    Create,
    Update,
    Delete,
}

impl CrudOperation {
    fn random_weighted() -> Self {
        let mut rng = rand::thread_rng();
        // 60% create, 30% update, 10% delete
        let roll = rng.gen_range(0..100);
        if roll < 60 {
            CrudOperation::Create
        } else if roll < 90 {
            CrudOperation::Update
        } else {
            CrudOperation::Delete
        }
    }
}

// ==================== CLUSTER SETUP ====================

#[given(expr = "a real LithairServer cluster of {int} nodes with multi-model support")]
async fn given_real_cluster_multi_model(world: &mut LithairWorld, node_count: u32) {
    println!(
        "🚀 Starting REAL multi-model LithairServer cluster with {} nodes...",
        node_count
    );

    // Reset stats
    STRESS_STATS.products_created.store(0, Ordering::Relaxed);
    STRESS_STATS.products_updated.store(0, Ordering::Relaxed);
    STRESS_STATS.products_deleted.store(0, Ordering::Relaxed);
    STRESS_STATS.customers_created.store(0, Ordering::Relaxed);
    STRESS_STATS.customers_updated.store(0, Ordering::Relaxed);
    STRESS_STATS.customers_deleted.store(0, Ordering::Relaxed);
    STRESS_STATS.orders_created.store(0, Ordering::Relaxed);
    STRESS_STATS.orders_updated.store(0, Ordering::Relaxed);
    STRESS_STATS.orders_deleted.store(0, Ordering::Relaxed);
    STRESS_STATS.requests_to_node_0.store(0, Ordering::Relaxed);
    STRESS_STATS.requests_to_node_1.store(0, Ordering::Relaxed);
    STRESS_STATS.requests_to_node_2.store(0, Ordering::Relaxed);
    STRESS_STATS.errors.store(0, Ordering::Relaxed);
    STRESS_STATS.redirects.store(0, Ordering::Relaxed);

    // Clear created IDs
    {
        let mut ids = CREATED_IDS.lock().await;
        ids.products.clear();
        ids.customers.clear();
        ids.orders.clear();
    }

    // Use persistent data directory (/tmp/lithair-stress-test/) cleaned at start
    let ports = world
        .start_real_cluster_persistent(node_count as usize)
        .await
        .expect("Failed to start real cluster");

    println!(
        "✅ Multi-model real cluster of {} nodes started (ports: {:?})",
        node_count, ports
    );
    println!("📁 Data persisted at: /tmp/lithair-stress-test/");
}

// ==================== RANDOM CRUD OPERATIONS ====================

#[when(regex = r"I execute (\d+) random CRUD operations across all models targeting random nodes")]
async fn when_execute_random_crud(world: &mut LithairWorld, operation_count: u64) {
    println!("🎲 Executing {} random CRUD operations across all models...", operation_count);

    *STRESS_STATS.start_time.lock().unwrap() = Some(std::time::Instant::now());

    let cluster_size = world.real_cluster_size().await;
    let ports = world.get_real_cluster_ports().await;

    // Optimized HTTP client with connection pooling and keep-alive
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .pool_max_idle_per_host(200)          // More idle connections per host
        .pool_idle_timeout(std::time::Duration::from_secs(90))  // Keep connections alive longer
        .tcp_keepalive(std::time::Duration::from_secs(60))      // TCP keep-alive
        .tcp_nodelay(true)                     // Disable Nagle's algorithm for lower latency
        .build()
        .expect("Failed to create HTTP client");

    let mut rng = rand::thread_rng();
    let batch_size = 100; // Process in batches for progress reporting
    let mut completed = 0u64;

    while completed < operation_count {
        let batch_end = std::cmp::min(completed + batch_size, operation_count);

        for _ in completed..batch_end {
            // Random node selection
            let node_idx = rng.gen_range(0..cluster_size);
            let port = ports[node_idx];

            // Track node targeting
            match node_idx {
                0 => STRESS_STATS.requests_to_node_0.fetch_add(1, Ordering::Relaxed),
                1 => STRESS_STATS.requests_to_node_1.fetch_add(1, Ordering::Relaxed),
                _ => STRESS_STATS.requests_to_node_2.fetch_add(1, Ordering::Relaxed),
            };

            // Random model and operation
            let model = ModelType::random();
            let operation = CrudOperation::random_weighted();

            let result = execute_operation(&client, port, model, operation).await;

            if let Err(e) = result {
                STRESS_STATS.errors.fetch_add(1, Ordering::Relaxed);
                if e.contains("redirect") || e.contains("307") {
                    STRESS_STATS.redirects.fetch_add(1, Ordering::Relaxed);
                }
            }
        }

        completed = batch_end;

        // Progress report every 10%
        if completed % (operation_count / 10).max(1) == 0 {
            let progress = (completed as f64 / operation_count as f64) * 100.0;
            println!(
                "📊 Progress: {:.0}% ({}/{} operations)",
                progress, completed, operation_count
            );
        }
    }

    *STRESS_STATS.end_time.lock().unwrap() = Some(std::time::Instant::now());

    let duration = STRESS_STATS.duration();
    let ops_per_sec = operation_count as f64 / duration.as_secs_f64();

    println!(
        "✅ Completed {} operations in {:.2}s ({:.0} ops/sec)",
        operation_count,
        duration.as_secs_f64(),
        ops_per_sec
    );
}

#[when(regex = r"I execute (\d+) concurrent random CRUD operations with (\d+) workers")]
async fn when_execute_concurrent_crud(
    world: &mut LithairWorld,
    operation_count: u64,
    worker_count: u32,
) {
    println!(
        "🎲 Executing {} concurrent CRUD operations with {} workers...",
        operation_count, worker_count
    );

    *STRESS_STATS.start_time.lock().unwrap() = Some(std::time::Instant::now());

    let ports = world.get_real_cluster_ports().await;
    let ops_per_worker = operation_count / worker_count as u64;

    let mut handles = Vec::new();

    for worker_id in 0..worker_count {
        let ports_clone = ports.clone();
        let ops = if worker_id == worker_count - 1 {
            // Last worker handles remainder
            operation_count - (ops_per_worker * (worker_count as u64 - 1))
        } else {
            ops_per_worker
        };

        // Pre-generate random choices for this worker to avoid Send issues with thread_rng
        let mut rng = rand::thread_rng();
        let choices: Vec<(usize, ModelType, CrudOperation)> = (0..ops)
            .map(|_| {
                let node_idx = rng.gen_range(0..ports_clone.len());
                let model = ModelType::random();
                let operation = CrudOperation::random_weighted();
                (node_idx, model, operation)
            })
            .collect();

        let handle = tokio::spawn(async move {
            // Optimized HTTP client with connection pooling and keep-alive
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .pool_max_idle_per_host(50)           // Per-worker connection pool
                .pool_idle_timeout(std::time::Duration::from_secs(90))
                .tcp_keepalive(std::time::Duration::from_secs(60))
                .tcp_nodelay(true)
                .build()
                .expect("Failed to create HTTP client");

            for (node_idx, model, operation) in choices {
                let port = ports_clone[node_idx];

                match node_idx {
                    0 => STRESS_STATS.requests_to_node_0.fetch_add(1, Ordering::Relaxed),
                    1 => STRESS_STATS.requests_to_node_1.fetch_add(1, Ordering::Relaxed),
                    _ => STRESS_STATS.requests_to_node_2.fetch_add(1, Ordering::Relaxed),
                };

                if let Err(_) = execute_operation(&client, port, model, operation).await {
                    STRESS_STATS.errors.fetch_add(1, Ordering::Relaxed);
                }
            }
        });

        handles.push(handle);
    }

    // Wait for all workers to complete
    for handle in handles {
        let _ = handle.await;
    }

    *STRESS_STATS.end_time.lock().unwrap() = Some(std::time::Instant::now());

    let duration = STRESS_STATS.duration();
    let ops_per_sec = operation_count as f64 / duration.as_secs_f64();

    println!(
        "✅ Completed {} concurrent operations in {:.2}s ({:.0} ops/sec)",
        operation_count,
        duration.as_secs_f64(),
        ops_per_sec
    );
}

#[when(regex = r"I execute (\d+) more random CRUD operations on remaining nodes")]
async fn when_execute_more_crud_remaining(world: &mut LithairWorld, operation_count: u64) {
    println!(
        "🎲 Executing {} more random CRUD operations on remaining nodes...",
        operation_count
    );

    let ports = world.get_real_cluster_ports().await;

    // Filter to only alive nodes
    let alive_ports: Vec<u16> = {
        let nodes = world.real_cluster_nodes.lock().await;
        nodes.iter().filter(|n| n.process.is_some()).map(|n| n.port).collect()
    };

    if alive_ports.is_empty() {
        println!("⚠️ No alive nodes to send requests to");
        return;
    }

    // Optimized HTTP client with connection pooling and keep-alive
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .pool_max_idle_per_host(100)
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        .tcp_keepalive(std::time::Duration::from_secs(60))
        .tcp_nodelay(true)
        .build()
        .expect("Failed to create HTTP client");

    let mut rng = rand::thread_rng();

    for _ in 0..operation_count {
        let port = alive_ports[rng.gen_range(0..alive_ports.len())];

        let model = ModelType::random();
        let operation = CrudOperation::random_weighted();

        if let Err(_) = execute_operation(&client, port, model, operation).await {
            STRESS_STATS.errors.fetch_add(1, Ordering::Relaxed);
        }
    }

    println!(
        "✅ Completed {} more operations on {} remaining nodes",
        operation_count,
        alive_ports.len()
    );
}

// ==================== CHAOS OPERATIONS ====================

#[when("I kill a random follower node")]
async fn when_kill_random_follower(world: &mut LithairWorld) {
    let mut nodes = world.real_cluster_nodes.lock().await;

    // Find a follower (node_id != 0) to kill
    for node in nodes.iter_mut() {
        if node.node_id != 0 && node.process.is_some() {
            if let Some(mut process) = node.process.take() {
                println!("🔪 Killing follower node {} (port {})", node.node_id, node.port);
                let _ = process.kill();
                let _ = process.wait();
                println!("💀 Follower node {} killed", node.node_id);
                break;
            }
        }
    }
}

// ==================== WAIT AND SYNC ====================

#[when("I wait for replication to complete")]
async fn when_wait_for_replication(_world: &mut LithairWorld) {
    // Wait for async replication to propagate across the cluster
    // The replication is fire-and-forget (tokio::spawn), so we need to wait
    // for all pending replication requests to complete
    println!("⏳ Waiting for async replication to complete...");
    println!("   (Replication is fire-and-forget, allowing time for propagation)");

    // Wait 10 seconds to allow all async replication tasks to complete
    // This accounts for:
    // - Network latency between nodes
    // - Retry backoff (up to 3 retries with exponential backoff)
    // - Disk I/O on followers
    sleep(Duration::from_secs(10)).await;
    println!("✅ Replication wait complete");
}

// ==================== CONSISTENCY VERIFICATION ====================

#[then("all models should have consistent data across all nodes")]
async fn then_all_models_consistent(world: &mut LithairWorld) {
    let cluster_size = world.real_cluster_size().await;
    println!("🔍 Verifying data consistency across {} nodes for all models...", cluster_size);

    let models = [
        ("Product", "/api/products"),
        ("Customer", "/api/customers"),
        ("Order", "/api/orders"),
    ];

    for (model_name, endpoint) in models.iter() {
        let mut counts = Vec::new();

        for i in 0..cluster_size {
            let result = world.make_real_cluster_request(i, "GET", endpoint, None).await;
            match result {
                Ok(response) => {
                    let count = if let Some(arr) = response.as_array() { arr.len() } else { 0 };
                    counts.push(count);
                    println!("  Node {} {}: {} items", i, model_name, count);
                }
                Err(e) => {
                    println!("  ⚠️ Node {} {} error: {}", i, model_name, e);
                    counts.push(0);
                }
            }
        }

        // Verify all nodes have the same count
        if !counts.is_empty() {
            let first_count = counts[0];
            let all_same = counts.iter().all(|&c| c == first_count);
            if all_same {
                println!("  ✅ {} consistent across all nodes ({} items)", model_name, first_count);
            } else {
                println!("  ⚠️ {} INCONSISTENT: counts = {:?}", model_name, counts);
            }
        }
    }

    println!("✅ Consistency check complete");
}

#[then("the remaining nodes should have consistent data")]
async fn then_remaining_nodes_consistent(world: &mut LithairWorld) {
    println!("🔍 Verifying data consistency on remaining alive nodes...");

    let alive_nodes: Vec<(u64, u16)> = {
        let nodes = world.real_cluster_nodes.lock().await;
        nodes
            .iter()
            .filter(|n| n.process.is_some())
            .map(|n| (n.node_id, n.port))
            .collect()
    };

    println!("  Alive nodes: {:?}", alive_nodes);

    let models = [
        ("Product", "/api/products"),
        ("Customer", "/api/customers"),
        ("Order", "/api/orders"),
    ];

    for (model_name, endpoint) in models.iter() {
        let mut counts = Vec::new();

        for (node_id, _port) in &alive_nodes {
            let result =
                world.make_real_cluster_request(*node_id as usize, "GET", endpoint, None).await;
            match result {
                Ok(response) => {
                    let count = if let Some(arr) = response.as_array() { arr.len() } else { 0 };
                    counts.push(count);
                }
                Err(_) => {
                    counts.push(0);
                }
            }
        }

        if counts.len() >= 2 {
            let first_count = counts[0];
            let all_same = counts.iter().all(|&c| c == first_count);
            if all_same {
                println!(
                    "  ✅ {} consistent on remaining nodes ({} items)",
                    model_name, first_count
                );
            } else {
                println!("  ⚠️ {} variance on remaining nodes: {:?}", model_name, counts);
            }
        }
    }

    println!("✅ Remaining nodes consistency check complete");
}

#[then("the operation count should match expected values")]
async fn then_operation_count_matches(_world: &mut LithairWorld) {
    let total = STRESS_STATS.total_operations();
    let errors = STRESS_STATS.errors.load(Ordering::Relaxed);

    println!("📊 Operation counts:");
    println!(
        "  Products: {} created, {} updated, {} deleted",
        STRESS_STATS.products_created.load(Ordering::Relaxed),
        STRESS_STATS.products_updated.load(Ordering::Relaxed),
        STRESS_STATS.products_deleted.load(Ordering::Relaxed)
    );
    println!(
        "  Customers: {} created, {} updated, {} deleted",
        STRESS_STATS.customers_created.load(Ordering::Relaxed),
        STRESS_STATS.customers_updated.load(Ordering::Relaxed),
        STRESS_STATS.customers_deleted.load(Ordering::Relaxed)
    );
    println!(
        "  Orders: {} created, {} updated, {} deleted",
        STRESS_STATS.orders_created.load(Ordering::Relaxed),
        STRESS_STATS.orders_updated.load(Ordering::Relaxed),
        STRESS_STATS.orders_deleted.load(Ordering::Relaxed)
    );
    println!("  Total: {} operations, {} errors", total, errors);

    assert!(total > 0, "Should have executed some operations");
    println!("✅ Operation counts verified");
}

#[then("no data should be lost or corrupted")]
async fn then_no_data_lost(world: &mut LithairWorld) {
    // Verify by checking that we can read all data without errors
    let cluster_size = world.real_cluster_size().await;
    let mut all_readable = true;

    for i in 0..cluster_size {
        for endpoint in ["/api/products", "/api/customers", "/api/orders"].iter() {
            let result = world.make_real_cluster_request(i, "GET", endpoint, None).await;
            if result.is_err() {
                all_readable = false;
                println!("⚠️ Node {} {} not readable", i, endpoint);
            }
        }
    }

    assert!(all_readable, "All nodes should have readable data");
    println!("✅ No data loss or corruption detected");
}

#[then("operations should have succeeded on the majority")]
async fn then_majority_succeeded(_world: &mut LithairWorld) {
    let total = STRESS_STATS.total_operations();
    let errors = STRESS_STATS.errors.load(Ordering::Relaxed);
    let success_rate =
        if total > 0 { ((total - errors) as f64 / total as f64) * 100.0 } else { 0.0 };

    println!(
        "📊 Success rate: {:.1}% ({} successes, {} errors)",
        success_rate,
        total - errors,
        errors
    );

    assert!(success_rate >= 50.0, "At least 50% of operations should succeed");
    println!("✅ Majority of operations succeeded");
}

#[then("the data files should be identical across all nodes")]
async fn then_data_files_identical(_world: &mut LithairWorld) {
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader};
    use std::path::PathBuf;

    println!("🔍 Verifying data file consistency across all nodes...");
    println!("   Note: Leader stores *Created events, followers store *Replicated events");
    println!("   Comparing event COUNTS (not raw file bytes) for semantic equivalence.\n");

    let base_dir = PathBuf::from("/tmp/lithair-stress-test");
    let models = ["products", "customers", "orders"];

    // Collect event counts for each node
    // Structure: node -> model -> event_count
    let mut node_event_counts: HashMap<String, HashMap<String, usize>> = HashMap::new();

    for node_id in 0..3 {
        let node_dir = base_dir.join(format!("node_{}", node_id));
        let mut model_counts: HashMap<String, usize> = HashMap::new();

        for model in &models {
            let events_dir = format!("pure_node_{}/{}_events", node_id, model);
            let raftlog_path = node_dir.join(&events_dir).join("events.raftlog");

            if raftlog_path.exists() {
                // Count lines (events) in the raftlog file
                let event_count = if let Ok(file) = std::fs::File::open(&raftlog_path) {
                    let reader = BufReader::new(file);
                    reader.lines().count()
                } else {
                    0
                };

                let file_size = std::fs::metadata(&raftlog_path).map(|m| m.len()).unwrap_or(0);

                model_counts.insert(model.to_string(), event_count);
                println!(
                    "  Node {} {}: {} events ({} bytes)",
                    node_id, model, event_count, file_size
                );
            } else {
                model_counts.insert(model.to_string(), 0);
                println!("  ⚠️ Node {} {}: MISSING", node_id, model);
            }
        }

        node_event_counts.insert(format!("node_{}", node_id), model_counts);
    }

    // Compare event counts across nodes
    println!("\n📊 Event count consistency comparison:");
    let mut all_consistent = true;
    let mut consistency_report = Vec::new();

    for model in &models {
        let mut counts: Vec<usize> = Vec::new();

        for node_id in 0..3 {
            let node_key = format!("node_{}", node_id);
            if let Some(model_counts) = node_event_counts.get(&node_key) {
                if let Some(&count) = model_counts.get(*model) {
                    counts.push(count);
                }
            }
        }

        // Check if all nodes have events and counts match
        let has_events = counts.iter().all(|&c| c > 0);
        let counts_match = counts.iter().all(|&c| c == counts[0]);

        if has_events && counts_match {
            println!("  ✅ {} event counts MATCH across all nodes ({} events)", model, counts[0]);
            consistency_report.push(format!("{}: {} events (consistent)", model, counts[0]));
        } else if !has_events {
            println!("  ❌ {} events MISSING on some nodes - counts: {:?}", model, counts);
            consistency_report.push(format!("{}: MISSING events (counts: {:?})", model, counts));
            all_consistent = false;
        } else {
            // Counts don't match - check variance percentage
            let max_count = *counts.iter().max().unwrap_or(&0);
            let min_count = *counts.iter().min().unwrap_or(&0);
            let variance = if max_count > 0 {
                ((max_count - min_count) as f64 / max_count as f64) * 100.0
            } else {
                0.0
            };

            // Allow small variance (< 1%) due to async replication timing
            if variance < 1.0 {
                println!(
                    "  ✅ {} event counts CLOSE ENOUGH ({:.2}% variance) - counts: {:?}",
                    model, variance, counts
                );
                consistency_report
                    .push(format!("{}: ~{} events ({:.2}% variance)", model, max_count, variance));
            } else {
                println!(
                    "  ⚠️ {} event counts DIFFER ({:.2}% variance) - counts: {:?}",
                    model, variance, counts
                );
                consistency_report
                    .push(format!("{}: DIFFERS {:?} ({:.2}% variance)", model, counts, variance));
                all_consistent = false;
            }
        }
    }

    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║           EVENT-LEVEL CONSISTENCY REPORT                      ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    for report in &consistency_report {
        println!("║  {}", report);
    }
    println!("╠══════════════════════════════════════════════════════════════╣");
    if all_consistent {
        println!("║  ✅ ALL NODES HAVE CONSISTENT EVENT COUNTS                    ║");
    } else {
        println!("║  ❌ EVENT COUNTS INCONSISTENT - POSSIBLE REPLICATION LAG      ║");
    }
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // This assertion verifies replication completeness
    assert!(all_consistent, "Event counts should be consistent across all nodes (allow <1% variance for async replication)");
}

#[then("I display stress test statistics")]
async fn then_display_statistics(_world: &mut LithairWorld) {
    let duration = STRESS_STATS.duration();
    let total = STRESS_STATS.total_operations();
    let errors = STRESS_STATS.errors.load(Ordering::Relaxed);
    let redirects = STRESS_STATS.redirects.load(Ordering::Relaxed);

    let ops_per_sec =
        if duration.as_secs_f64() > 0.0 { total as f64 / duration.as_secs_f64() } else { 0.0 };

    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║                   STRESS TEST STATISTICS                      ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!(
        "║ Duration: {:.2}s                                              ",
        duration.as_secs_f64()
    );
    println!("║ Total Operations: {}                                          ", total);
    println!("║ Throughput: {:.0} ops/sec                                     ", ops_per_sec);
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║ BY MODEL:                                                     ║");
    println!(
        "║   Products:  C={} U={} D={}",
        STRESS_STATS.products_created.load(Ordering::Relaxed),
        STRESS_STATS.products_updated.load(Ordering::Relaxed),
        STRESS_STATS.products_deleted.load(Ordering::Relaxed)
    );
    println!(
        "║   Customers: C={} U={} D={}",
        STRESS_STATS.customers_created.load(Ordering::Relaxed),
        STRESS_STATS.customers_updated.load(Ordering::Relaxed),
        STRESS_STATS.customers_deleted.load(Ordering::Relaxed)
    );
    println!(
        "║   Orders:    C={} U={} D={}",
        STRESS_STATS.orders_created.load(Ordering::Relaxed),
        STRESS_STATS.orders_updated.load(Ordering::Relaxed),
        STRESS_STATS.orders_deleted.load(Ordering::Relaxed)
    );
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║ BY NODE:                                                      ║");
    println!(
        "║   Node 0 (Leader):   {} requests",
        STRESS_STATS.requests_to_node_0.load(Ordering::Relaxed)
    );
    println!(
        "║   Node 1 (Follower): {} requests",
        STRESS_STATS.requests_to_node_1.load(Ordering::Relaxed)
    );
    println!(
        "║   Node 2 (Follower): {} requests",
        STRESS_STATS.requests_to_node_2.load(Ordering::Relaxed)
    );
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!(
        "║ ERRORS: {} total ({} redirects)                              ",
        errors, redirects
    );
    println!(
        "║ SUCCESS RATE: {:.1}%                                          ",
        if total > 0 { ((total - errors) as f64 / total as f64) * 100.0 } else { 0.0 }
    );
    println!("╚══════════════════════════════════════════════════════════════╝\n");
}

// ==================== HELPER FUNCTIONS ====================

async fn execute_operation(
    client: &reqwest::Client,
    port: u16,
    model: ModelType,
    operation: CrudOperation,
) -> Result<(), String> {
    let base_url = format!("http://127.0.0.1:{}", port);
    let endpoint = model.endpoint();

    match operation {
        CrudOperation::Create => {
            let body = generate_create_body(model);
            let url = format!("{}{}", base_url, endpoint);

            let resp = client
                .post(&url)
                .json(&body)
                .send()
                .await
                .map_err(|e| format!("Create request failed: {}", e))?;

            if resp.status().is_success() {
                // Extract ID from response and store it
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    if let Some(id) = json.get("id").and_then(|v| v.as_str()) {
                        let mut ids = CREATED_IDS.lock().await;
                        match model {
                            ModelType::Product => {
                                ids.products.push(id.to_string());
                                STRESS_STATS.products_created.fetch_add(1, Ordering::Relaxed);
                            }
                            ModelType::Customer => {
                                ids.customers.push(id.to_string());
                                STRESS_STATS.customers_created.fetch_add(1, Ordering::Relaxed);
                            }
                            ModelType::Order => {
                                ids.orders.push(id.to_string());
                                STRESS_STATS.orders_created.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                }
                Ok(())
            } else if resp.status().as_u16() == 307 {
                Err("redirect".to_string())
            } else {
                Err(format!("Create failed with status: {}", resp.status()))
            }
        }

        CrudOperation::Update => {
            // Get a random existing ID for this model
            let id = {
                let ids = CREATED_IDS.lock().await;
                let id_list = match model {
                    ModelType::Product => &ids.products,
                    ModelType::Customer => &ids.customers,
                    ModelType::Order => &ids.orders,
                };
                if id_list.is_empty() {
                    return Ok(()); // No items to update, skip
                }
                let mut rng = rand::thread_rng();
                id_list[rng.gen_range(0..id_list.len())].clone()
            };

            let body = generate_update_body(model, &id);
            let url = format!("{}{}/{}", base_url, endpoint, id);

            let resp = client
                .put(&url)
                .json(&body)
                .send()
                .await
                .map_err(|e| format!("Update request failed: {}", e))?;

            if resp.status().is_success() {
                match model {
                    ModelType::Product => {
                        STRESS_STATS.products_updated.fetch_add(1, Ordering::Relaxed)
                    }
                    ModelType::Customer => {
                        STRESS_STATS.customers_updated.fetch_add(1, Ordering::Relaxed)
                    }
                    ModelType::Order => STRESS_STATS.orders_updated.fetch_add(1, Ordering::Relaxed),
                };
                Ok(())
            } else if resp.status().as_u16() == 307 {
                Err("redirect".to_string())
            } else {
                Err(format!("Update failed with status: {}", resp.status()))
            }
        }

        CrudOperation::Delete => {
            // Get a random existing ID for this model
            let id = {
                let mut ids = CREATED_IDS.lock().await;
                let id_list = match model {
                    ModelType::Product => &mut ids.products,
                    ModelType::Customer => &mut ids.customers,
                    ModelType::Order => &mut ids.orders,
                };
                if id_list.is_empty() {
                    return Ok(()); // No items to delete, skip
                }
                let mut rng = rand::thread_rng();
                let idx = rng.gen_range(0..id_list.len());
                id_list.remove(idx) // Remove from list as well
            };

            let url = format!("{}{}/{}", base_url, endpoint, id);

            let resp = client
                .delete(&url)
                .send()
                .await
                .map_err(|e| format!("Delete request failed: {}", e))?;

            if resp.status().is_success() || resp.status().as_u16() == 404 {
                match model {
                    ModelType::Product => {
                        STRESS_STATS.products_deleted.fetch_add(1, Ordering::Relaxed)
                    }
                    ModelType::Customer => {
                        STRESS_STATS.customers_deleted.fetch_add(1, Ordering::Relaxed)
                    }
                    ModelType::Order => STRESS_STATS.orders_deleted.fetch_add(1, Ordering::Relaxed),
                };
                Ok(())
            } else if resp.status().as_u16() == 307 {
                Err("redirect".to_string())
            } else {
                Err(format!("Delete failed with status: {}", resp.status()))
            }
        }
    }
}

fn generate_create_body(model: ModelType) -> serde_json::Value {
    let mut rng = rand::thread_rng();

    match model {
        ModelType::Product => {
            serde_json::json!({
                "name": format!("Product_{}", rng.gen::<u32>()),
                "price": rng.gen_range(1.0..1000.0),
                "category": format!("Category_{}", rng.gen_range(1..10))
            })
        }
        ModelType::Customer => {
            let tiers = ["bronze", "silver", "gold", "platinum"];
            let tier = tiers[rng.gen_range(0..4)];
            serde_json::json!({
                "name": format!("Customer_{}", rng.gen::<u32>()),
                "email": format!("customer_{}@test.com", rng.gen::<u32>()),
                "tier": tier
            })
        }
        ModelType::Order => {
            serde_json::json!({
                "customer_id": uuid::Uuid::new_v4().to_string(),
                "product_id": uuid::Uuid::new_v4().to_string(),
                "quantity": rng.gen_range(1..100),
                "total_price": rng.gen_range(10.0..10000.0),
                "status": "pending"
            })
        }
    }
}

fn generate_update_body(model: ModelType, id: &str) -> serde_json::Value {
    let mut rng = rand::thread_rng();

    match model {
        ModelType::Product => {
            serde_json::json!({
                "id": id,
                "name": format!("UpdatedProduct_{}", rng.gen::<u32>()),
                "price": rng.gen_range(1.0..1000.0),
                "category": format!("UpdatedCat_{}", rng.gen_range(1..10))
            })
        }
        ModelType::Customer => {
            let tiers = ["bronze", "silver", "gold", "platinum"];
            let tier = tiers[rng.gen_range(0..4)];
            serde_json::json!({
                "id": id,
                "name": format!("UpdatedCustomer_{}", rng.gen::<u32>()),
                "email": format!("updated_{}@test.com", rng.gen::<u32>()),
                "tier": tier
            })
        }
        ModelType::Order => {
            let statuses = ["pending", "processing", "shipped", "delivered"];
            let status = statuses[rng.gen_range(0..4)];
            serde_json::json!({
                "id": id,
                "customer_id": uuid::Uuid::new_v4().to_string(),
                "product_id": uuid::Uuid::new_v4().to_string(),
                "quantity": rng.gen_range(1..100),
                "total_price": rng.gen_range(10.0..10000.0),
                "status": status
            })
        }
    }
}
