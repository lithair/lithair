//! Benchmark engine for Lithair Playground
//!
//! Provides integrated benchmarking with:
//! - Multi-table support (items, orders, logs)
//! - Full CRUD operations (create, read, update, delete)
//! - Per-operation and per-table statistics
//! - Real-time progress reporting

#![allow(dead_code)]

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::sse_events::SseEventBroadcaster;

/// Operation types for benchmarking
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OpType {
    Create,
    Read,
    Update,
    Delete,
}

/// Table types for multi-table benchmarking
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TableType {
    Items,
    Orders,
    Logs,
}

impl TableType {
    fn endpoint(&self) -> &'static str {
        match self {
            TableType::Items => "/api/items",
            TableType::Orders => "/api/orders",
            TableType::Logs => "/api/logs",
        }
    }

    fn all() -> &'static [TableType] {
        &[TableType::Items, TableType::Orders, TableType::Logs]
    }
}

/// Benchmark configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkConfig {
    /// Type of benchmark: "write", "read", "mixed", "crud"
    #[serde(default = "default_benchmark_type")]
    pub benchmark_type: String,

    /// Number of concurrent workers
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,

    /// Duration in seconds
    #[serde(default = "default_duration")]
    pub duration_secs: u64,

    /// Payload size in bytes for write operations
    #[serde(default = "default_payload_size")]
    pub payload_size: usize,

    /// Target operations per second (0 = unlimited)
    #[serde(default)]
    pub target_ops_per_sec: u64,

    /// Enable multi-table operations
    #[serde(default = "default_multi_table")]
    pub multi_table: bool,

    /// CRUD operation ratios (create:read:update:delete)
    /// e.g., [50, 30, 15, 5] = 50% create, 30% read, 15% update, 5% delete
    #[serde(default = "default_crud_ratios")]
    pub crud_ratios: [u8; 4],
}

fn default_benchmark_type() -> String {
    "crud".to_string()
}
fn default_concurrency() -> usize {
    10
}
fn default_duration() -> u64 {
    10
}
fn default_payload_size() -> usize {
    256
}
fn default_multi_table() -> bool {
    true
}
fn default_crud_ratios() -> [u8; 4] {
    [40, 30, 20, 10]
} // 40% create, 30% read, 20% update, 10% delete

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            benchmark_type: default_benchmark_type(),
            concurrency: default_concurrency(),
            duration_secs: default_duration(),
            payload_size: default_payload_size(),
            target_ops_per_sec: 0,
            multi_table: default_multi_table(),
            crud_ratios: default_crud_ratios(),
        }
    }
}

/// Per-operation statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpStats {
    pub total: u64,
    pub successful: u64,
    pub failed: u64,
    pub avg_latency_ms: f64,
}

/// Per-table statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TableStats {
    pub creates: OpStats,
    pub reads: OpStats,
    pub updates: OpStats,
    pub deletes: OpStats,
    pub total_ops: u64,
}

/// Benchmark status/results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkStatus {
    pub id: Uuid,
    pub config: BenchmarkConfig,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub running: bool,
    pub progress_percent: f64,
    pub elapsed_secs: f64,

    // Global stats
    pub total_ops: u64,
    pub successful_ops: u64,
    pub failed_ops: u64,
    pub ops_per_sec: f64,
    pub avg_latency_ms: f64,
    pub p50_latency_ms: f64,
    pub p95_latency_ms: f64,
    pub p99_latency_ms: f64,
    pub max_latency_ms: f64,

    // Per-operation breakdown
    pub creates: OpStats,
    pub reads: OpStats,
    pub updates: OpStats,
    pub deletes: OpStats,

    // Per-table breakdown
    pub items_stats: TableStats,
    pub orders_stats: TableStats,
    pub logs_stats: TableStats,
}

/// Atomic counters for thread-safe stats
struct AtomicOpStats {
    total: AtomicU64,
    successful: AtomicU64,
    failed: AtomicU64,
    latencies: RwLock<Vec<f64>>,
}

impl AtomicOpStats {
    fn new() -> Self {
        Self {
            total: AtomicU64::new(0),
            successful: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            latencies: RwLock::new(Vec::new()),
        }
    }

    async fn record(&self, success: bool, latency_ms: f64) {
        self.total.fetch_add(1, Ordering::SeqCst);
        if success {
            self.successful.fetch_add(1, Ordering::SeqCst);
        } else {
            self.failed.fetch_add(1, Ordering::SeqCst);
        }
        let mut latencies = self.latencies.write().await;
        if latencies.len() < 5000 {
            latencies.push(latency_ms);
        }
    }

    async fn to_stats(&self) -> OpStats {
        let latencies = self.latencies.read().await;
        let avg = if latencies.is_empty() {
            0.0
        } else {
            latencies.iter().sum::<f64>() / latencies.len() as f64
        };
        OpStats {
            total: self.total.load(Ordering::SeqCst),
            successful: self.successful.load(Ordering::SeqCst),
            failed: self.failed.load(Ordering::SeqCst),
            avg_latency_ms: avg,
        }
    }
}

/// Per-table atomic stats
struct AtomicTableStats {
    creates: AtomicOpStats,
    reads: AtomicOpStats,
    updates: AtomicOpStats,
    deletes: AtomicOpStats,
}

impl AtomicTableStats {
    fn new() -> Self {
        Self {
            creates: AtomicOpStats::new(),
            reads: AtomicOpStats::new(),
            updates: AtomicOpStats::new(),
            deletes: AtomicOpStats::new(),
        }
    }

    fn get_op_stats(&self, op: OpType) -> &AtomicOpStats {
        match op {
            OpType::Create => &self.creates,
            OpType::Read => &self.reads,
            OpType::Update => &self.updates,
            OpType::Delete => &self.deletes,
        }
    }

    async fn to_stats(&self) -> TableStats {
        let creates = self.creates.to_stats().await;
        let reads = self.reads.to_stats().await;
        let updates = self.updates.to_stats().await;
        let deletes = self.deletes.to_stats().await;
        let total_ops = creates.total + reads.total + updates.total + deletes.total;
        TableStats { creates, reads, updates, deletes, total_ops }
    }
}

/// Benchmark engine
#[derive(Clone)]
pub struct BenchmarkEngine {
    id: Uuid,
    config: BenchmarkConfig,
    port: u16,
    event_broadcaster: Arc<SseEventBroadcaster>,
    stop_flag: Arc<AtomicBool>,
    started_at: Instant,

    // Global latencies for percentile calculation
    latencies: Arc<RwLock<Vec<f64>>>,

    // Per-operation stats
    create_stats: Arc<AtomicOpStats>,
    read_stats: Arc<AtomicOpStats>,
    update_stats: Arc<AtomicOpStats>,
    delete_stats: Arc<AtomicOpStats>,

    // Per-table stats
    items_stats: Arc<AtomicTableStats>,
    orders_stats: Arc<AtomicTableStats>,
    logs_stats: Arc<AtomicTableStats>,

    // Known IDs for update/delete operations
    known_ids: Arc<RwLock<Vec<(TableType, Uuid)>>>,
}

impl BenchmarkEngine {
    pub fn new(
        config: BenchmarkConfig,
        port: u16,
        event_broadcaster: Arc<SseEventBroadcaster>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            config,
            port,
            event_broadcaster,
            stop_flag: Arc::new(AtomicBool::new(false)),
            started_at: Instant::now(),
            latencies: Arc::new(RwLock::new(Vec::new())),
            create_stats: Arc::new(AtomicOpStats::new()),
            read_stats: Arc::new(AtomicOpStats::new()),
            update_stats: Arc::new(AtomicOpStats::new()),
            delete_stats: Arc::new(AtomicOpStats::new()),
            items_stats: Arc::new(AtomicTableStats::new()),
            orders_stats: Arc::new(AtomicTableStats::new()),
            logs_stats: Arc::new(AtomicTableStats::new()),
            known_ids: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Run the benchmark
    pub async fn run(&self) -> BenchmarkStatus {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        let duration = Duration::from_secs(self.config.duration_secs);
        let started_at = Utc::now();

        // Spawn workers
        let mut handles = Vec::new();
        for worker_id in 0..self.config.concurrency {
            let engine = self.clone();
            let client = client.clone();
            let handle = tokio::spawn(async move {
                engine.worker_loop(worker_id, client).await;
            });
            handles.push(handle);
        }

        // Progress reporter
        let reporter_engine = self.clone();
        let reporter_handle = tokio::spawn(async move {
            reporter_engine.progress_reporter().await;
        });

        // Wait for duration or stop signal
        let stop_flag = self.stop_flag.clone();
        tokio::select! {
            _ = tokio::time::sleep(duration) => {
                stop_flag.store(true, Ordering::SeqCst);
            }
            _ = async {
                while !stop_flag.load(Ordering::SeqCst) {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            } => {}
        }

        // Wait for workers to finish
        for handle in handles {
            let _ = handle.await;
        }
        reporter_handle.abort();

        // Calculate final stats
        self.calculate_status(started_at.to_rfc3339(), Some(Utc::now().to_rfc3339()))
            .await
    }

    /// Worker loop - performs benchmark operations
    async fn worker_loop(&self, _worker_id: usize, client: reqwest::Client) {
        let base_url = format!("http://127.0.0.1:{}", self.port);

        while !self.stop_flag.load(Ordering::SeqCst) {
            // Select operation type based on config
            let op_type = self.select_operation();
            let table = self.select_table();

            let start = Instant::now();
            let success = match op_type {
                OpType::Create => self.do_create(&client, &base_url, table).await,
                OpType::Read => self.do_read(&client, &base_url, table).await,
                OpType::Update => self.do_update(&client, &base_url, table).await,
                OpType::Delete => self.do_delete(&client, &base_url, table).await,
            };

            let latency_ms = start.elapsed().as_secs_f64() * 1000.0;

            // Record global latency
            {
                let mut latencies = self.latencies.write().await;
                if latencies.len() < 10000 {
                    latencies.push(latency_ms);
                }
            }

            // Record per-operation stats
            match op_type {
                OpType::Create => self.create_stats.record(success, latency_ms).await,
                OpType::Read => self.read_stats.record(success, latency_ms).await,
                OpType::Update => self.update_stats.record(success, latency_ms).await,
                OpType::Delete => self.delete_stats.record(success, latency_ms).await,
            }

            // Record per-table stats
            let table_stats = match table {
                TableType::Items => &self.items_stats,
                TableType::Orders => &self.orders_stats,
                TableType::Logs => &self.logs_stats,
            };
            table_stats.get_op_stats(op_type).record(success, latency_ms).await;

            // Rate limiting if configured
            if self.config.target_ops_per_sec > 0 {
                let target_interval = Duration::from_secs_f64(
                    1.0 / (self.config.target_ops_per_sec as f64 / self.config.concurrency as f64),
                );
                let elapsed = start.elapsed();
                if elapsed < target_interval {
                    tokio::time::sleep(target_interval - elapsed).await;
                }
            }
        }
    }

    /// Select operation type based on CRUD ratios
    fn select_operation(&self) -> OpType {
        let ratios = &self.config.crud_ratios;
        let total: u32 = ratios.iter().map(|&r| r as u32).sum();
        let rand = fastrand::u32(0..total);

        let mut cumulative = 0u32;
        for (i, &ratio) in ratios.iter().enumerate() {
            cumulative += ratio as u32;
            if rand < cumulative {
                return match i {
                    0 => OpType::Create,
                    1 => OpType::Read,
                    2 => OpType::Update,
                    _ => OpType::Delete,
                };
            }
        }
        OpType::Create
    }

    /// Select table for multi-table mode
    fn select_table(&self) -> TableType {
        if !self.config.multi_table {
            return TableType::Items;
        }
        match fastrand::u8(0..3) {
            0 => TableType::Items,
            1 => TableType::Orders,
            _ => TableType::Logs,
        }
    }

    /// Perform a create operation
    async fn do_create(&self, client: &reqwest::Client, base_url: &str, table: TableType) -> bool {
        let payload = match table {
            TableType::Items => serde_json::json!({
                "name": format!("benchmark-item-{}", Uuid::new_v4()),
                "description": generate_payload(self.config.payload_size),
                "priority": rand_priority(),
                "tags": vec!["benchmark", "test"],
                "status": "Draft"
            }),
            TableType::Orders => serde_json::json!({
                "customer_id": format!("customer-{}", fastrand::u32(1..1000)),
                "status": "Pending",
                "total_cents": fastrand::i64(100..100000),
                "item_count": fastrand::i32(1..20),
                "shipping_address": generate_payload(64),
                "notes": generate_payload(32)
            }),
            TableType::Logs => {
                let levels = ["Info", "Warning", "Error", "Debug"];
                let level = levels[fastrand::usize(0..4)];
                serde_json::json!({
                    "level": level,
                    "action": format!("benchmark-action-{}", fastrand::u32(1..100)),
                    "entity_type": "BenchmarkEntity",
                    "entity_id": Uuid::new_v4().to_string(),
                    "details": {"benchmark": true, "worker": fastrand::u32(0..100)},
                    "source_node": self.port as u64
                })
            }
        };

        let url = format!("{}{}", base_url, table.endpoint());
        match client.post(&url).json(&payload).send().await {
            Ok(resp) if resp.status().is_success() => {
                // Try to extract ID and store it
                if let Ok(data) = resp.json::<serde_json::Value>().await {
                    if let Some(id) = data.get("id").and_then(|v| v.as_str()) {
                        if let Ok(uuid) = Uuid::parse_str(id) {
                            let mut known_ids = self.known_ids.write().await;
                            if known_ids.len() < 1000 {
                                known_ids.push((table, uuid));
                            }
                        }
                    }
                }
                true
            }
            _ => false,
        }
    }

    /// Perform a read operation
    async fn do_read(&self, client: &reqwest::Client, base_url: &str, table: TableType) -> bool {
        let url = format!("{}{}", base_url, table.endpoint());
        match client.get(&url).send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    /// Perform an update operation
    async fn do_update(&self, client: &reqwest::Client, base_url: &str, table: TableType) -> bool {
        // Get a known ID for this table
        let id = {
            let known_ids = self.known_ids.read().await;
            known_ids.iter().filter(|(t, _)| *t == table).map(|(_, id)| *id).next()
        };

        let Some(id) = id else {
            // No known IDs, do a create instead
            return self.do_create(client, base_url, table).await;
        };

        let payload = match table {
            TableType::Items => {
                let statuses = ["Draft", "Active", "Archived"];
                let status = statuses[fastrand::usize(0..3)];
                serde_json::json!({
                    "description": generate_payload(self.config.payload_size),
                    "priority": rand_priority(),
                    "status": status
                })
            }
            TableType::Orders => {
                let statuses = ["Pending", "Confirmed", "Processing", "Shipped"];
                let status = statuses[fastrand::usize(0..4)];
                serde_json::json!({
                    "status": status,
                    "notes": generate_payload(32)
                })
            }
            TableType::Logs => {
                // Logs are typically append-only, do a create instead
                return self.do_create(client, base_url, table).await;
            }
        };

        let url = format!("{}{}/{}", base_url, table.endpoint(), id);
        match client.put(&url).json(&payload).send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    /// Perform a delete operation
    async fn do_delete(&self, client: &reqwest::Client, base_url: &str, table: TableType) -> bool {
        // Get and remove a known ID for this table
        let id = {
            let mut known_ids = self.known_ids.write().await;
            known_ids
                .iter()
                .position(|(t, _)| *t == table)
                .map(|pos| known_ids.remove(pos).1)
        };

        let Some(id) = id else {
            // No known IDs, do a read instead
            return self.do_read(client, base_url, table).await;
        };

        let url = format!("{}{}/{}", base_url, table.endpoint(), id);
        match client.delete(&url).send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    /// Report progress periodically
    async fn progress_reporter(&self) {
        let mut interval = tokio::time::interval(Duration::from_millis(500));
        let started_at = Utc::now().to_rfc3339();

        loop {
            interval.tick().await;

            if self.stop_flag.load(Ordering::SeqCst) {
                break;
            }

            let status = self.calculate_status(started_at.clone(), None).await;

            // Broadcast progress via SSE
            self.event_broadcaster
                .broadcast(
                    "benchmark",
                    serde_json::json!({
                        "type": "progress",
                        "data": status
                    }),
                )
                .await;
        }
    }

    /// Calculate current status
    async fn calculate_status(
        &self,
        started_at: String,
        completed_at: Option<String>,
    ) -> BenchmarkStatus {
        let elapsed = self.started_at.elapsed().as_secs_f64();

        // Get per-operation stats
        let creates = self.create_stats.to_stats().await;
        let reads = self.read_stats.to_stats().await;
        let updates = self.update_stats.to_stats().await;
        let deletes = self.delete_stats.to_stats().await;

        // Get per-table stats
        let items_stats = self.items_stats.to_stats().await;
        let orders_stats = self.orders_stats.to_stats().await;
        let logs_stats = self.logs_stats.to_stats().await;

        // Calculate totals
        let total = creates.total + reads.total + updates.total + deletes.total;
        let successful =
            creates.successful + reads.successful + updates.successful + deletes.successful;
        let failed = creates.failed + reads.failed + updates.failed + deletes.failed;

        let ops_per_sec = if elapsed > 0.0 { total as f64 / elapsed } else { 0.0 };

        let progress = if self.config.duration_secs > 0 {
            (elapsed / self.config.duration_secs as f64 * 100.0).min(100.0)
        } else {
            0.0
        };

        // Calculate latency percentiles
        let (avg, p50, p95, p99, max) = {
            let latencies = self.latencies.read().await;
            if latencies.is_empty() {
                (0.0, 0.0, 0.0, 0.0, 0.0)
            } else {
                let mut sorted = latencies.clone();
                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());

                let avg = sorted.iter().sum::<f64>() / sorted.len() as f64;
                let p50 = percentile(&sorted, 50.0);
                let p95 = percentile(&sorted, 95.0);
                let p99 = percentile(&sorted, 99.0);
                let max = *sorted.last().unwrap_or(&0.0);

                (avg, p50, p95, p99, max)
            }
        };

        BenchmarkStatus {
            id: self.id,
            config: self.config.clone(),
            started_at,
            completed_at,
            running: !self.stop_flag.load(Ordering::SeqCst),
            progress_percent: progress,
            elapsed_secs: elapsed,
            total_ops: total,
            successful_ops: successful,
            failed_ops: failed,
            ops_per_sec,
            avg_latency_ms: avg,
            p50_latency_ms: p50,
            p95_latency_ms: p95,
            p99_latency_ms: p99,
            max_latency_ms: max,
            creates,
            reads,
            updates,
            deletes,
            items_stats,
            orders_stats,
            logs_stats,
        }
    }

    /// Get current progress
    pub async fn get_progress(&self) -> BenchmarkStatus {
        self.calculate_status(Utc::now().to_rfc3339(), None).await
    }

    /// Stop the benchmark
    pub async fn stop(&self) {
        self.stop_flag.store(true, Ordering::SeqCst);
    }
}

/// Calculate percentile from sorted array
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

/// Generate random payload of given size
fn generate_payload(size: usize) -> String {
    use std::iter::repeat_with;
    repeat_with(fastrand::alphanumeric).take(size).collect()
}

/// Random priority (0-10)
fn rand_priority() -> i32 {
    fastrand::i32(0..=10)
}
