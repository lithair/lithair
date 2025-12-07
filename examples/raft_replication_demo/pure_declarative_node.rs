//! Pure Lithair Declarative Cluster Node
//!
//! ULTIMATE Lithair experience using LithairServer with Raft clustering!
//! This demo is the reference for all examples of lithair. Every edits in the code should be tested here and not created with another example (to avoid code duplication)
//! Everything have to be tested here to ensure it works in a distributed environment
//! Must be declarative at most ( all function must be in the rafstone macros and core ) but can be inserted here JUST FOR IMPLEMENTATION / BEBIN FEATURE

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
#[tokio::main]
async fn main() -> Result<()> {
    let args = ClusterArgs::parse();
    let peers: Vec<String> = args
        .peers
        .unwrap_or_default()
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

    // 🚀 Multi-model distributed system with full CRUD replication!
    // - 3 models: Product, Customer, Order
    // - Auto-generates HTTP server with all CRUD endpoints
    // - Auto-generates Raft consensus for leader election
    // - Auto-enables write redirection to leader
    // - Auto-configures EventStore persistence per model
    // - Auto-replicates CREATE, UPDATE, DELETE across cluster
    let mut server = LithairServer::new()
        .with_port(args.port)
        .with_model::<Product>(&products_path, "/api/products")
        .with_model::<Customer>(&customers_path, "/api/customers")
        .with_model::<Order>(&orders_path, "/api/orders");

    // Enable Raft cluster if peers are provided
    if !peers.is_empty() {
        server = server.with_raft_cluster(args.node_id, peers);
    }

    server.build()?.serve().await
}
