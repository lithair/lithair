//! Raft Consistency Test
//!
//! Automated testing of Raft replication consistency with heavy load.
//! - Launches 3 nodes automatically
//! - Runs concurrent CRUD operations
//! - Verifies consistency across all nodes
//! - Reports detailed results

use anyhow::{Context, Result};
use clap::Parser;
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::sleep;

#[derive(Parser, Debug)]
#[command(name = "consistency_test")]
#[command(about = "Automated Raft consistency testing")]
struct Args {
    /// Number of operations per table
    #[arg(long, default_value = "100")]
    ops: usize,

    /// Concurrency level
    #[arg(long, default_value = "10")]
    concurrency: usize,

    /// Include UPDATE operations
    #[arg(long, default_value = "true")]
    with_updates: bool,

    /// Include DELETE operations
    #[arg(long, default_value = "true")]
    with_deletes: bool,

    /// Skip node launch (use existing nodes)
    #[arg(long)]
    skip_launch: bool,

    /// Wait time after benchmark before checking (ms)
    #[arg(long, default_value = "2000")]
    settle_time: u64,
}

const LEADER_PORT: u16 = 18080;
const FOLLOWER1_PORT: u16 = 18081;
const FOLLOWER2_PORT: u16 = 18082;

const TABLES: &[&str] = &["api/items", "api/orders", "api/logs"];

/// PlaygroundItem - matches the server's model exactly
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TestItem {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    priority: i32,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default = "default_item_status")]
    status: String,
}

fn default_item_status() -> String {
    "Draft".to_string()
}

/// Order - matches the server's model exactly
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TestOrder {
    customer_id: String,
    #[serde(default = "default_order_status")]
    status: String,
    #[serde(default)]
    total_cents: i64,
    #[serde(default)]
    item_count: i32,
    #[serde(default)]
    shipping_address: String,
    #[serde(default)]
    notes: String,
}

fn default_order_status() -> String {
    "Pending".to_string()
}

/// AuditLog - matches the server's model exactly
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TestLog {
    #[serde(default = "default_log_level")]
    level: String,
    action: String,
    #[serde(default)]
    entity_type: String,
    #[serde(default)]
    entity_id: String,
    #[serde(default = "default_details")]
    details: serde_json::Value,
    #[serde(default)]
    source_node: u64,
}

fn default_log_level() -> String {
    "Info".to_string()
}
fn default_details() -> serde_json::Value {
    serde_json::json!({})
}

#[derive(Debug, Default)]
struct BenchmarkStats {
    creates: AtomicUsize,
    updates: AtomicUsize,
    deletes: AtomicUsize,
    errors: AtomicUsize,
    total_latency_ms: AtomicUsize,
}

#[derive(Debug)]
struct ConsistencyResult {
    table: String,
    leader_count: usize,
    follower1_count: usize,
    follower2_count: usize,
    consistent: bool,
}

struct NodeManager {
    processes: Vec<Child>,
}

impl NodeManager {
    fn new() -> Self {
        Self { processes: Vec::new() }
    }

    fn launch_nodes(&mut self) -> Result<()> {
        println!("Launching 3 nodes...");

        // Find the playground_node binary
        let binary = std::env::current_dir()?.join("target/debug/playground_node");

        if !binary.exists() {
            anyhow::bail!(
                "playground_node binary not found at {:?}. Run: cargo build --package lithair_playground",
                binary
            );
        }

        // Node 1: Leader (node-id 0 = initial leader)
        let node1 = Command::new(&binary)
            .args([
                "--port",
                &LEADER_PORT.to_string(),
                "--node-id",
                "0",
                "--peers",
                &format!("{},{}", FOLLOWER1_PORT, FOLLOWER2_PORT),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("Failed to launch node 1")?;
        self.processes.push(node1);

        // Node 2: Follower 1
        let node2 = Command::new(&binary)
            .args([
                "--port",
                &FOLLOWER1_PORT.to_string(),
                "--node-id",
                "1",
                "--peers",
                &format!("{},{}", LEADER_PORT, FOLLOWER2_PORT),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("Failed to launch node 2")?;
        self.processes.push(node2);

        // Node 3: Follower 2
        let node3 = Command::new(&binary)
            .args([
                "--port",
                &FOLLOWER2_PORT.to_string(),
                "--node-id",
                "2",
                "--peers",
                &format!("{},{}", LEADER_PORT, FOLLOWER1_PORT),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("Failed to launch node 3")?;
        self.processes.push(node3);

        println!("  Nodes launched, waiting for startup...");
        Ok(())
    }

    fn kill_all(&mut self) {
        for process in &mut self.processes {
            let _ = process.kill();
        }
        self.processes.clear();
    }
}

impl Drop for NodeManager {
    fn drop(&mut self) {
        self.kill_all();
    }
}

async fn wait_for_nodes_ready() -> Result<()> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(2)).build()?;

    let ports = [LEADER_PORT, FOLLOWER1_PORT, FOLLOWER2_PORT];

    for _ in 0..30 {
        let mut all_ready = true;
        for port in ports {
            let url = format!("http://127.0.0.1:{}/_raft/health", port);
            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {}
                _ => {
                    all_ready = false;
                    break;
                }
            }
        }
        if all_ready {
            println!("  All nodes ready!");
            return Ok(());
        }
        sleep(Duration::from_millis(500)).await;
    }

    anyhow::bail!("Nodes did not become ready within timeout")
}

/// Shared state for tracking created IDs (for updates/deletes)
struct KnownIds {
    items: RwLock<Vec<uuid::Uuid>>,
    orders: RwLock<Vec<uuid::Uuid>>,
}

impl KnownIds {
    fn new() -> Self {
        Self { items: RwLock::new(Vec::new()), orders: RwLock::new(Vec::new()) }
    }
}

/// CRUD operation type (matching playground ratios: 40/30/20/10)
#[derive(Clone, Copy)]
enum CrudOp {
    Create,
    Read,
    Update,
    Delete,
}

impl CrudOp {
    fn random() -> Self {
        let r = fastrand::u8(0..100);
        match r {
            0..=39 => CrudOp::Create,  // 40%
            40..=69 => CrudOp::Read,   // 30%
            70..=89 => CrudOp::Update, // 20%
            _ => CrudOp::Delete,       // 10%
        }
    }
}

/// Table type
#[derive(Clone, Copy, PartialEq)]
enum TableType {
    Items,
    Orders,
    Logs,
}

impl TableType {
    fn random() -> Self {
        match fastrand::u8(0..3) {
            0 => TableType::Items,
            1 => TableType::Orders,
            _ => TableType::Logs,
        }
    }

    fn endpoint(&self) -> &'static str {
        match self {
            TableType::Items => "/api/items",
            TableType::Orders => "/api/orders",
            TableType::Logs => "/api/logs",
        }
    }
}

async fn run_benchmark(args: &Args, stats: Arc<BenchmarkStats>) -> Result<()> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(30)).build()?;

    let leader_url = format!("http://127.0.0.1:{}", LEADER_PORT);
    let known_ids = Arc::new(KnownIds::new());

    println!("\nRunning benchmark (CRUD mix: 40% create, 30% read, 20% update, 10% delete):");
    println!("  Total operations: {}", args.ops * 3);
    println!("  Concurrency: {}", args.concurrency);

    let start = Instant::now();
    let semaphore = Arc::new(tokio::sync::Semaphore::new(args.concurrency));
    let mut handles = Vec::new();

    // Run mixed CRUD operations (total = ops * 3 to match original multi-table behavior)
    let total_ops = args.ops * 3;

    for i in 0..total_ops {
        let client = client.clone();
        let base_url = leader_url.clone();
        let stats = stats.clone();
        let sem = semaphore.clone();
        let known = known_ids.clone();

        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();
            let table = TableType::random();
            let op = CrudOp::random();

            let op_start = Instant::now();
            let success = match (op, table) {
                // CREATE operations
                (CrudOp::Create, TableType::Items) => {
                    let payload = serde_json::json!({
                        "name": format!("benchmark-item-{}", uuid::Uuid::new_v4()),
                        "description": format!("Benchmark description {}", i),
                        "priority": (i % 11) as i32,
                        "tags": ["benchmark", "test"],
                        "status": "Draft"
                    });
                    let url = format!("{}/api/items", base_url);
                    match client.post(&url).json(&payload).send().await {
                        Ok(resp) if resp.status().is_success() => {
                            if let Ok(data) = resp.json::<serde_json::Value>().await {
                                if let Some(id) = data.get("id").and_then(|v| v.as_str()) {
                                    if let Ok(uuid) = uuid::Uuid::parse_str(id) {
                                        let mut ids = known.items.write().await;
                                        if ids.len() < 500 {
                                            ids.push(uuid);
                                        }
                                    }
                                }
                            }
                            stats.creates.fetch_add(1, Ordering::Relaxed);
                            true
                        }
                        _ => {
                            stats.errors.fetch_add(1, Ordering::Relaxed);
                            false
                        }
                    }
                }
                (CrudOp::Create, TableType::Orders) => {
                    let payload = serde_json::json!({
                        "customer_id": format!("customer-{}", (i % 1000) + 1),
                        "status": "Pending",
                        "total_cents": ((i % 1000) * 100 + 100) as i64,
                        "item_count": ((i % 20) + 1) as i32,
                        "shipping_address": format!("Address {}", i),
                        "notes": format!("Notes {}", i)
                    });
                    let url = format!("{}/api/orders", base_url);
                    match client.post(&url).json(&payload).send().await {
                        Ok(resp) if resp.status().is_success() => {
                            if let Ok(data) = resp.json::<serde_json::Value>().await {
                                if let Some(id) = data.get("id").and_then(|v| v.as_str()) {
                                    if let Ok(uuid) = uuid::Uuid::parse_str(id) {
                                        let mut ids = known.orders.write().await;
                                        if ids.len() < 500 {
                                            ids.push(uuid);
                                        }
                                    }
                                }
                            }
                            stats.creates.fetch_add(1, Ordering::Relaxed);
                            true
                        }
                        _ => {
                            stats.errors.fetch_add(1, Ordering::Relaxed);
                            false
                        }
                    }
                }
                (CrudOp::Create, TableType::Logs) => {
                    let levels = ["Info", "Warning", "Error", "Debug"];
                    let payload = serde_json::json!({
                        "level": levels[i % 4],
                        "action": format!("benchmark-action-{}", (i % 100) + 1),
                        "entity_type": "BenchmarkEntity",
                        "entity_id": uuid::Uuid::new_v4().to_string(),
                        "details": {"benchmark": true, "worker": i % 100},
                        "source_node": LEADER_PORT as u64
                    });
                    let url = format!("{}/api/logs", base_url);
                    match client.post(&url).json(&payload).send().await {
                        Ok(resp) if resp.status().is_success() => {
                            stats.creates.fetch_add(1, Ordering::Relaxed);
                            true
                        }
                        _ => {
                            stats.errors.fetch_add(1, Ordering::Relaxed);
                            false
                        }
                    }
                }

                // READ operations
                (CrudOp::Read, _) => {
                    let url = format!("{}{}", base_url, table.endpoint());
                    match client.get(&url).send().await {
                        Ok(resp) if resp.status().is_success() => true,
                        _ => false,
                    }
                }

                // UPDATE operations (items and orders only, logs are append-only)
                (CrudOp::Update, TableType::Items) => {
                    let id = {
                        let ids = known.items.read().await;
                        if ids.is_empty() {
                            None
                        } else {
                            Some(ids[fastrand::usize(..ids.len())])
                        }
                    };
                    if let Some(id) = id {
                        let statuses = ["Draft", "Active", "Archived"];
                        let payload = serde_json::json!({
                            "description": format!("Updated {}", i),
                            "priority": (i % 11) as i32,
                            "status": statuses[i % 3]
                        });
                        let url = format!("{}/api/items/{}", base_url, id);
                        match client.put(&url).json(&payload).send().await {
                            Ok(resp) if resp.status().is_success() => {
                                stats.updates.fetch_add(1, Ordering::Relaxed);
                                true
                            }
                            _ => {
                                stats.errors.fetch_add(1, Ordering::Relaxed);
                                false
                            }
                        }
                    } else {
                        // No items to update, do a create instead
                        false
                    }
                }
                (CrudOp::Update, TableType::Orders) => {
                    let id = {
                        let ids = known.orders.read().await;
                        if ids.is_empty() {
                            None
                        } else {
                            Some(ids[fastrand::usize(..ids.len())])
                        }
                    };
                    if let Some(id) = id {
                        let statuses = ["Pending", "Confirmed", "Processing", "Shipped"];
                        let payload = serde_json::json!({
                            "status": statuses[i % 4],
                            "notes": format!("Updated notes {}", i)
                        });
                        let url = format!("{}/api/orders/{}", base_url, id);
                        match client.put(&url).json(&payload).send().await {
                            Ok(resp) if resp.status().is_success() => {
                                stats.updates.fetch_add(1, Ordering::Relaxed);
                                true
                            }
                            _ => {
                                stats.errors.fetch_add(1, Ordering::Relaxed);
                                false
                            }
                        }
                    } else {
                        false
                    }
                }
                (CrudOp::Update, TableType::Logs) => {
                    // Logs are append-only, do a create instead
                    false
                }

                // DELETE operations (items and orders only)
                (CrudOp::Delete, TableType::Items) => {
                    let id = {
                        let mut ids = known.items.write().await;
                        if ids.is_empty() {
                            None
                        } else {
                            Some(ids.pop().unwrap())
                        }
                    };
                    if let Some(id) = id {
                        let url = format!("{}/api/items/{}", base_url, id);
                        match client.delete(&url).send().await {
                            Ok(resp) if resp.status().is_success() => {
                                stats.deletes.fetch_add(1, Ordering::Relaxed);
                                true
                            }
                            _ => {
                                stats.errors.fetch_add(1, Ordering::Relaxed);
                                false
                            }
                        }
                    } else {
                        false
                    }
                }
                (CrudOp::Delete, TableType::Orders) => {
                    let id = {
                        let mut ids = known.orders.write().await;
                        if ids.is_empty() {
                            None
                        } else {
                            Some(ids.pop().unwrap())
                        }
                    };
                    if let Some(id) = id {
                        let url = format!("{}/api/orders/{}", base_url, id);
                        match client.delete(&url).send().await {
                            Ok(resp) if resp.status().is_success() => {
                                stats.deletes.fetch_add(1, Ordering::Relaxed);
                                true
                            }
                            _ => {
                                stats.errors.fetch_add(1, Ordering::Relaxed);
                                false
                            }
                        }
                    } else {
                        false
                    }
                }
                (CrudOp::Delete, TableType::Logs) => {
                    // Logs are append-only
                    false
                }
            };

            if success {
                stats
                    .total_latency_ms
                    .fetch_add(op_start.elapsed().as_millis() as usize, Ordering::Relaxed);
            }
        }));
    }

    // Wait for all operations to complete
    join_all(handles).await;

    let elapsed = start.elapsed();
    let total_ops = stats.creates.load(Ordering::Relaxed)
        + stats.updates.load(Ordering::Relaxed)
        + stats.deletes.load(Ordering::Relaxed);

    println!("\nBenchmark completed in {:.2}s", elapsed.as_secs_f64());
    println!("  Creates: {}", stats.creates.load(Ordering::Relaxed));
    println!("  Updates: {}", stats.updates.load(Ordering::Relaxed));
    println!("  Deletes: {}", stats.deletes.load(Ordering::Relaxed));
    println!("  Errors: {}", stats.errors.load(Ordering::Relaxed));
    println!("  Throughput: {:.0} ops/sec", total_ops as f64 / elapsed.as_secs_f64());

    Ok(())
}

async fn get_count(client: &reqwest::Client, port: u16, table: &str) -> Result<usize> {
    let url = format!("http://127.0.0.1:{}/{}", port, table);
    let resp = client.get(&url).send().await?;
    let items: Vec<serde_json::Value> = resp.json().await?;
    Ok(items.len())
}

async fn check_consistency(settle_time: u64) -> Result<Vec<ConsistencyResult>> {
    println!("\nWaiting {}ms for replication to settle...", settle_time);
    sleep(Duration::from_millis(settle_time)).await;

    println!("Checking consistency across nodes...");

    let client = reqwest::Client::builder().timeout(Duration::from_secs(10)).build()?;

    let mut results = Vec::new();

    for table in TABLES {
        let leader_count = get_count(&client, LEADER_PORT, table).await?;
        let follower1_count = get_count(&client, FOLLOWER1_PORT, table).await?;
        let follower2_count = get_count(&client, FOLLOWER2_PORT, table).await?;

        let consistent = leader_count == follower1_count && follower1_count == follower2_count;

        results.push(ConsistencyResult {
            table: table.to_string(),
            leader_count,
            follower1_count,
            follower2_count,
            consistent,
        });
    }

    Ok(results)
}

fn print_results(results: &[ConsistencyResult]) {
    println!("\n╔════════════════════════════════════════════════════════════╗");
    println!("║                   CONSISTENCY RESULTS                       ║");
    println!("╠════════════════════════════════════════════════════════════╣");
    println!("║ Table    │ Leader  │ Follower1 │ Follower2 │ Status        ║");
    println!("╠══════════╪═════════╪═══════════╪═══════════╪═══════════════╣");

    let mut all_consistent = true;
    for result in results {
        let status = if result.consistent { "✓ OK" } else { "✗ MISMATCH" };
        if !result.consistent {
            all_consistent = false;
        }
        println!(
            "║ {:<8} │ {:>7} │ {:>9} │ {:>9} │ {:<13} ║",
            result.table,
            result.leader_count,
            result.follower1_count,
            result.follower2_count,
            status
        );
    }

    println!("╚════════════════════════════════════════════════════════════╝");

    if all_consistent {
        println!("\n✓ All tables are CONSISTENT!");
    } else {
        println!("\n✗ INCONSISTENCY DETECTED!");
        for result in results {
            if !result.consistent {
                let diff1 = result.leader_count as i64 - result.follower1_count as i64;
                let diff2 = result.leader_count as i64 - result.follower2_count as i64;
                println!(
                    "  {} - Leader has {} more than F1, {} more than F2",
                    result.table, diff1, diff2
                );
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let mut node_manager = NodeManager::new();

    if !args.skip_launch {
        // Launch nodes
        node_manager.launch_nodes()?;

        // Wait for nodes to be ready
        wait_for_nodes_ready().await?;

        // Small delay for cluster formation
        println!("  Waiting for cluster formation...");
        sleep(Duration::from_secs(2)).await;
    } else {
        println!("Skipping node launch, using existing nodes...");
    }

    // Run benchmark
    let stats = Arc::new(BenchmarkStats::default());
    run_benchmark(&args, stats).await?;

    // Check consistency
    let results = check_consistency(args.settle_time).await?;

    // Print results
    print_results(&results);

    // Exit with error code if inconsistent
    let all_consistent = results.iter().all(|r| r.consistent);
    if !all_consistent {
        std::process::exit(1);
    }

    Ok(())
}
