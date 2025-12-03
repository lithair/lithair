//! Pure Lithair Declarative Cluster Node
//!
//! ULTIMATE Lithair experience using DeclarativeCluster!
//! This demo is the reference for all examples of lithair. Every edits in the code should be tested here and not created with another example (to avoid code duplication)
//! Everything have to be tested here to ensure it works in a distributed environment
//! Must be declarative at most ( all function must be in the rafstone macros and core ) but can be inserted here JUST FOR IMPLEMENTATION / BEBIN FEATURE

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use clap::Parser;
use lithair_core::cluster::{ClusterArgs, DeclarativeCluster};
use lithair_macros::DeclarativeModel;

// ============================================================================
// MÊME DeclarativeModel - Totalement déclaratif
// ============================================================================

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

fn generate_uuid() -> Uuid {
    Uuid::new_v4()
}

/// PURE Declarative Main - Single function call starts complete distributed system!
#[tokio::main]
async fn main() -> Result<()> {
    let args = ClusterArgs::parse();
    let peers = args
        .peers
        .unwrap_or_default()
        .iter()
        .map(|p| format!("127.0.0.1:{}", p))
        .collect();

    // 🚀 This ONE line replaces 321 lines of manual code!
    // - Auto-generates HTTP server with all CRUD endpoints
    // - Auto-generates HYPER HTTP replication routing
    // - Auto-configures OpenRaft consensus
    // - Auto-configures EventStore persistence
    // - Auto-enables RBAC permissions
    // - Auto-generates /status and /internal/replicate endpoints
    // - Auto-detects distributed vs single-node mode
    DeclarativeCluster::start::<Product>(args.node_id, args.port, peers).await
}
