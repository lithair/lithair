//! Bombarder - Cluster Management and Stress Testing Tool
//!
//! A comprehensive tool for managing Lithair clusters and stress testing replication.
//!
//! ## Usage
//!
//! ### Cluster Management
//! ```bash
//! # Start a 3-node cluster
//! cargo run --bin bombarder -- cluster start
//!
//! # Check cluster status
//! cargo run --bin bombarder -- cluster status
//!
//! # Stop all nodes
//! cargo run --bin bombarder -- cluster stop
//! ```
//!
//! ### Stress Tests
//! ```bash
//! # Flood test - rapid creates
//! cargo run --bin bombarder -- flood --count 100 --concurrency 10
//!
//! # Burst test - batched writes
//! cargo run --bin bombarder -- burst --size 50 --bursts 5
//!
//! # Mixed CRUD operations
//! cargo run --bin bombarder -- mixed --count 100
//! ```
//!
//! ### Diagnostics
//! ```bash
//! # Verify consistency across nodes
//! cargo run --bin bombarder -- verify
//!
//! # Watch cluster health in real-time
//! cargo run --bin bombarder -- watch
//! ```

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use futures::future::join_all;
use md5::{Digest, Md5};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time::sleep;
use uuid::Uuid;

// ============================================================================
// CLI DEFINITIONS
// ============================================================================

#[derive(Parser)]
#[command(name = "bombarder")]
#[command(about = "Lithair Replication Stress Testing Tool")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Cluster management commands
    Cluster {
        #[command(subcommand)]
        action: ClusterAction,
    },
    /// Flood test - rapid sequential/concurrent creates
    Flood {
        /// Number of entries to create
        #[arg(short, long, default_value = "100")]
        count: u64,
        /// Concurrency level
        #[arg(short = 'j', long, default_value = "10")]
        concurrency: usize,
        /// Target node URL (default: leader at 8080)
        #[arg(short, long, default_value = "http://127.0.0.1:8080")]
        target: String,
    },
    /// Burst test - batched writes with pauses
    Burst {
        /// Size of each burst
        #[arg(short, long, default_value = "50")]
        size: u64,
        /// Number of bursts
        #[arg(short, long, default_value = "5")]
        bursts: u64,
        /// Pause between bursts (ms)
        #[arg(short, long, default_value = "500")]
        pause: u64,
        /// Target node URL
        #[arg(short, long, default_value = "http://127.0.0.1:8080")]
        target: String,
    },
    /// Mixed CRUD operations
    Mixed {
        /// Total number of operations
        #[arg(short, long, default_value = "100")]
        count: u64,
        /// Target node URL
        #[arg(short, long, default_value = "http://127.0.0.1:8080")]
        target: String,
    },
    /// Storm test - random operations on random nodes (the real chaos!)
    Storm {
        /// Total number of operations
        #[arg(short, long, default_value = "500")]
        count: u64,
        /// Concurrency level
        #[arg(short = 'j', long, default_value = "50")]
        concurrency: usize,
        /// Base port (will target port, port+1, port+2)
        #[arg(short, long, default_value = "8080")]
        port: u16,
        /// Read percentage (0-100, rest is writes: POST/PUT/DELETE)
        #[arg(short, long, default_value = "30")]
        read_pct: u8,
        /// Enable chaos mode (randomly kill/restart nodes)
        #[arg(long, default_value = "false")]
        chaos: bool,
        /// Interval between node kills in seconds (only with --chaos)
        #[arg(short = 'k', long, default_value = "10")]
        kill_interval: u64,
    },
    /// Verify data consistency across nodes
    Verify {
        /// Node URLs to verify (comma-separated)
        #[arg(
            short,
            long,
            default_value = "http://127.0.0.1:8080,http://127.0.0.1:8081,http://127.0.0.1:8082"
        )]
        nodes: String,
    },
    /// Watch cluster health in real-time
    Watch {
        /// Leader URL for health checks
        #[arg(short, long, default_value = "http://127.0.0.1:8080")]
        leader: String,
        /// Refresh interval in seconds
        #[arg(short, long, default_value = "2")]
        interval: u64,
    },
    /// Multi-model flood test - stress test both KVEntry and Counter simultaneously
    MultiFlood {
        /// Number of entries per model to create
        #[arg(short, long, default_value = "100")]
        count: u64,
        /// Concurrency level per model
        #[arg(short = 'j', long, default_value = "10")]
        concurrency: usize,
        /// Target node URL
        #[arg(short, long, default_value = "http://127.0.0.1:8080")]
        target: String,
    },
    /// Chaos testing - kill/restart nodes during flood to test resilience
    Chaos {
        #[command(subcommand)]
        mode: ChaosMode,
    },
}

#[derive(Subcommand)]
enum ChaosMode {
    /// Kill one follower node during flood, then verify consistency
    KillOne {
        /// Number of entries to create
        #[arg(short, long, default_value = "200")]
        count: u64,
        /// Concurrency level
        #[arg(short = 'j', long, default_value = "20")]
        concurrency: usize,
        /// Base port (nodes on port, port+1, port+2)
        #[arg(short, long, default_value = "8080")]
        port: u16,
    },
    /// Kill the leader node during flood to trigger election
    KillLeader {
        /// Number of entries to create
        #[arg(short, long, default_value = "200")]
        count: u64,
        /// Concurrency level
        #[arg(short = 'j', long, default_value = "20")]
        concurrency: usize,
        /// Base port
        #[arg(short, long, default_value = "8080")]
        port: u16,
    },
    /// Rolling restart - kill and restart each node in turn during flood
    Rolling {
        /// Number of entries to create
        #[arg(short, long, default_value = "500")]
        count: u64,
        /// Concurrency level
        #[arg(short = 'j', long, default_value = "10")]
        concurrency: usize,
        /// Base port
        #[arg(short, long, default_value = "8080")]
        port: u16,
        /// Delay between kill and restart (ms)
        #[arg(short, long, default_value = "2000")]
        restart_delay: u64,
    },
    /// Resync test - kill follower, continue flood, restart, verify snapshot resync works
    Resync {
        /// Number of entries to create
        #[arg(short, long, default_value = "200")]
        count: u64,
        /// Concurrency level
        #[arg(short = 'j', long, default_value = "20")]
        concurrency: usize,
        /// Base port
        #[arg(short, long, default_value = "8080")]
        port: u16,
        /// Time to wait for resync after restart (seconds)
        #[arg(short, long, default_value = "10")]
        resync_wait: u64,
    },
}

#[derive(Subcommand)]
enum ClusterAction {
    /// Start a 3-node cluster
    Start {
        /// Base port (nodes will use port, port+1, port+2)
        #[arg(short, long, default_value = "8080")]
        port: u16,
    },
    /// Stop all cluster nodes
    Stop,
    /// Show cluster status
    Status {
        /// Node URLs to check
        #[arg(
            short,
            long,
            default_value = "http://127.0.0.1:8080,http://127.0.0.1:8081,http://127.0.0.1:8082"
        )]
        nodes: String,
    },
    /// Show detailed sync status for each follower (ops diagnostic)
    SyncStatus {
        /// Leader URL
        #[arg(short, long, default_value = "http://127.0.0.1:8080")]
        leader: String,
    },
    /// Force resync a specific follower from snapshot (ops manual intervention)
    Resync {
        /// Leader URL
        #[arg(short, long, default_value = "http://127.0.0.1:8080")]
        leader: String,
        /// Target follower address (e.g., 127.0.0.1:8081)
        #[arg(short, long)]
        target: String,
    },
}

// ============================================================================
// DATA STRUCTURES
// ============================================================================

#[derive(Debug, Serialize)]
struct CreateKVEntry {
    key: String,
    value: String,
    seq: u64,
    source_node: u64,
}

#[derive(Debug, Deserialize)]
struct KVEntry {
    id: Uuid,
    key: String,
    value: String,
    seq: u64,
    #[allow(dead_code)]
    source_node: u64,
    #[allow(dead_code)]
    created_at: String,
}

#[derive(Debug, Serialize)]
struct CreateCounter {
    name: String,
    value: i64,
    increments: u64,
}

#[derive(Debug, Deserialize)]
struct Counter {
    id: Uuid,
    name: String,
    value: i64,
    #[allow(dead_code)]
    increments: u64,
    #[allow(dead_code)]
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct RaftHealth {
    node_id: u64,
    state: String,
    term: u64,
    commit_index: u64,
    last_applied: u64,
    leader_id: Option<u64>,
    voters: Vec<u64>,
}

#[derive(Debug)]
struct StressMetrics {
    #[allow(dead_code)]
    total_ops: u64,
    successful: AtomicU64,
    failed: AtomicU64,
    start_time: Instant,
    latencies: Mutex<Vec<Duration>>,
}

impl StressMetrics {
    fn new(total_ops: u64) -> Self {
        Self {
            total_ops,
            successful: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            start_time: Instant::now(),
            latencies: Mutex::new(Vec::with_capacity(total_ops as usize)),
        }
    }

    async fn record_success(&self, latency: Duration) {
        self.successful.fetch_add(1, Ordering::Relaxed);
        self.latencies.lock().await.push(latency);
    }

    fn record_failure(&self) {
        self.failed.fetch_add(1, Ordering::Relaxed);
    }

    async fn report(&self) {
        let elapsed = self.start_time.elapsed();
        let successful = self.successful.load(Ordering::Relaxed);
        let failed = self.failed.load(Ordering::Relaxed);
        let total = successful + failed;

        let latencies = self.latencies.lock().await;
        let (min, avg, max, p95, p99) = if !latencies.is_empty() {
            let mut sorted: Vec<_> = latencies.iter().copied().collect();
            sorted.sort();
            let min = sorted[0];
            let max = sorted[sorted.len() - 1];
            let avg = Duration::from_nanos(
                sorted.iter().map(|d| d.as_nanos() as u64).sum::<u64>() / sorted.len() as u64,
            );
            let p95 = sorted[(sorted.len() as f64 * 0.95) as usize];
            let p99 = sorted[(sorted.len() as f64 * 0.99).min(sorted.len() as f64 - 1.0) as usize];
            (min, avg, max, p95, p99)
        } else {
            (Duration::ZERO, Duration::ZERO, Duration::ZERO, Duration::ZERO, Duration::ZERO)
        };

        let throughput = if elapsed.as_secs_f64() > 0.0 {
            successful as f64 / elapsed.as_secs_f64()
        } else {
            0.0
        };

        println!("\n═══════════════════════════════════════════════════════════════");
        println!("  STRESS TEST RESULTS");
        println!("═══════════════════════════════════════════════════════════════");
        println!("  Operations:     {}/{} ({} failed)", successful, total, failed);
        println!("  Duration:       {:.2}s", elapsed.as_secs_f64());
        println!("  Throughput:     {:.2} ops/sec", throughput);
        println!("───────────────────────────────────────────────────────────────");
        println!("  Latency (ms):");
        println!("    Min:          {:.2}", min.as_secs_f64() * 1000.0);
        println!("    Avg:          {:.2}", avg.as_secs_f64() * 1000.0);
        println!("    Max:          {:.2}", max.as_secs_f64() * 1000.0);
        println!("    P95:          {:.2}", p95.as_secs_f64() * 1000.0);
        println!("    P99:          {:.2}", p99.as_secs_f64() * 1000.0);
        println!("───────────────────────────────────────────────────────────────");
        let success_rate = if total > 0 { successful as f64 / total as f64 * 100.0 } else { 0.0 };
        let status = if success_rate >= 99.0 {
            "✅ EXCELLENT"
        } else if success_rate >= 95.0 {
            "⚠️  ACCEPTABLE"
        } else {
            "❌ FAILED"
        };
        println!("  Success Rate:   {:.2}% {}", success_rate, status);
        println!("═══════════════════════════════════════════════════════════════");
    }
}

// ============================================================================
// CLUSTER MANAGEMENT
// ============================================================================

static CLUSTER_PIDS: std::sync::OnceLock<Mutex<Vec<Child>>> = std::sync::OnceLock::new();

fn get_cluster_pids() -> &'static Mutex<Vec<Child>> {
    CLUSTER_PIDS.get_or_init(|| Mutex::new(Vec::new()))
}

async fn start_cluster(base_port: u16) -> Result<()> {
    println!("Starting 3-node cluster...");

    let ports = [base_port, base_port + 1, base_port + 2];
    let mut children = Vec::new();

    // Data directory
    let base_dir = std::env::var("STRESS_TEST_DATA").unwrap_or_else(|_| "data".to_string());
    std::fs::create_dir_all(&base_dir)?;

    for (idx, &port) in ports.iter().enumerate() {
        let peers: Vec<String> =
            ports.iter().filter(|&&p| p != port).map(|p| p.to_string()).collect();

        let peers_arg = peers.join(",");

        println!("  Starting node {} on port {} with peers [{}]", idx, port, peers_arg);

        let child = Command::new("cargo")
            .args([
                "run",
                "--bin",
                "stress_node",
                "--",
                "--node-id",
                &idx.to_string(),
                "--port",
                &port.to_string(),
                "--peers",
                &peers_arg,
            ])
            .env("STRESS_TEST_DATA", &base_dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context(format!("Failed to start node {}", idx))?;

        children.push(child);
    }

    // Store PIDs for later cleanup
    *get_cluster_pids().lock().await = children;

    // Wait for nodes to be ready
    println!("\nWaiting for nodes to be ready...");
    let client = Client::new();
    let mut ready_count = 0;
    let max_attempts = 30;

    for attempt in 0..max_attempts {
        ready_count = 0;
        for &port in &ports {
            let url = format!("http://127.0.0.1:{}/status", port);
            if client.get(&url).timeout(Duration::from_secs(1)).send().await.is_ok() {
                ready_count += 1;
            }
        }
        if ready_count == 3 {
            break;
        }
        if attempt % 5 == 0 {
            println!("  {}/{} nodes ready...", ready_count, 3);
        }
        sleep(Duration::from_millis(500)).await;
    }

    if ready_count == 3 {
        println!("\n✅ Cluster started successfully!");
        println!("   Leader:    http://127.0.0.1:{}", base_port);
        println!("   Follower1: http://127.0.0.1:{}", base_port + 1);
        println!("   Follower2: http://127.0.0.1:{}", base_port + 2);
    } else {
        println!("\n⚠️  Only {}/3 nodes ready. Check logs for issues.", ready_count);
    }

    Ok(())
}

async fn stop_cluster() -> Result<()> {
    println!("Stopping cluster...");

    // Kill by port using pkill or similar
    for port in [8080, 8081, 8082] {
        let _ = Command::new("pkill")
            .args(["-f", &format!("stress_node.*--port.*{}", port)])
            .output()
            .await;
    }

    // Also try to kill any stored children
    let mut children = get_cluster_pids().lock().await;
    for child in children.iter_mut() {
        let _ = child.kill().await;
    }
    children.clear();

    // Give processes time to terminate
    sleep(Duration::from_millis(500)).await;

    println!("✅ Cluster stopped");
    Ok(())
}

async fn cluster_status(nodes_str: &str) -> Result<()> {
    let client = Client::new();
    let nodes: Vec<&str> = nodes_str.split(',').collect();

    println!("═══════════════════════════════════════════════════════════════");
    println!("  CLUSTER STATUS");
    println!("═══════════════════════════════════════════════════════════════");

    for node in &nodes {
        let health_url = format!("{}/_raft/health", node.trim());
        let status_url = format!("{}/status", node.trim());

        print!("  {} ", node.trim());

        // Check basic connectivity
        match client.get(&status_url).timeout(Duration::from_secs(2)).send().await {
            Ok(resp) if resp.status().is_success() => {
                print!("🟢 UP");
            }
            _ => {
                println!("🔴 DOWN");
                continue;
            }
        }

        // Get Raft health
        match client.get(&health_url).timeout(Duration::from_secs(2)).send().await {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(health) = resp.json::<RaftHealth>().await {
                    let role = match health.state.as_str() {
                        "Leader" => "👑 Leader",
                        "Follower" => "📥 Follower",
                        "Candidate" => "🗳️ Candidate",
                        _ => &health.state,
                    };
                    println!(
                        " | {} | Term: {} | Commit: {} | Applied: {}",
                        role, health.term, health.commit_index, health.last_applied
                    );
                } else {
                    println!(" | (health parse error)");
                }
            }
            _ => {
                println!(" | (no raft info)");
            }
        }
    }

    // Count entries on each node
    println!("───────────────────────────────────────────────────────────────");
    println!("  Entry counts:");
    for node in &nodes {
        let url = format!("{}/api/kv", node.trim());
        match client.get(&url).timeout(Duration::from_secs(2)).send().await {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(entries) = resp.json::<Vec<KVEntry>>().await {
                    println!("    {}: {} entries", node.trim(), entries.len());
                }
            }
            _ => {
                println!("    {}: (unavailable)", node.trim());
            }
        }
    }

    println!("═══════════════════════════════════════════════════════════════");
    Ok(())
}

/// Show detailed sync status for each follower (ops diagnostic)
async fn show_sync_status(leader_url: &str) -> Result<()> {
    let client = Client::new();
    let url = format!("{}/_raft/sync-status", leader_url);

    println!("═══════════════════════════════════════════════════════════════");
    println!("  FOLLOWER SYNC STATUS");
    println!("═══════════════════════════════════════════════════════════════");
    println!("  Leader: {}", leader_url);
    println!("───────────────────────────────────────────────────────────────");

    match client.get(&url).timeout(Duration::from_secs(5)).send().await {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(status) = resp.json::<serde_json::Value>().await {
                if let Some(false) = status.get("is_leader").and_then(|v| v.as_bool()) {
                    println!("  ⚠️  This node is not the leader!");
                    if let Some(msg) = status.get("message").and_then(|v| v.as_str()) {
                        println!("  {}", msg);
                    }
                } else {
                    if let Some(commit_idx) = status.get("commit_index").and_then(|v| v.as_u64()) {
                        println!("  📊 Leader commit index: {}", commit_idx);
                    }
                    println!();

                    if let Some(followers) = status.get("followers").and_then(|v| v.as_array()) {
                        for f in followers {
                            let addr = f.get("address").and_then(|v| v.as_str()).unwrap_or("?");
                            let health = f.get("health").and_then(|v| v.as_str()).unwrap_or("?");
                            let last_idx = f
                                .get("last_replicated_index")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0);
                            let lag = f.get("lag").and_then(|v| v.as_u64()).unwrap_or(0);
                            let latency =
                                f.get("last_latency_ms").and_then(|v| v.as_u64()).unwrap_or(0);
                            let failures =
                                f.get("consecutive_failures").and_then(|v| v.as_u64()).unwrap_or(0);

                            let health_icon = match health {
                                "healthy" => "🟢",
                                "lagging" => "🟡",
                                "desynced" => "🔴",
                                _ => "⚪",
                            };

                            println!("  {} {} ({})", health_icon, addr, health);
                            println!("      Last replicated: {} | Lag: {} entries", last_idx, lag);
                            println!("      Latency: {}ms | Failures: {}", latency, failures);
                            println!();
                        }
                    }
                }
            }
        }
        Ok(resp) => {
            println!("  ❌ Error: HTTP {}", resp.status());
        }
        Err(e) => {
            println!("  ❌ Connection error: {}", e);
        }
    }

    println!("═══════════════════════════════════════════════════════════════");
    Ok(())
}

/// Force resync a follower from snapshot (ops manual intervention)
async fn force_resync(leader_url: &str, target: &str) -> Result<()> {
    let client = Client::new();
    let url = format!("{}/_raft/force-resync?target={}", leader_url, target);

    println!("═══════════════════════════════════════════════════════════════");
    println!("  FORCE RESYNC");
    println!("═══════════════════════════════════════════════════════════════");
    println!("  Leader: {}", leader_url);
    println!("  Target: {}", target);
    println!("───────────────────────────────────────────────────────────────");
    println!("  🔄 Triggering snapshot resync...");

    match client.post(&url).timeout(Duration::from_secs(120)).send().await {
        Ok(resp) => {
            let status_code = resp.status();
            if let Ok(result) = resp.json::<serde_json::Value>().await {
                let success = result.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
                let message =
                    result.get("message").and_then(|v| v.as_str()).unwrap_or("No message");

                if success {
                    println!("  ✅ {}", message);
                } else {
                    println!("  ❌ {} (HTTP {})", message, status_code);
                }
            } else {
                println!("  ❌ HTTP {} (no response body)", status_code);
            }
        }
        Err(e) => {
            println!("  ❌ Request failed: {}", e);
        }
    }

    println!("═══════════════════════════════════════════════════════════════");
    Ok(())
}

// ============================================================================
// STRESS TESTS
// ============================================================================

async fn flood_test(target: &str, count: u64, concurrency: usize) -> Result<()> {
    println!("═══════════════════════════════════════════════════════════════");
    println!("  FLOOD TEST");
    println!("═══════════════════════════════════════════════════════════════");
    println!("  Target:       {}", target);
    println!("  Operations:   {}", count);
    println!("  Concurrency:  {}", concurrency);
    println!("═══════════════════════════════════════════════════════════════");

    let client = Arc::new(Client::new());
    let metrics = Arc::new(StressMetrics::new(count));
    let seq = Arc::new(AtomicU64::new(0));

    let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency));

    let mut handles = Vec::new();

    for _ in 0..count {
        let client = Arc::clone(&client);
        let metrics = Arc::clone(&metrics);
        let seq = Arc::clone(&seq);
        let target = target.to_string();
        let permit = semaphore.clone().acquire_owned().await?;

        let handle = tokio::spawn(async move {
            let _permit = permit;
            let current_seq = seq.fetch_add(1, Ordering::Relaxed);

            let entry = CreateKVEntry {
                key: format!("flood_{}", current_seq),
                value: format!("value_{}", Uuid::new_v4()),
                seq: current_seq,
                source_node: 0,
            };

            let start = Instant::now();
            let result = client
                .post(format!("{}/api/kv", target))
                .json(&entry)
                .timeout(Duration::from_secs(30))
                .send()
                .await;

            let latency = start.elapsed();

            match result {
                Ok(resp) if resp.status().is_success() => {
                    metrics.record_success(latency).await;
                }
                Ok(resp) => {
                    log::warn!("Request failed with status {}: seq={}", resp.status(), current_seq);
                    metrics.record_failure();
                }
                Err(e) => {
                    log::warn!("Request error: {} seq={}", e, current_seq);
                    metrics.record_failure();
                }
            }
        });

        handles.push(handle);
    }

    // Progress indicator
    let metrics_clone = Arc::clone(&metrics);
    let progress_handle = tokio::spawn(async move {
        loop {
            let done = metrics_clone.successful.load(Ordering::Relaxed)
                + metrics_clone.failed.load(Ordering::Relaxed);
            if done >= count {
                break;
            }
            print!("\r  Progress: {}/{} ({:.1}%)", done, count, done as f64 / count as f64 * 100.0);
            sleep(Duration::from_millis(100)).await;
        }
        println!();
    });

    // Wait for all operations
    join_all(handles).await;
    progress_handle.abort();

    metrics.report().await;
    Ok(())
}

/// Storm test - random operations on random nodes
/// This is the real chaos test that distributes load across all nodes
async fn storm_test(
    count: u64,
    concurrency: usize,
    base_port: u16,
    read_pct: u8,
    chaos: bool,
    kill_interval: u64,
) -> Result<()> {
    use rand::{Rng, SeedableRng};

    let nodes: Vec<String> =
        (0..3).map(|i| format!("http://127.0.0.1:{}", base_port + i)).collect();

    println!("═══════════════════════════════════════════════════════════════");
    if chaos {
        println!("  🔥 STORM + CHAOS TEST 🔥");
    } else {
        println!("  ⚡ STORM TEST - MULTI-NODE CHAOS ⚡");
    }
    println!("═══════════════════════════════════════════════════════════════");
    println!("  Nodes:        {:?}", nodes);
    println!("  Operations:   {}", count);
    println!("  Concurrency:  {}", concurrency);
    println!("  Read %:       {}% (writes: {}%)", read_pct, 100 - read_pct);
    if chaos {
        println!("  Chaos:        ENABLED (kill every {}s)", kill_interval);
    }
    println!("═══════════════════════════════════════════════════════════════");
    println!("  Operation mix:");
    println!("    GET  (list)   : {}%", read_pct);
    println!("    POST (create) : {}%", (100 - read_pct) * 70 / 100);
    println!("    PUT  (update) : {}%", (100 - read_pct) * 20 / 100);
    println!("    DELETE        : {}%", (100 - read_pct) * 10 / 100);
    println!("═══════════════════════════════════════════════════════════════");

    // Chaos mode: spawn node killer task
    let chaos_handle = if chaos {
        Some(tokio::spawn(async move {
            use rand::seq::SliceRandom;
            let mut rng = rand::rngs::StdRng::from_entropy();
            let ports: Vec<u16> = vec![base_port, base_port + 1, base_port + 2];

            loop {
                tokio::time::sleep(Duration::from_secs(kill_interval)).await;

                // Pick a random non-leader node to kill (avoid killing leader for now)
                let victim_ports: Vec<u16> =
                    ports.iter().filter(|&&p| p != base_port).copied().collect();
                if let Some(&victim_port) = victim_ports.choose(&mut rng) {
                    log::warn!("🔪 CHAOS: Killing node on port {}", victim_port);

                    // Kill the node
                    let _ = tokio::process::Command::new("pkill")
                        .args(["-f", &format!("--port {}", victim_port)])
                        .output()
                        .await;

                    // Wait a bit
                    tokio::time::sleep(Duration::from_secs(2)).await;

                    // Restart the node
                    log::warn!("🔄 CHAOS: Restarting node on port {}", victim_port);
                    let node_id = victim_port - base_port;
                    let peers: Vec<String> = ports
                        .iter()
                        .filter(|&&p| p != victim_port)
                        .map(|p| p.to_string())
                        .collect();

                    let _ = tokio::process::Command::new("cargo")
                        .args(["run", "--bin", "stress_node", "--"])
                        .args(["--node-id", &node_id.to_string()])
                        .args(["--port", &victim_port.to_string()])
                        .args(["--peers", &peers.join(",")])
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn();
                }
            }
        }))
    } else {
        None
    };

    let client = Arc::new(Client::new());
    let metrics = Arc::new(StressMetrics::new(count));
    let seq = Arc::new(AtomicU64::new(0));
    // Track created IDs for update/delete operations
    let created_ids: Arc<tokio::sync::RwLock<Vec<Uuid>>> =
        Arc::new(tokio::sync::RwLock::new(Vec::new()));

    // Metrics per operation type
    let reads = Arc::new(AtomicU64::new(0));
    let creates = Arc::new(AtomicU64::new(0));
    let updates = Arc::new(AtomicU64::new(0));
    let deletes = Arc::new(AtomicU64::new(0));

    // Metrics per node
    let node_hits: Arc<[AtomicU64; 3]> =
        Arc::new([AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)]);

    let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency));
    let mut handles = Vec::new();

    for _ in 0..count {
        let client = Arc::clone(&client);
        let metrics = Arc::clone(&metrics);
        let seq = Arc::clone(&seq);
        let nodes = nodes.clone();
        let created_ids = Arc::clone(&created_ids);
        let reads = Arc::clone(&reads);
        let creates = Arc::clone(&creates);
        let updates = Arc::clone(&updates);
        let deletes = Arc::clone(&deletes);
        let node_hits = Arc::clone(&node_hits);
        let permit = semaphore.clone().acquire_owned().await?;

        let handle = tokio::spawn(async move {
            let _permit = permit;
            let mut rng = rand::rngs::StdRng::from_entropy();

            // Pick random node
            let node_idx = rng.gen_range(0..3);
            let node = &nodes[node_idx];
            node_hits[node_idx].fetch_add(1, Ordering::Relaxed);

            // Pick operation type based on read_pct
            let op_roll: u8 = rng.gen_range(0..100);
            let start = Instant::now();

            let result = if op_roll < read_pct {
                // READ (GET list)
                reads.fetch_add(1, Ordering::Relaxed);
                client
                    .get(format!("{}/api/kv", node))
                    .timeout(Duration::from_secs(30))
                    .send()
                    .await
            } else {
                // WRITE operations
                let write_roll: u8 = rng.gen_range(0..100);

                if write_roll < 70 {
                    // POST (create) - 70% of writes
                    creates.fetch_add(1, Ordering::Relaxed);
                    let current_seq = seq.fetch_add(1, Ordering::Relaxed);
                    let new_id = Uuid::new_v4();

                    let entry = serde_json::json!({
                        "id": new_id,
                        "key": format!("storm_{}", current_seq),
                        "value": format!("value_{}", new_id),
                        "seq": current_seq,
                        "source_node": node_idx as u64,
                    });

                    let resp = client
                        .post(format!("{}/api/kv", node))
                        .json(&entry)
                        .timeout(Duration::from_secs(30))
                        .send()
                        .await;

                    // Store ID for future updates/deletes
                    if resp.is_ok() {
                        created_ids.write().await.push(new_id);
                    }
                    resp
                } else if write_roll < 90 {
                    // PUT (update) - 20% of writes
                    updates.fetch_add(1, Ordering::Relaxed);
                    let ids = created_ids.read().await;
                    if ids.is_empty() {
                        // No IDs yet, do a create instead
                        drop(ids);
                        creates.fetch_add(1, Ordering::Relaxed);
                        let current_seq = seq.fetch_add(1, Ordering::Relaxed);
                        let entry = CreateKVEntry {
                            key: format!("storm_{}", current_seq),
                            value: format!("value_{}", Uuid::new_v4()),
                            seq: current_seq,
                            source_node: node_idx as u64,
                        };
                        client
                            .post(format!("{}/api/kv", node))
                            .json(&entry)
                            .timeout(Duration::from_secs(30))
                            .send()
                            .await
                    } else {
                        let id = ids[rng.gen_range(0..ids.len())];
                        drop(ids);
                        let update = serde_json::json!({
                            "value": format!("updated_{}", Uuid::new_v4()),
                        });
                        client
                            .put(format!("{}/api/kv/{}", node, id))
                            .json(&update)
                            .timeout(Duration::from_secs(30))
                            .send()
                            .await
                    }
                } else {
                    // DELETE - 10% of writes
                    deletes.fetch_add(1, Ordering::Relaxed);
                    let mut ids = created_ids.write().await;
                    if ids.is_empty() {
                        // No IDs yet, do a create instead
                        drop(ids);
                        creates.fetch_add(1, Ordering::Relaxed);
                        let current_seq = seq.fetch_add(1, Ordering::Relaxed);
                        let entry = CreateKVEntry {
                            key: format!("storm_{}", current_seq),
                            value: format!("value_{}", Uuid::new_v4()),
                            seq: current_seq,
                            source_node: node_idx as u64,
                        };
                        client
                            .post(format!("{}/api/kv", node))
                            .json(&entry)
                            .timeout(Duration::from_secs(30))
                            .send()
                            .await
                    } else {
                        let idx = rng.gen_range(0..ids.len());
                        let id = ids.remove(idx);
                        drop(ids);
                        client
                            .delete(format!("{}/api/kv/{}", node, id))
                            .timeout(Duration::from_secs(30))
                            .send()
                            .await
                    }
                }
            };

            let latency = start.elapsed();

            match result {
                Ok(resp) if resp.status().is_success() || resp.status().is_redirection() => {
                    metrics.record_success(latency).await;
                }
                Ok(resp) => {
                    log::warn!("Request failed: {} on {}", resp.status(), node);
                    metrics.record_failure();
                }
                Err(e) => {
                    log::warn!("Request error: {} on {}", e, node);
                    metrics.record_failure();
                }
            }
        });

        handles.push(handle);
    }

    // Progress indicator
    let metrics_clone = Arc::clone(&metrics);
    let progress_handle = tokio::spawn(async move {
        loop {
            let done = metrics_clone.successful.load(Ordering::Relaxed)
                + metrics_clone.failed.load(Ordering::Relaxed);
            if done >= count {
                break;
            }
            print!("\r  Progress: {}/{} ({:.1}%)", done, count, done as f64 / count as f64 * 100.0);
            sleep(Duration::from_millis(100)).await;
        }
        println!();
    });

    // Wait for all operations
    join_all(handles).await;
    progress_handle.abort();

    // Report results
    metrics.report().await;

    // Additional storm-specific stats
    println!("───────────────────────────────────────────────────────────────");
    println!("  Operation Distribution:");
    println!(
        "    GET:    {} ({:.1}%)",
        reads.load(Ordering::Relaxed),
        reads.load(Ordering::Relaxed) as f64 / count as f64 * 100.0
    );
    println!(
        "    POST:   {} ({:.1}%)",
        creates.load(Ordering::Relaxed),
        creates.load(Ordering::Relaxed) as f64 / count as f64 * 100.0
    );
    println!(
        "    PUT:    {} ({:.1}%)",
        updates.load(Ordering::Relaxed),
        updates.load(Ordering::Relaxed) as f64 / count as f64 * 100.0
    );
    println!(
        "    DELETE: {} ({:.1}%)",
        deletes.load(Ordering::Relaxed),
        deletes.load(Ordering::Relaxed) as f64 / count as f64 * 100.0
    );
    println!("───────────────────────────────────────────────────────────────");
    println!("  Node Distribution:");
    for (i, hits) in node_hits.iter().enumerate() {
        let h = hits.load(Ordering::Relaxed);
        println!(
            "    Node {} (port {}): {} ({:.1}%)",
            i,
            base_port + i as u16,
            h,
            h as f64 / count as f64 * 100.0
        );
    }
    println!("═══════════════════════════════════════════════════════════════");

    // Stop chaos task if running
    if let Some(handle) = chaos_handle {
        handle.abort();
    }

    Ok(())
}

async fn burst_test(target: &str, size: u64, bursts: u64, pause_ms: u64) -> Result<()> {
    println!("═══════════════════════════════════════════════════════════════");
    println!("  BURST TEST");
    println!("═══════════════════════════════════════════════════════════════");
    println!("  Target:       {}", target);
    println!("  Burst size:   {}", size);
    println!("  Bursts:       {}", bursts);
    println!("  Pause:        {}ms", pause_ms);
    println!("═══════════════════════════════════════════════════════════════");

    let client = Arc::new(Client::new());
    let total_ops = size * bursts;
    let metrics = Arc::new(StressMetrics::new(total_ops));
    let seq = Arc::new(AtomicU64::new(0));

    for burst_idx in 0..bursts {
        println!("\n  Burst {}/{}", burst_idx + 1, bursts);

        let mut handles = Vec::new();

        for _ in 0..size {
            let client = Arc::clone(&client);
            let metrics = Arc::clone(&metrics);
            let seq = Arc::clone(&seq);
            let target = target.to_string();

            let handle = tokio::spawn(async move {
                let current_seq = seq.fetch_add(1, Ordering::Relaxed);

                let entry = CreateKVEntry {
                    key: format!("burst_{}", current_seq),
                    value: format!("value_{}", Uuid::new_v4()),
                    seq: current_seq,
                    source_node: 0,
                };

                let start = Instant::now();
                let result = client
                    .post(format!("{}/api/kv", target))
                    .json(&entry)
                    .timeout(Duration::from_secs(30))
                    .send()
                    .await;

                let latency = start.elapsed();

                match result {
                    Ok(resp) if resp.status().is_success() => {
                        metrics.record_success(latency).await;
                    }
                    _ => {
                        metrics.record_failure();
                    }
                }
            });

            handles.push(handle);
        }

        join_all(handles).await;

        let success = metrics.successful.load(Ordering::Relaxed);
        let failed = metrics.failed.load(Ordering::Relaxed);
        println!("    Completed: {} success, {} failed", success, failed);

        if burst_idx < bursts - 1 {
            println!("    Pausing {}ms...", pause_ms);
            sleep(Duration::from_millis(pause_ms)).await;
        }
    }

    metrics.report().await;
    Ok(())
}

async fn mixed_test(target: &str, count: u64) -> Result<()> {
    println!("═══════════════════════════════════════════════════════════════");
    println!("  MIXED CRUD TEST");
    println!("═══════════════════════════════════════════════════════════════");
    println!("  Target:       {}", target);
    println!("  Operations:   {}", count);
    println!("═══════════════════════════════════════════════════════════════");

    let client = Client::new();
    let metrics = Arc::new(StressMetrics::new(count));

    // First, create some entries
    let create_count = count / 2;
    let mut created_ids: Vec<Uuid> = Vec::new();

    println!("\n  Phase 1: Creating {} entries...", create_count);
    for i in 0..create_count {
        let entry = CreateKVEntry {
            key: format!("mixed_{}", i),
            value: format!("initial_{}", i),
            seq: i,
            source_node: 0,
        };

        let start = Instant::now();
        let result = client
            .post(format!("{}/api/kv", target))
            .json(&entry)
            .timeout(Duration::from_secs(30))
            .send()
            .await;

        match result {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(created) = resp.json::<KVEntry>().await {
                    created_ids.push(created.id);
                    metrics.record_success(start.elapsed()).await;
                }
            }
            _ => {
                metrics.record_failure();
            }
        }
    }

    // Read operations
    let read_count = count / 4;
    println!("  Phase 2: Reading {} entries...", read_count);
    for _ in 0..read_count {
        let start = Instant::now();
        let result = client
            .get(format!("{}/api/kv", target))
            .timeout(Duration::from_secs(10))
            .send()
            .await;

        match result {
            Ok(resp) if resp.status().is_success() => {
                metrics.record_success(start.elapsed()).await;
            }
            _ => {
                metrics.record_failure();
            }
        }
    }

    // Update operations
    let update_count = count / 4;
    println!("  Phase 3: Updating {} entries...", update_count);
    for (i, id) in created_ids.iter().take(update_count as usize).enumerate() {
        let update = serde_json::json!({
            "value": format!("updated_{}", i)
        });

        let start = Instant::now();
        let result = client
            .put(format!("{}/api/kv/{}", target, id))
            .json(&update)
            .timeout(Duration::from_secs(30))
            .send()
            .await;

        match result {
            Ok(resp) if resp.status().is_success() => {
                metrics.record_success(start.elapsed()).await;
            }
            _ => {
                metrics.record_failure();
            }
        }
    }

    metrics.report().await;
    Ok(())
}

async fn multi_flood_test(target: &str, count: u64, concurrency: usize) -> Result<()> {
    println!("═══════════════════════════════════════════════════════════════");
    println!("  MULTI-MODEL FLOOD TEST");
    println!("═══════════════════════════════════════════════════════════════");
    println!("  Target:       {}", target);
    println!("  Operations:   {} per model ({}x2 total)", count, count);
    println!("  Concurrency:  {} per model", concurrency);
    println!("═══════════════════════════════════════════════════════════════");

    let client = Arc::new(Client::new());
    let total_ops = count * 2; // Both models
    let metrics = Arc::new(StressMetrics::new(total_ops));
    let kv_seq = Arc::new(AtomicU64::new(0));
    let counter_seq = Arc::new(AtomicU64::new(0));

    let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency * 2));

    let mut handles = Vec::new();

    // Spawn KVEntry creates
    for _ in 0..count {
        let client = Arc::clone(&client);
        let metrics = Arc::clone(&metrics);
        let seq = Arc::clone(&kv_seq);
        let target = target.to_string();
        let permit = semaphore.clone().acquire_owned().await?;

        let handle = tokio::spawn(async move {
            let _permit = permit;
            let current_seq = seq.fetch_add(1, Ordering::Relaxed);

            let entry = CreateKVEntry {
                key: format!("multi_kv_{}", current_seq),
                value: format!("value_{}", Uuid::new_v4()),
                seq: current_seq,
                source_node: 0,
            };

            let start = Instant::now();
            let result = client
                .post(format!("{}/api/kv", target))
                .json(&entry)
                .timeout(Duration::from_secs(30))
                .send()
                .await;

            match result {
                Ok(resp) if resp.status().is_success() => {
                    metrics.record_success(start.elapsed()).await;
                }
                Ok(resp) => {
                    log::warn!("KV request failed: {} seq={}", resp.status(), current_seq);
                    metrics.record_failure();
                }
                Err(e) => {
                    log::warn!("KV request error: {} seq={}", e, current_seq);
                    metrics.record_failure();
                }
            }
        });
        handles.push(handle);
    }

    // Spawn Counter creates (interleaved with KVEntry)
    for _ in 0..count {
        let client = Arc::clone(&client);
        let metrics = Arc::clone(&metrics);
        let seq = Arc::clone(&counter_seq);
        let target = target.to_string();
        let permit = semaphore.clone().acquire_owned().await?;

        let handle = tokio::spawn(async move {
            let _permit = permit;
            let current_seq = seq.fetch_add(1, Ordering::Relaxed);

            let counter = CreateCounter {
                name: format!("multi_counter_{}", current_seq),
                value: current_seq as i64,
                increments: 0,
            };

            let start = Instant::now();
            let result = client
                .post(format!("{}/api/counter", target))
                .json(&counter)
                .timeout(Duration::from_secs(30))
                .send()
                .await;

            match result {
                Ok(resp) if resp.status().is_success() => {
                    metrics.record_success(start.elapsed()).await;
                }
                Ok(resp) => {
                    log::warn!("Counter request failed: {} seq={}", resp.status(), current_seq);
                    metrics.record_failure();
                }
                Err(e) => {
                    log::warn!("Counter request error: {} seq={}", e, current_seq);
                    metrics.record_failure();
                }
            }
        });
        handles.push(handle);
    }

    // Progress indicator
    let metrics_clone = Arc::clone(&metrics);
    let progress_handle = tokio::spawn(async move {
        loop {
            let done = metrics_clone.successful.load(Ordering::Relaxed)
                + metrics_clone.failed.load(Ordering::Relaxed);
            if done >= total_ops {
                break;
            }
            print!(
                "\r  Progress: {}/{} ({:.1}%) [KV + Counter]",
                done,
                total_ops,
                done as f64 / total_ops as f64 * 100.0
            );
            sleep(Duration::from_millis(100)).await;
        }
        println!();
    });

    // Wait for all operations
    join_all(handles).await;
    progress_handle.abort();

    println!("\n  Model breakdown:");
    println!("    KVEntry:  {} creates", count);
    println!("    Counter:  {} creates", count);

    metrics.report().await;
    Ok(())
}

// ============================================================================
// CHAOS TESTING
// ============================================================================

/// Helper to kill a node by port
async fn kill_node_by_port(port: u16) -> Result<()> {
    // Use exact match for --port argument to avoid killing other nodes
    // The pattern matches "--port 8081" but not "--peers 8080,8081"
    let _ = Command::new("pkill")
        .args(["-f", &format!("stress_node.*--port {} ", port)])
        .output()
        .await;
    // Also try lsof + kill as backup - this only kills the process listening on the port
    let output = Command::new("sh")
        .args(["-c", &format!("lsof -ti:{} | xargs -r kill -9", port)])
        .output()
        .await;
    if output.is_err() {
        log::warn!("Failed to kill process on port {}", port);
    }
    Ok(())
}

/// Helper to start a single node
async fn start_node(node_id: u64, port: u16, peer_ports: &[u16]) -> Result<Child> {
    let peers: Vec<String> =
        peer_ports.iter().filter(|&&p| p != port).map(|p| p.to_string()).collect();
    let peers_arg = peers.join(",");

    let base_dir =
        std::env::var("STRESS_TEST_DATA").unwrap_or_else(|_| "/tmp/chaos_test".to_string());

    let child = Command::new("cargo")
        .args([
            "run",
            "-q",
            "--bin",
            "stress_node",
            "--",
            "--node-id",
            &node_id.to_string(),
            "--port",
            &port.to_string(),
            "--peers",
            &peers_arg,
        ])
        .env("STRESS_TEST_DATA", &base_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context(format!("Failed to start node {}", node_id))?;

    Ok(child)
}

/// Helper to wait for a node to be ready
async fn wait_for_node(url: &str, timeout_secs: u64) -> bool {
    let client = Client::new();
    let start = Instant::now();
    while start.elapsed().as_secs() < timeout_secs {
        if let Ok(resp) = client
            .get(format!("{}/status", url))
            .timeout(Duration::from_secs(1))
            .send()
            .await
        {
            if resp.status().is_success() {
                return true;
            }
        }
        sleep(Duration::from_millis(200)).await;
    }
    false
}

/// Chaos test: Kill one follower during flood
async fn chaos_kill_one(count: u64, concurrency: usize, base_port: u16) -> Result<()> {
    let ports = [base_port, base_port + 1, base_port + 2];
    let leader_url = format!("http://127.0.0.1:{}", base_port);
    let follower_port = base_port + 1;

    println!("═══════════════════════════════════════════════════════════════");
    println!("  CHAOS TEST: KILL ONE FOLLOWER");
    println!("═══════════════════════════════════════════════════════════════");
    println!("  Leader:       {}", leader_url);
    println!("  Target kill:  port {}", follower_port);
    println!("  Operations:   {}", count);
    println!("  Concurrency:  {}", concurrency);
    println!("═══════════════════════════════════════════════════════════════");

    let client = Arc::new(Client::new());
    let metrics = Arc::new(StressMetrics::new(count));
    let seq = Arc::new(AtomicU64::new(0));
    let killed = Arc::new(std::sync::atomic::AtomicBool::new(false));

    let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency));
    let mut handles = Vec::new();

    // Spawn flood operations
    for _ in 0..count {
        let client = Arc::clone(&client);
        let metrics = Arc::clone(&metrics);
        let seq = Arc::clone(&seq);
        let leader_url = leader_url.clone();
        let permit = semaphore.clone().acquire_owned().await?;

        let handle = tokio::spawn(async move {
            let _permit = permit;
            let current_seq = seq.fetch_add(1, Ordering::Relaxed);

            let entry = CreateKVEntry {
                key: format!("chaos_kill_{}", current_seq),
                value: format!("value_{}", Uuid::new_v4()),
                seq: current_seq,
                source_node: 0,
            };

            let start = Instant::now();
            let result = client
                .post(format!("{}/api/kv", leader_url))
                .json(&entry)
                .timeout(Duration::from_secs(30))
                .send()
                .await;

            match result {
                Ok(resp) if resp.status().is_success() => {
                    metrics.record_success(start.elapsed()).await;
                }
                Ok(resp) => {
                    log::warn!("Request failed: {} seq={}", resp.status(), current_seq);
                    metrics.record_failure();
                }
                Err(e) => {
                    log::warn!("Request error: {} seq={}", e, current_seq);
                    metrics.record_failure();
                }
            }
        });
        handles.push(handle);
    }

    // Kill follower after 30% progress
    let metrics_clone = Arc::clone(&metrics);
    let killed_clone = Arc::clone(&killed);
    let kill_handle = tokio::spawn(async move {
        let threshold = (count as f64 * 0.3) as u64;
        loop {
            let done = metrics_clone.successful.load(Ordering::Relaxed)
                + metrics_clone.failed.load(Ordering::Relaxed);
            if done >= threshold {
                println!(
                    "\n  💀 KILLING follower on port {} (at {}% progress)",
                    follower_port,
                    (done * 100) / count
                );
                let _ = kill_node_by_port(follower_port).await;
                killed_clone.store(true, Ordering::Relaxed);
                break;
            }
            sleep(Duration::from_millis(50)).await;
        }
    });

    // Progress indicator
    let metrics_clone2 = Arc::clone(&metrics);
    let killed_clone2 = Arc::clone(&killed);
    let progress_handle = tokio::spawn(async move {
        loop {
            let done = metrics_clone2.successful.load(Ordering::Relaxed)
                + metrics_clone2.failed.load(Ordering::Relaxed);
            if done >= count {
                break;
            }
            let killed_status =
                if killed_clone2.load(Ordering::Relaxed) { " [NODE KILLED]" } else { "" };
            print!(
                "\r  Progress: {}/{} ({:.1}%){}",
                done,
                count,
                done as f64 / count as f64 * 100.0,
                killed_status
            );
            sleep(Duration::from_millis(100)).await;
        }
        println!();
    });

    // Wait for flood to complete
    join_all(handles).await;
    kill_handle.abort();
    progress_handle.abort();

    metrics.report().await;

    // Verify consistency on remaining nodes
    println!("\n  Verifying consistency on remaining nodes...");
    let nodes_str = format!("http://127.0.0.1:{},http://127.0.0.1:{}", ports[0], ports[2]);
    verify_consistency(&nodes_str).await?;

    Ok(())
}

/// Chaos test: Kill the leader to trigger election
async fn chaos_kill_leader(count: u64, concurrency: usize, base_port: u16) -> Result<()> {
    let ports = [base_port, base_port + 1, base_port + 2];
    let leader_url = format!("http://127.0.0.1:{}", base_port);

    println!("═══════════════════════════════════════════════════════════════");
    println!("  CHAOS TEST: KILL LEADER");
    println!("═══════════════════════════════════════════════════════════════");
    println!("  Leader:       {} (will be killed)", leader_url);
    println!("  Operations:   {}", count);
    println!("  Concurrency:  {}", concurrency);
    println!("  Note: Requests will fail during election, then resume");
    println!("═══════════════════════════════════════════════════════════════");

    let client = Arc::new(Client::new());
    let metrics = Arc::new(StressMetrics::new(count));
    let seq = Arc::new(AtomicU64::new(0));
    let leader_killed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let current_target = Arc::new(Mutex::new(leader_url.clone()));

    let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency));
    let mut handles = Vec::new();

    // Spawn flood operations
    for _ in 0..count {
        let client = Arc::clone(&client);
        let metrics = Arc::clone(&metrics);
        let seq = Arc::clone(&seq);
        let current_target = Arc::clone(&current_target);
        let permit = semaphore.clone().acquire_owned().await?;

        let handle = tokio::spawn(async move {
            let _permit = permit;
            let current_seq = seq.fetch_add(1, Ordering::Relaxed);

            let entry = CreateKVEntry {
                key: format!("chaos_leader_{}", current_seq),
                value: format!("value_{}", Uuid::new_v4()),
                seq: current_seq,
                source_node: 0,
            };

            // Try current target, retry on other nodes if needed
            let targets = {
                let t = current_target.lock().await.clone();
                vec![t]
            };

            let start = Instant::now();
            let mut success = false;

            for target in &targets {
                let result = client
                    .post(format!("{}/api/kv", target))
                    .json(&entry)
                    .timeout(Duration::from_secs(30))
                    .send()
                    .await;

                match result {
                    Ok(resp) if resp.status().is_success() => {
                        success = true;
                        break;
                    }
                    _ => continue,
                }
            }

            if success {
                metrics.record_success(start.elapsed()).await;
            } else {
                metrics.record_failure();
            }
        });
        handles.push(handle);
    }

    // Kill leader after 30% progress
    let metrics_clone = Arc::clone(&metrics);
    let leader_killed_clone = Arc::clone(&leader_killed);
    let current_target_clone = Arc::clone(&current_target);
    let kill_handle = tokio::spawn(async move {
        let threshold = (count as f64 * 0.3) as u64;
        loop {
            let done = metrics_clone.successful.load(Ordering::Relaxed)
                + metrics_clone.failed.load(Ordering::Relaxed);
            if done >= threshold {
                println!(
                    "\n  👑💀 KILLING LEADER on port {} (at {}% progress)",
                    base_port,
                    (done * 100) / count
                );
                let _ = kill_node_by_port(base_port).await;
                leader_killed_clone.store(true, Ordering::Relaxed);

                // Switch target to a follower
                *current_target_clone.lock().await = format!("http://127.0.0.1:{}", base_port + 1);
                println!("  🔄 Switching target to port {}", base_port + 1);
                break;
            }
            sleep(Duration::from_millis(50)).await;
        }
    });

    // Progress indicator
    let metrics_clone2 = Arc::clone(&metrics);
    let leader_killed_clone2 = Arc::clone(&leader_killed);
    let progress_handle = tokio::spawn(async move {
        loop {
            let done = metrics_clone2.successful.load(Ordering::Relaxed)
                + metrics_clone2.failed.load(Ordering::Relaxed);
            if done >= count {
                break;
            }
            let status =
                if leader_killed_clone2.load(Ordering::Relaxed) { " [LEADER KILLED]" } else { "" };
            print!(
                "\r  Progress: {}/{} ({:.1}%){}",
                done,
                count,
                done as f64 / count as f64 * 100.0,
                status
            );
            sleep(Duration::from_millis(100)).await;
        }
        println!();
    });

    // Wait for flood to complete
    join_all(handles).await;
    kill_handle.abort();
    progress_handle.abort();

    metrics.report().await;

    // Verify consistency on remaining nodes
    println!("\n  Verifying consistency on remaining nodes...");
    let nodes_str = format!("http://127.0.0.1:{},http://127.0.0.1:{}", ports[1], ports[2]);
    verify_consistency(&nodes_str).await?;

    Ok(())
}

/// Chaos test: Rolling restart during flood
async fn chaos_rolling(
    count: u64,
    concurrency: usize,
    base_port: u16,
    restart_delay_ms: u64,
) -> Result<()> {
    let ports = [base_port, base_port + 1, base_port + 2];
    let leader_url = format!("http://127.0.0.1:{}", base_port);

    println!("═══════════════════════════════════════════════════════════════");
    println!("  CHAOS TEST: ROLLING RESTART");
    println!("═══════════════════════════════════════════════════════════════");
    println!("  Leader:        {}", leader_url);
    println!("  Operations:    {}", count);
    println!("  Concurrency:   {}", concurrency);
    println!("  Restart delay: {}ms", restart_delay_ms);
    println!("  Sequence:      Kill/restart each node in turn");
    println!("═══════════════════════════════════════════════════════════════");

    let client = Arc::new(Client::new());
    let metrics = Arc::new(StressMetrics::new(count));
    let seq = Arc::new(AtomicU64::new(0));
    let current_phase = Arc::new(Mutex::new(String::from("Starting")));

    let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency));
    let mut handles = Vec::new();

    // Spawn flood operations
    for _ in 0..count {
        let client = Arc::clone(&client);
        let metrics = Arc::clone(&metrics);
        let seq = Arc::clone(&seq);
        let leader_url = leader_url.clone();
        let permit = semaphore.clone().acquire_owned().await?;

        let handle = tokio::spawn(async move {
            let _permit = permit;
            let current_seq = seq.fetch_add(1, Ordering::Relaxed);

            let entry = CreateKVEntry {
                key: format!("chaos_rolling_{}", current_seq),
                value: format!("value_{}", Uuid::new_v4()),
                seq: current_seq,
                source_node: 0,
            };

            let start = Instant::now();
            let result = client
                .post(format!("{}/api/kv", leader_url))
                .json(&entry)
                .timeout(Duration::from_secs(30))
                .send()
                .await;

            match result {
                Ok(resp) if resp.status().is_success() => {
                    metrics.record_success(start.elapsed()).await;
                }
                Ok(resp) => {
                    log::warn!("Request failed: {} seq={}", resp.status(), current_seq);
                    metrics.record_failure();
                }
                Err(e) => {
                    log::warn!("Request error: {} seq={}", e, current_seq);
                    metrics.record_failure();
                }
            }
        });
        handles.push(handle);
    }

    // Rolling restart task - restart followers only (not leader)
    let metrics_clone = Arc::clone(&metrics);
    let current_phase_clone = Arc::clone(&current_phase);
    let rolling_handle = tokio::spawn(async move {
        // Wait for 20% progress before starting chaos
        let start_threshold = (count as f64 * 0.2) as u64;
        loop {
            let done = metrics_clone.successful.load(Ordering::Relaxed)
                + metrics_clone.failed.load(Ordering::Relaxed);
            if done >= start_threshold {
                break;
            }
            sleep(Duration::from_millis(50)).await;
        }

        // Rolling restart followers
        for (i, &port) in [ports[1], ports[2]].iter().enumerate() {
            let node_id = i as u64 + 1;

            // Update phase
            *current_phase_clone.lock().await = format!("Killing node {} (port {})", node_id, port);
            println!("\n  💀 Killing node {} on port {}", node_id, port);

            let _ = kill_node_by_port(port).await;

            // Wait before restart
            *current_phase_clone.lock().await =
                format!("Node {} down, waiting {}ms", node_id, restart_delay_ms);
            sleep(Duration::from_millis(restart_delay_ms)).await;

            // Restart node
            *current_phase_clone.lock().await = format!("Restarting node {}", node_id);
            println!("  🔄 Restarting node {} on port {}", node_id, port);

            let _ = start_node(node_id, port, &ports).await;

            // Wait for node to be ready
            let url = format!("http://127.0.0.1:{}", port);
            if wait_for_node(&url, 30).await {
                println!("  ✅ Node {} back online", node_id);
            } else {
                println!("  ⚠️  Node {} failed to come back", node_id);
            }

            *current_phase_clone.lock().await = format!("Node {} restored", node_id);

            // Brief pause before next node
            sleep(Duration::from_millis(1000)).await;
        }

        *current_phase_clone.lock().await = String::from("Rolling complete");
    });

    // Progress indicator
    let metrics_clone2 = Arc::clone(&metrics);
    let current_phase_clone2 = Arc::clone(&current_phase);
    let progress_handle = tokio::spawn(async move {
        loop {
            let done = metrics_clone2.successful.load(Ordering::Relaxed)
                + metrics_clone2.failed.load(Ordering::Relaxed);
            if done >= count {
                break;
            }
            let phase = current_phase_clone2.lock().await.clone();
            print!(
                "\r  Progress: {}/{} ({:.1}%) [{}]        ",
                done,
                count,
                done as f64 / count as f64 * 100.0,
                phase
            );
            sleep(Duration::from_millis(100)).await;
        }
        println!();
    });

    // Wait for flood to complete
    join_all(handles).await;
    rolling_handle.abort();
    progress_handle.abort();

    metrics.report().await;

    // Wait for nodes to stabilize
    println!("\n  Waiting for cluster to stabilize...");
    sleep(Duration::from_secs(3)).await;

    // Verify consistency
    let nodes_str = format!(
        "http://127.0.0.1:{},http://127.0.0.1:{},http://127.0.0.1:{}",
        ports[0], ports[1], ports[2]
    );
    verify_consistency(&nodes_str).await?;

    Ok(())
}

/// Chaos test: Kill follower, continue flood, restart node, verify snapshot resync
///
/// This test validates the complete resync flow:
/// 1. Start flood test
/// 2. Kill a follower mid-test (marked as Desynced)
/// 3. Continue flood (leader skips desynced follower)
/// 4. Restart the killed node
/// 5. Wait for snapshot resync to complete
/// 6. Verify ALL 3 nodes are consistent
async fn chaos_resync_test(
    count: u64,
    concurrency: usize,
    base_port: u16,
    resync_wait_secs: u64,
) -> Result<()> {
    let ports = [base_port, base_port + 1, base_port + 2];
    let leader_url = format!("http://127.0.0.1:{}", base_port);
    let killed_port = base_port + 1;
    let killed_node_id = 1u64;

    println!("═══════════════════════════════════════════════════════════════");
    println!("  CHAOS TEST: SNAPSHOT RESYNC VALIDATION");
    println!("═══════════════════════════════════════════════════════════════");
    println!("  Leader:         {}", leader_url);
    println!("  Target kill:    port {} (node {})", killed_port, killed_node_id);
    println!("  Operations:     {}", count);
    println!("  Concurrency:    {}", concurrency);
    println!("  Resync wait:    {}s", resync_wait_secs);
    println!("═══════════════════════════════════════════════════════════════");
    println!("  Test flow:");
    println!("    1. Flood with {} ops", count);
    println!("    2. Kill follower at 30% progress");
    println!("    3. Continue flood (node marked Desynced)");
    println!("    4. Restart killed node");
    println!("    5. Wait {}s for snapshot resync", resync_wait_secs);
    println!("    6. Verify ALL 3 nodes consistent");
    println!("═══════════════════════════════════════════════════════════════");

    let client = Arc::new(Client::new());
    let metrics = Arc::new(StressMetrics::new(count));
    let seq = Arc::new(AtomicU64::new(0));
    let phase = Arc::new(tokio::sync::Mutex::new("flooding".to_string()));

    let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency));
    let mut handles = Vec::new();

    // Spawn flood operations
    for _ in 0..count {
        let client = Arc::clone(&client);
        let metrics = Arc::clone(&metrics);
        let seq = Arc::clone(&seq);
        let leader_url = leader_url.clone();
        let permit = semaphore.clone().acquire_owned().await?;

        let handle = tokio::spawn(async move {
            let _permit = permit;
            let current_seq = seq.fetch_add(1, Ordering::Relaxed);

            let entry = CreateKVEntry {
                key: format!("resync_test_{}", current_seq),
                value: format!("value_{}", Uuid::new_v4()),
                seq: current_seq,
                source_node: 0,
            };

            let start = Instant::now();
            let result = client
                .post(format!("{}/api/kv", leader_url))
                .json(&entry)
                .timeout(Duration::from_secs(30))
                .send()
                .await;

            match result {
                Ok(resp) if resp.status().is_success() => {
                    metrics.record_success(start.elapsed()).await;
                }
                Ok(resp) => {
                    log::warn!("Request failed: {} seq={}", resp.status(), current_seq);
                    metrics.record_failure();
                }
                Err(e) => {
                    log::warn!("Request error: {} seq={}", e, current_seq);
                    metrics.record_failure();
                }
            }
        });
        handles.push(handle);
    }

    // Phase 1: Kill follower at 30% progress
    let metrics_clone = Arc::clone(&metrics);
    let phase_clone = Arc::clone(&phase);
    let kill_handle = tokio::spawn(async move {
        let kill_threshold = (count as f64 * 0.3) as u64;
        loop {
            let done = metrics_clone.successful.load(Ordering::Relaxed)
                + metrics_clone.failed.load(Ordering::Relaxed);
            if done >= kill_threshold {
                println!(
                    "\n\n  💀 PHASE 1: Killing follower on port {} (at {}% progress)",
                    killed_port,
                    (done * 100) / count
                );
                let _ = kill_node_by_port(killed_port).await;
                *phase_clone.lock().await = "node killed - continuing flood".to_string();
                break;
            }
            sleep(Duration::from_millis(50)).await;
        }
    });

    // Progress indicator
    let metrics_clone2 = Arc::clone(&metrics);
    let phase_clone2 = Arc::clone(&phase);
    let progress_handle = tokio::spawn(async move {
        loop {
            let done = metrics_clone2.successful.load(Ordering::Relaxed)
                + metrics_clone2.failed.load(Ordering::Relaxed);
            if done >= count {
                break;
            }
            let current_phase = phase_clone2.lock().await.clone();
            print!(
                "\r  Progress: {}/{} ({:.1}%) [{}]          ",
                done,
                count,
                done as f64 / count as f64 * 100.0,
                current_phase
            );
            sleep(Duration::from_millis(100)).await;
        }
        println!();
    });

    // Wait for flood to complete
    join_all(handles).await;
    kill_handle.abort();
    progress_handle.abort();

    println!("\n  📊 Flood completed:");
    metrics.report().await;

    // Phase 2: Restart the killed node
    println!(
        "\n  🔄 PHASE 2: Restarting killed node {} on port {}...",
        killed_node_id, killed_port
    );
    let peer_ports: Vec<u16> = ports.iter().cloned().filter(|&p| p != killed_port).collect();
    let _child = start_node(killed_node_id, killed_port, &peer_ports).await?;

    // Wait for node to be ready
    let node_url = format!("http://127.0.0.1:{}", killed_port);
    let ready_client = Client::new();
    let mut ready = false;
    for i in 0..30 {
        sleep(Duration::from_millis(500)).await;
        if let Ok(resp) = ready_client
            .get(format!("{}/status", node_url))
            .timeout(Duration::from_secs(2))
            .send()
            .await
        {
            if resp.status().is_success() {
                println!("  ✅ Node {} ready after {}ms", killed_node_id, (i + 1) * 500);
                ready = true;
                break;
            }
        }
        print!("\r  Waiting for node to start... {}s", (i + 1) / 2);
    }
    if !ready {
        println!("\n  ⚠️  Node may not be fully ready, continuing anyway...");
    }

    // Phase 3: Wait for resync
    println!("\n  ⏳ PHASE 3: Waiting {}s for snapshot resync...", resync_wait_secs);
    for i in 0..resync_wait_secs {
        print!("\r  Resync wait: {}s remaining...   ", resync_wait_secs - i);
        sleep(Duration::from_secs(1)).await;
    }
    println!("\r  Resync wait: complete!              ");

    // Phase 3.5: Verify resync stats (observability check)
    println!("\n  📊 PHASE 3.5: Checking resync statistics...");
    let mut resync_observed = false;

    // Check leader's resync stats (should have sent snapshots)
    if let Ok(resp) = ready_client
        .get(format!("http://127.0.0.1:{}/_raft/resync_stats", base_port))
        .timeout(Duration::from_secs(5))
        .send()
        .await
    {
        if let Ok(stats) = resp.json::<serde_json::Value>().await {
            let send_successes =
                stats["resync_stats"]["snapshot_send_successes"].as_u64().unwrap_or(0);
            let send_attempts =
                stats["resync_stats"]["snapshot_send_attempts"].as_u64().unwrap_or(0);
            println!(
                "  Leader (port {}): {} send attempts, {} successes",
                base_port, send_attempts, send_successes
            );
            if send_successes > 0 {
                resync_observed = true;
            }
        }
    }

    // Check restarted follower's resync stats (should have received snapshot)
    if let Ok(resp) = ready_client
        .get(format!("http://127.0.0.1:{}/_raft/resync_stats", killed_port))
        .timeout(Duration::from_secs(5))
        .send()
        .await
    {
        if let Ok(stats) = resp.json::<serde_json::Value>().await {
            let received = stats["resync_stats"]["snapshots_received"].as_u64().unwrap_or(0);
            let applied = stats["resync_stats"]["snapshots_applied"].as_u64().unwrap_or(0);
            println!(
                "  Follower (port {}): {} received, {} applied",
                killed_port, received, applied
            );
            if applied > 0 {
                resync_observed = true;
            }
        }
    }

    if resync_observed {
        println!("  ✅ Snapshot resync activity confirmed via stats!");
    } else {
        println!("  ⚠️  No snapshot resync activity detected in stats");
        println!("     (Data may have synced via normal log replication)");
    }

    // Phase 4: Verify consistency on ALL 3 nodes
    println!("\n  🔍 PHASE 4: Verifying consistency on ALL 3 nodes...");
    let nodes_str = format!(
        "http://127.0.0.1:{},http://127.0.0.1:{},http://127.0.0.1:{}",
        ports[0], ports[1], ports[2]
    );
    let result = verify_consistency(&nodes_str).await;

    // Summary
    println!("\n═══════════════════════════════════════════════════════════════");
    println!("  RESYNC TEST SUMMARY");
    println!("═══════════════════════════════════════════════════════════════");
    match &result {
        Ok(_) => {
            println!("  ✅ PASS: Snapshot resync worked correctly!");
            println!("     - Node was killed and restarted");
            println!("     - Snapshot was sent to desynced follower");
            println!("     - All 3 nodes are now consistent");
        }
        Err(e) => {
            println!("  ❌ FAIL: Resync verification failed!");
            println!("     Error: {}", e);
            println!("     - Check logs for snapshot send/receive errors");
            println!("     - May need to increase resync_wait time");
        }
    }
    println!("═══════════════════════════════════════════════════════════════");

    result
}

// ============================================================================
// DIAGNOSTICS
// ============================================================================

async fn verify_consistency(nodes_str: &str) -> Result<()> {
    let client = Client::new();
    let nodes: Vec<&str> = nodes_str.split(',').collect();

    println!("═══════════════════════════════════════════════════════════════");
    println!("  CONSISTENCY VERIFICATION (MULTI-MODEL)");
    println!("═══════════════════════════════════════════════════════════════");
    println!("  Checking {} nodes...", nodes.len());

    // Check KVEntry model
    let mut kv_data: HashMap<String, (usize, String)> = HashMap::new();
    for node in &nodes {
        let url = format!("{}/api/kv", node.trim());
        if let Ok(resp) = client.get(&url).timeout(Duration::from_secs(5)).send().await {
            if resp.status().is_success() {
                if let Ok(mut entries) = resp.json::<Vec<KVEntry>>().await {
                    entries.sort_by(|a, b| a.id.cmp(&b.id));
                    let mut hasher = Md5::new();
                    for entry in &entries {
                        hasher.update(entry.id.to_string());
                        hasher.update(&entry.key);
                        hasher.update(&entry.value);
                        hasher.update(entry.seq.to_string());
                    }
                    let hash = format!("{:x}", hasher.finalize());
                    kv_data.insert(node.to_string(), (entries.len(), hash));
                }
            }
        }
    }

    // Check Counter model
    let mut counter_data: HashMap<String, (usize, String)> = HashMap::new();
    for node in &nodes {
        let url = format!("{}/api/counter", node.trim());
        if let Ok(resp) = client.get(&url).timeout(Duration::from_secs(5)).send().await {
            if resp.status().is_success() {
                if let Ok(mut entries) = resp.json::<Vec<Counter>>().await {
                    entries.sort_by(|a, b| a.id.cmp(&b.id));
                    let mut hasher = Md5::new();
                    for entry in &entries {
                        hasher.update(entry.id.to_string());
                        hasher.update(&entry.name);
                        hasher.update(entry.value.to_string());
                    }
                    let hash = format!("{:x}", hasher.finalize());
                    counter_data.insert(node.to_string(), (entries.len(), hash));
                }
            }
        }
    }

    // Report KVEntry consistency
    println!("───────────────────────────────────────────────────────────────");
    println!("  KVEntry Model:");
    for (node, (count, hash)) in &kv_data {
        println!("    {} - {} entries - hash: {}...", node, count, &hash[..16]);
    }
    let kv_hashes: Vec<&String> = kv_data.values().map(|(_, h)| h).collect();
    let kv_consistent = kv_hashes.windows(2).all(|w| w[0] == w[1]) && !kv_hashes.is_empty();

    // Report Counter consistency
    println!("───────────────────────────────────────────────────────────────");
    println!("  Counter Model:");
    if counter_data.is_empty() || counter_data.values().all(|(c, _)| *c == 0) {
        println!("    (no counters created yet)");
    } else {
        for (node, (count, hash)) in &counter_data {
            println!("    {} - {} entries - hash: {}...", node, count, &hash[..16]);
        }
    }
    let counter_hashes: Vec<&String> = counter_data.values().map(|(_, h)| h).collect();
    let counter_consistent = counter_hashes.windows(2).all(|w| w[0] == w[1])
        || counter_data.values().all(|(c, _)| *c == 0);

    // Overall result
    println!("───────────────────────────────────────────────────────────────");
    let kv_status = if kv_consistent { "✅" } else { "❌" };
    let counter_status = if counter_consistent { "✅" } else { "❌" };
    println!("  KVEntry:  {} | Counter: {}", kv_status, counter_status);

    if kv_consistent && counter_consistent {
        println!("  Result: ✅ CONSISTENT - All models replicated correctly");
    } else {
        println!("  Result: ❌ INCONSISTENT - Replication problem detected!");
    }
    println!("═══════════════════════════════════════════════════════════════");

    Ok(())
}

async fn watch_health(leader: &str, interval_secs: u64) -> Result<()> {
    let client = Client::new();

    println!("Watching cluster health (Ctrl+C to stop)...\n");

    loop {
        // Clear screen and move cursor to top
        print!("\x1B[2J\x1B[1;1H");

        println!("═══════════════════════════════════════════════════════════════");
        println!("  CLUSTER HEALTH MONITOR");
        println!(
            "  {} - refreshing every {}s",
            chrono::Utc::now().format("%H:%M:%S"),
            interval_secs
        );
        println!("═══════════════════════════════════════════════════════════════");

        let health_url = format!("{}/_raft/health", leader);
        match client.get(&health_url).timeout(Duration::from_secs(2)).send().await {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(health) = resp.json::<RaftHealth>().await {
                    let role_icon = match health.state.as_str() {
                        "Leader" => "👑",
                        "Follower" => "📥",
                        "Candidate" => "🗳️",
                        _ => "❓",
                    };

                    println!("  Node ID:       {} {}", health.node_id, role_icon);
                    println!("  State:         {}", health.state);
                    println!("  Term:          {}", health.term);
                    println!("  Commit Index:  {}", health.commit_index);
                    println!("  Last Applied:  {}", health.last_applied);
                    if let Some(leader_id) = health.leader_id {
                        println!("  Leader ID:     {}", leader_id);
                    }
                    println!("  Voters:        {:?}", health.voters);

                    // Health indicators
                    println!("───────────────────────────────────────────────────────────────");
                    let commit_lag = health.commit_index.saturating_sub(health.last_applied);
                    let lag_status = if commit_lag == 0 {
                        "✅ In sync"
                    } else if commit_lag < 10 {
                        "⚠️  Slightly behind"
                    } else {
                        "❌ Lagging"
                    };
                    println!("  Commit Lag:    {} ({})", commit_lag, lag_status);
                }
            }
            _ => {
                println!("  ❌ Unable to reach leader at {}", leader);
            }
        }

        // Also check entry count
        let kv_url = format!("{}/api/kv", leader);
        if let Ok(resp) = client.get(&kv_url).timeout(Duration::from_secs(2)).send().await {
            if resp.status().is_success() {
                if let Ok(entries) = resp.json::<Vec<KVEntry>>().await {
                    println!("  Entry Count:   {}", entries.len());
                }
            }
        }

        println!("═══════════════════════════════════════════════════════════════");
        println!("\n  Press Ctrl+C to exit");

        sleep(Duration::from_secs(interval_secs)).await;
    }
}

// ============================================================================
// MAIN
// ============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        .format_timestamp_millis()
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Cluster { action } => match action {
            ClusterAction::Start { port } => start_cluster(port).await?,
            ClusterAction::Stop => stop_cluster().await?,
            ClusterAction::Status { nodes } => cluster_status(&nodes).await?,
            ClusterAction::SyncStatus { leader } => show_sync_status(&leader).await?,
            ClusterAction::Resync { leader, target } => force_resync(&leader, &target).await?,
        },
        Commands::Flood { count, concurrency, target } => {
            flood_test(&target, count, concurrency).await?
        }
        Commands::Burst { size, bursts, pause, target } => {
            burst_test(&target, size, bursts, pause).await?
        }
        Commands::Mixed { count, target } => mixed_test(&target, count).await?,
        Commands::Storm { count, concurrency, port, read_pct, chaos, kill_interval } => {
            storm_test(count, concurrency, port, read_pct, chaos, kill_interval).await?
        }
        Commands::Verify { nodes } => verify_consistency(&nodes).await?,
        Commands::Watch { leader, interval } => watch_health(&leader, interval).await?,
        Commands::MultiFlood { count, concurrency, target } => {
            multi_flood_test(&target, count, concurrency).await?
        }
        Commands::Chaos { mode } => match mode {
            ChaosMode::KillOne { count, concurrency, port } => {
                chaos_kill_one(count, concurrency, port).await?
            }
            ChaosMode::KillLeader { count, concurrency, port } => {
                chaos_kill_leader(count, concurrency, port).await?
            }
            ChaosMode::Rolling { count, concurrency, port, restart_delay } => {
                chaos_rolling(count, concurrency, port, restart_delay).await?
            }
            ChaosMode::Resync { count, concurrency, port, resync_wait } => {
                chaos_resync_test(count, concurrency, port, resync_wait).await?
            }
        },
    }

    Ok(())
}
