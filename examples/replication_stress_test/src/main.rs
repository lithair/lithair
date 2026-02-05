//! Replication Stress Test Node
//!
//! Minimal Lithair node for testing and debugging replication issues.
//! Uses a single simple KVEntry model to isolate replication behavior.
//!
//! ## Running a 3-node cluster
//!
//! ```bash
//! # Terminal 1 - Leader (node 0)
//! cargo run --bin stress_node -- --node-id 0 --port 8080 --peers 8081,8082
//!
//! # Terminal 2 - Follower 1
//! cargo run --bin stress_node -- --node-id 1 --port 8081 --peers 8080,8082
//!
//! # Terminal 3 - Follower 2
//! cargo run --bin stress_node -- --node-id 2 --port 8082 --peers 8080,8081
//! ```
//!
//! ## Test replication
//!
//! ```bash
//! # Create entry on leader
//! curl -X POST http://localhost:8080/api/kv \
//!   -H "Content-Type: application/json" \
//!   -d '{"key":"test1","value":"hello"}'
//!
//! # Read from followers (should be replicated)
//! curl http://localhost:8081/api/kv
//! curl http://localhost:8082/api/kv
//!
//! # Check cluster health
//! curl http://localhost:8080/_raft/health | jq
//! ```

use anyhow::Result;
use chrono::{DateTime, Utc};
use clap::Parser;
use lithair_core::app::LithairServer;
use lithair_core::cluster::ClusterArgs;
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// MINIMAL MODEL FOR REPLICATION TESTING
// ============================================================================

/// KVEntry - Minimal key-value model for replication stress testing
///
/// This model is intentionally simple to isolate replication behavior
/// from model complexity. Each field serves a specific purpose:
///
/// - `id`: Primary key for uniqueness
/// - `key`: Indexed field for lookups
/// - `value`: Arbitrary payload
/// - `seq`: Sequence number for ordering verification
/// - `source_node`: Which node created this entry (for debugging)
/// - `created_at`: Timestamp for timing analysis
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
pub struct KVEntry {
    #[db(primary_key, indexed)]
    #[lifecycle(immutable)]
    #[http(expose)]
    #[persistence(replicate)]
    #[serde(default = "generate_uuid")]
    pub id: Uuid,

    /// Key for lookups - indexed for fast queries
    #[db(indexed)]
    #[http(expose, validate = "non_empty")]
    #[persistence(replicate)]
    pub key: String,

    /// Value - can be any string payload
    #[http(expose)]
    #[persistence(replicate)]
    pub value: String,

    /// Sequence number - for tracking ordering in stress tests
    #[http(expose)]
    #[persistence(replicate)]
    #[serde(default)]
    pub seq: u64,

    /// Source node ID that created this entry
    #[http(expose)]
    #[persistence(replicate)]
    #[serde(default)]
    pub source_node: u64,

    /// Creation timestamp for timing analysis
    #[lifecycle(immutable)]
    #[http(expose)]
    #[persistence(replicate)]
    #[serde(default = "chrono::Utc::now")]
    pub created_at: DateTime<Utc>,
}

fn generate_uuid() -> Uuid {
    Uuid::new_v4()
}

// ============================================================================
// SECOND MODEL FOR MULTI-REPLICATION TESTING
// ============================================================================

/// Counter - Second model to test multi-model replication
///
/// Tests that multiple models can replicate simultaneously without conflicts.
/// Simulates a different access pattern (updates vs creates).
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
pub struct Counter {
    #[db(primary_key, indexed)]
    #[lifecycle(immutable)]
    #[http(expose)]
    #[persistence(replicate)]
    #[serde(default = "generate_uuid")]
    pub id: Uuid,

    /// Counter name - unique identifier for this counter
    #[db(indexed)]
    #[http(expose, validate = "non_empty")]
    #[persistence(replicate)]
    pub name: String,

    /// Current count value
    #[http(expose)]
    #[persistence(replicate)]
    #[serde(default)]
    pub value: i64,

    /// Number of increments performed
    #[http(expose)]
    #[persistence(replicate)]
    #[serde(default)]
    pub increments: u64,

    /// Last updated timestamp
    #[http(expose)]
    #[persistence(replicate)]
    #[serde(default = "chrono::Utc::now")]
    pub updated_at: DateTime<Utc>,
}

// ============================================================================
// MAIN ENTRY POINT
// ============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging with timestamp for debugging replication timing
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    let args = ClusterArgs::parse();
    let peer_ports = args.peers.clone().unwrap_or_default();
    let peers: Vec<String> = peer_ports.iter().map(|p| format!("127.0.0.1:{}", p)).collect();

    // Data directory - can be overridden via env var
    let base_dir = std::env::var("STRESS_TEST_DATA").unwrap_or_else(|_| "data".to_string());
    let data_dir = format!("{}/stress_node_{}", base_dir, args.node_id);
    std::fs::create_dir_all(&data_dir)?;

    let kv_path = format!("{}/kv_events", data_dir);
    let counter_path = format!("{}/counter_events", data_dir);

    // Print startup banner
    log::info!("═══════════════════════════════════════════════════════════════");
    log::info!("  REPLICATION STRESS TEST NODE");
    log::info!("═══════════════════════════════════════════════════════════════");
    log::info!("  Node ID:    {}", args.node_id);
    log::info!("  Port:       {}", args.port);
    log::info!("  Peers:      {:?}", peers);
    log::info!("  Data dir:   {}", data_dir);
    log::info!("═══════════════════════════════════════════════════════════════");
    log::info!("  API Endpoints:");
    log::info!("    GET    /api/kv          - List all KV entries");
    log::info!("    POST   /api/kv          - Create KV entry (replicated)");
    log::info!("    GET    /api/counter     - List all counters");
    log::info!("    POST   /api/counter     - Create counter (replicated)");
    log::info!("    PUT    /api/counter/:id - Update counter (replicated)");
    log::info!("═══════════════════════════════════════════════════════════════");
    log::info!("  Diagnostics:");
    log::info!("    GET    /_raft/health - Cluster health status");
    log::info!("    GET    /status       - Node status");
    log::info!("═══════════════════════════════════════════════════════════════");

    // Build server with both models
    let mut server = LithairServer::new()
        .with_port(args.port)
        .with_model::<KVEntry>(&kv_path, "/api/kv")
        .with_model::<Counter>(&counter_path, "/api/counter");

    // Enable Raft cluster if peers are provided
    if !peers.is_empty() {
        server = server.with_raft_cluster(args.node_id, peers);
        log::info!("Cluster mode enabled with {} peers", peer_ports.len());
    } else {
        log::info!("Single-node mode (no peers configured)");
    }

    // Start serving
    log::info!("Starting server on http://127.0.0.1:{}", args.port);
    server.build()?.serve().await
}
