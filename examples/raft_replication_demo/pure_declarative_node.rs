//! Pure Lithair Declarative Cluster Node
//!
//! ULTIMATE Lithair experience using LithairServer with Raft clustering!
//! This demo is the reference for all examples of lithair. Every edits in the code should be tested here and not created with another example (to avoid code duplication)
//! Everything have to be tested here to ensure it works in a distributed environment
//! Must be declarative at most ( all function must be in the rafstone macros and core ) but can be inserted here JUST FOR IMPLEMENTATION / BEBIN FEATURE
//!
//! ## New in v2: Full Async Replication Pipeline
//!
//! - **WAL Group Commit**: Batched fsync for ~10x throughput
//! - **Parallel Pipeline**: WAL + Replication run concurrently
//! - **Background Replication**: Lagging followers catch up async
//! - **Health Monitoring**: `/_raft/health` endpoint for cluster status
//! - **Snapshot Resync**: `/_raft/snapshot` for desynced nodes
//!
//! ## Running a 3-node cluster
//!
//! ```bash
//! # Terminal 1 - Leader (node 0)
//! cargo run --bin pure_declarative_node -- --node-id 0 --port 8080 --peers 8081,8082
//!
//! # Terminal 2 - Follower 1
//! cargo run --bin pure_declarative_node -- --node-id 1 --port 8081 --peers 8080,8082
//!
//! # Terminal 3 - Follower 2
//! cargo run --bin pure_declarative_node -- --node-id 2 --port 8082 --peers 8080,8081
//! ```
//!
//! ## Test replication
//!
//! ```bash
//! # Create product on leader
//! curl -X POST http://localhost:8080/api/products \
//!   -H "Content-Type: application/json" \
//!   -d '{"name":"Laptop","price":999.99,"category":"Electronics"}'
//!
//! # Read from any follower (should be replicated)
//! curl http://localhost:8081/api/products
//! curl http://localhost:8082/api/products
//!
//! # Check cluster health
//! curl http://localhost:8080/_raft/health | jq
//! ```

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use clap::Parser;
use lithair_core::cluster::ClusterArgs;
use lithair_core::app::LithairServer;
use lithair_macros::DeclarativeModel;

// ============================================================================
// MULTI-MODEL DECLARATIVE DEFINITIONS
// ============================================================================

/// Product model - core e-commerce entity
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
pub struct Product {
    #[db(primary_key, indexed)]
    #[lifecycle(immutable)]
    #[http(expose)]
    #[persistence(replicate, track_history)]
    #[permission(read = "ProductRead")]
    #[serde(default = "generate_uuid")]
    pub id: Uuid,

    #[db(indexed, unique)]
    #[lifecycle(audited, retention = 90)]
    #[http(expose, validate = "non_empty")]
    #[persistence(replicate, track_history)]
    #[permission(read = "ProductRead", write = "ProductWrite")]
    pub name: String,

    #[db(indexed)]
    #[lifecycle(audited, versioned = 5)]
    #[http(expose, validate = "min_value(0.01)")]
    #[persistence(replicate, track_history)]
    #[permission(read = "ProductRead", write = "ProductWrite")]
    pub price: f64,

    #[http(expose)]
    #[persistence(replicate)]
    #[permission(read = "PublicRead")]
    pub category: String,

    #[lifecycle(immutable)]
    #[http(expose)]
    #[persistence(track_history)]
    #[serde(default = "chrono::Utc::now")]
    pub created_at: DateTime<Utc>,
}

/// Customer model - user management
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
pub struct Customer {
    #[db(primary_key, indexed)]
    #[lifecycle(immutable)]
    #[http(expose)]
    #[persistence(replicate, track_history)]
    #[serde(default = "generate_uuid")]
    pub id: Uuid,

    #[db(indexed)]
    #[http(expose, validate = "non_empty")]
    #[persistence(replicate, track_history)]
    pub name: String,

    #[db(indexed, unique)]
    #[http(expose, validate = "non_empty")]
    #[persistence(replicate, track_history)]
    pub email: String,

    #[http(expose)]
    #[persistence(replicate)]
    #[serde(default)]
    pub tier: String,

    #[lifecycle(immutable)]
    #[http(expose)]
    #[persistence(track_history)]
    #[serde(default = "chrono::Utc::now")]
    pub created_at: DateTime<Utc>,
}

/// Order model - transactional data
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
pub struct Order {
    #[db(primary_key, indexed)]
    #[lifecycle(immutable)]
    #[http(expose)]
    #[persistence(replicate, track_history)]
    #[serde(default = "generate_uuid")]
    pub id: Uuid,

    #[db(indexed)]
    #[http(expose)]
    #[persistence(replicate, track_history)]
    pub customer_id: Uuid,

    #[db(indexed)]
    #[http(expose)]
    #[persistence(replicate, track_history)]
    pub product_id: Uuid,

    #[http(expose, validate = "min_value(1)")]
    #[persistence(replicate, track_history)]
    pub quantity: u32,

    #[http(expose)]
    #[persistence(replicate, track_history)]
    pub total_price: f64,

    #[db(indexed)]
    #[http(expose)]
    #[persistence(replicate, track_history)]
    #[serde(default = "default_status")]
    pub status: String,

    #[lifecycle(immutable)]
    #[http(expose)]
    #[persistence(track_history)]
    #[serde(default = "chrono::Utc::now")]
    pub created_at: DateTime<Utc>,
}

fn generate_uuid() -> Uuid {
    Uuid::new_v4()
}

fn default_status() -> String {
    "pending".to_string()
}

/// PURE Declarative Main - Single function call starts complete distributed system!
///
/// ## Architecture (v2 - Full Async Pipeline)
///
/// ```text
/// ┌─────────────────────────────────────────────────────────────────┐
/// │                     LITHAIR CLUSTER NODE                        │
/// ├─────────────────────────────────────────────────────────────────┤
/// │                                                                 │
/// │  HTTP Request (Hyper async)                                     │
/// │       │                                                         │
/// │       ▼                                                         │
/// │  ┌─────────────────────────────────────────┐                   │
/// │  │ Consensus Log (in-memory, atomic)       │                   │
/// │  └────────────────┬────────────────────────┘                   │
/// │                   │                                             │
/// │       ┌───────────┴───────────┐                                │
/// │       │    tokio::join!       │  ◄── PARALLEL                  │
/// │       ▼                       ▼                                 │
/// │  ┌─────────────┐    ┌──────────────────┐                       │
/// │  │ WAL Group   │    │ Replication      │                       │
/// │  │ Commit      │    │ (FuturesUnord)   │                       │
/// │  │ (5ms batch) │    │ HTTP to peers    │                       │
/// │  └─────────────┘    └──────────────────┘                       │
/// │       │                       │                                 │
/// │       └───────────┬───────────┘                                │
/// │                   ▼                                             │
/// │  ┌─────────────────────────────────────────┐                   │
/// │  │ Commit (majority) → Apply to state      │                   │
/// │  └─────────────────────────────────────────┘                   │
/// │                                                                 │
/// │  Background Tasks:                                              │
/// │  • WAL Flush (5ms interval)                                    │
/// │  • Lagging Follower Replication (50ms)                         │
/// │  • Leader Heartbeat (1s)                                       │
/// │                                                                 │
/// └─────────────────────────────────────────────────────────────────┘
/// ```
#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    let args = ClusterArgs::parse();
    let peer_ports = args.peers.clone().unwrap_or_default();
    let peers: Vec<String> = peer_ports
        .iter()
        .map(|p| format!("127.0.0.1:{}", p))
        .collect();

    // Auto-configure data directories (honor EXPERIMENT_DATA_BASE when provided)
    let base_dir = std::env::var("EXPERIMENT_DATA_BASE").unwrap_or_else(|_| "data".to_string());
    let data_dir = format!("{}/pure_node_{}", base_dir, args.node_id);
    std::fs::create_dir_all(&data_dir)?;

    // Event store paths for each model
    let products_path = format!("{}/products_events", data_dir);
    let customers_path = format!("{}/customers_events", data_dir);
    let orders_path = format!("{}/orders_events", data_dir);

    log::info!("═══════════════════════════════════════════════════════════");
    log::info!("  LITHAIR CLUSTER NODE v2 - Full Async Replication");
    log::info!("═══════════════════════════════════════════════════════════");
    log::info!("  Node ID:    {}", args.node_id);
    log::info!("  Port:       {}", args.port);
    log::info!("  Peers:      {:?}", peers);
    log::info!("  Data dir:   {}", data_dir);
    log::info!("═══════════════════════════════════════════════════════════");

    // 🚀 Multi-model distributed system with full CRUD replication!
    //
    // Features enabled automatically:
    // - WAL with Group Commit (5ms batching, ~10x throughput)
    // - Parallel WAL + Replication pipeline
    // - Background replication for lagging followers
    // - Snapshot manager for desynced node resync
    // - Follower health tracking (healthy/lagging/desynced)
    //
    // Endpoints:
    // - GET  /api/products          - List all products
    // - POST /api/products          - Create product (replicated)
    // - GET  /api/products/:id      - Get product by ID
    // - PUT  /api/products/:id      - Update product (replicated)
    // - DELETE /api/products/:id    - Delete product (replicated)
    // - GET  /_raft/health          - Cluster health status
    // - GET  /_raft/snapshot        - Get snapshot for resync
    // - POST /_raft/snapshot        - Install snapshot (follower)
    //
    let mut server = LithairServer::new()
        .with_port(args.port)
        .with_model::<Product>(&products_path, "/api/products")
        .with_model::<Customer>(&customers_path, "/api/customers")
        .with_model::<Order>(&orders_path, "/api/orders");

    // Enable Raft cluster if peers are provided
    if !peers.is_empty() {
        server = server.with_raft_cluster(args.node_id, peers);
        log::info!("🔗 Cluster mode enabled with {} peers", peer_ports.len());
    } else {
        log::info!("📦 Single-node mode (no peers)");
    }

    log::info!("═══════════════════════════════════════════════════════════");
    log::info!("  Starting server...");
    log::info!("═══════════════════════════════════════════════════════════");

    server.build()?.serve().await
}
