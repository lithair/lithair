//! Playground data models
//!
//! Simple models to demonstrate CRUD + replication capabilities.

use chrono::{DateTime, Utc};
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Item status workflow
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ItemStatus {
    #[default]
    Draft,
    Active,
    Archived,
}

/// PlaygroundItem - simple model for demo CRUD + replication
///
/// This model demonstrates:
/// - Primary key with indexing
/// - Field validation
/// - Replication across cluster
/// - History tracking
/// - Lifecycle management
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
pub struct PlaygroundItem {
    /// Unique identifier (auto-generated UUID)
    #[db(primary_key, indexed)]
    #[lifecycle(immutable)]
    #[http(expose)]
    #[persistence(replicate, track_history)]
    #[serde(default = "generate_uuid")]
    pub id: Uuid,

    /// Item name (required, indexed for search)
    #[db(indexed)]
    #[http(expose, validate = "non_empty")]
    #[persistence(replicate, track_history)]
    pub name: String,

    /// Item description (optional)
    #[http(expose)]
    #[persistence(replicate, track_history)]
    #[serde(default)]
    pub description: String,

    /// Current status (Draft -> Active -> Archived)
    #[http(expose)]
    #[persistence(replicate, track_history)]
    #[serde(default)]
    pub status: ItemStatus,

    /// Arbitrary metadata (JSON)
    #[http(expose)]
    #[persistence(replicate)]
    #[serde(default = "default_metadata")]
    pub metadata: serde_json::Value,

    /// Priority (for sorting/filtering demos)
    #[db(indexed)]
    #[http(expose)]
    #[persistence(replicate)]
    #[serde(default)]
    pub priority: i32,

    /// Tags (for filtering demos)
    #[http(expose)]
    #[persistence(replicate)]
    #[serde(default)]
    pub tags: Vec<String>,

    // ========================================================================
    // SCHEMA V2 FIELDS - Added via rolling upgrade
    // ========================================================================
    /// Category (added in schema v2)
    /// This field demonstrates adding a new field via rolling upgrade
    #[cfg(feature = "schema-v2")]
    #[db(indexed)]
    #[http(expose)]
    #[persistence(replicate)]
    #[serde(default)]
    pub category: String,

    // ========================================================================
    // SCHEMA V3 FIELDS - Added via rolling upgrade
    // ========================================================================
    /// Rating (added in schema v3)
    /// This field demonstrates adding a numeric field via rolling upgrade
    #[cfg(feature = "schema-v3")]
    #[http(expose)]
    #[persistence(replicate)]
    #[serde(default)]
    pub rating: f32,

    /// Creation timestamp (immutable)
    #[lifecycle(immutable)]
    #[http(expose)]
    #[persistence(track_history)]
    #[serde(default = "chrono::Utc::now")]
    pub created_at: DateTime<Utc>,

    /// Last update timestamp
    #[http(expose)]
    #[persistence(replicate, track_history)]
    #[serde(default = "chrono::Utc::now")]
    pub updated_at: DateTime<Utc>,
}

fn generate_uuid() -> Uuid {
    Uuid::new_v4()
}

fn default_metadata() -> serde_json::Value {
    serde_json::json!({})
}

impl PlaygroundItem {
    /// Create a new item with just a name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            description: String::new(),
            status: ItemStatus::Draft,
            metadata: serde_json::json!({}),
            priority: 0,
            tags: vec![],
            #[cfg(feature = "schema-v2")]
            category: String::new(),
            #[cfg(feature = "schema-v3")]
            rating: 0.0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    /// Create a new item with full details
    pub fn with_details(
        name: impl Into<String>,
        description: impl Into<String>,
        priority: i32,
        tags: Vec<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            description: description.into(),
            status: ItemStatus::Draft,
            metadata: serde_json::json!({}),
            priority,
            tags,
            #[cfg(feature = "schema-v2")]
            category: String::new(),
            #[cfg(feature = "schema-v3")]
            rating: 0.0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }
}

/// Get the current schema version based on enabled features
pub fn get_schema_version() -> &'static str {
    #[cfg(feature = "schema-v3")]
    return "v3";

    #[cfg(all(feature = "schema-v2", not(feature = "schema-v3")))]
    return "v2";

    #[cfg(not(feature = "schema-v2"))]
    return "v1";
}

// ============================================================================
// ORDER MODEL - Medium complexity with amounts and status workflow
// ============================================================================

/// Order status workflow
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum OrderStatus {
    #[default]
    Pending,
    Confirmed,
    Processing,
    Shipped,
    Delivered,
    Cancelled,
}

/// Order - demonstrates numeric fields, status workflow, relationships
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
pub struct Order {
    #[db(primary_key, indexed)]
    #[lifecycle(immutable)]
    #[http(expose)]
    #[persistence(replicate, track_history)]
    #[serde(default = "generate_uuid")]
    pub id: Uuid,

    /// Reference to customer (simulated foreign key)
    #[db(indexed)]
    #[http(expose)]
    #[persistence(replicate)]
    pub customer_id: String,

    /// Order status
    #[db(indexed)]
    #[http(expose)]
    #[persistence(replicate, track_history)]
    #[serde(default)]
    pub status: OrderStatus,

    /// Total amount in cents
    #[http(expose)]
    #[persistence(replicate, track_history)]
    #[serde(default)]
    pub total_cents: i64,

    /// Number of items
    #[http(expose)]
    #[persistence(replicate)]
    #[serde(default)]
    pub item_count: i32,

    /// Shipping address
    #[http(expose)]
    #[persistence(replicate)]
    #[serde(default)]
    pub shipping_address: String,

    /// Notes
    #[http(expose)]
    #[persistence(replicate)]
    #[serde(default)]
    pub notes: String,

    #[lifecycle(immutable)]
    #[http(expose)]
    #[persistence(track_history)]
    #[serde(default = "chrono::Utc::now")]
    pub created_at: DateTime<Utc>,

    #[http(expose)]
    #[persistence(replicate, track_history)]
    #[serde(default = "chrono::Utc::now")]
    pub updated_at: DateTime<Utc>,
}

// ============================================================================
// AUDIT LOG MODEL - Append-only logs for tracking operations
// ============================================================================

/// Log level
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LogLevel {
    #[default]
    Info,
    Warning,
    Error,
    Debug,
}

/// AuditLog - append-only log entries (typically not updated/deleted)
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
pub struct AuditLog {
    #[db(primary_key, indexed)]
    #[lifecycle(immutable)]
    #[http(expose)]
    #[persistence(replicate)]
    #[serde(default = "generate_uuid")]
    pub id: Uuid,

    /// Log level
    #[db(indexed)]
    #[http(expose)]
    #[persistence(replicate)]
    #[serde(default)]
    pub level: LogLevel,

    /// Action performed
    #[db(indexed)]
    #[http(expose)]
    #[persistence(replicate)]
    pub action: String,

    /// Entity type (e.g., "Item", "Order")
    #[db(indexed)]
    #[http(expose)]
    #[persistence(replicate)]
    #[serde(default)]
    pub entity_type: String,

    /// Entity ID
    #[db(indexed)]
    #[http(expose)]
    #[persistence(replicate)]
    #[serde(default)]
    pub entity_id: String,

    /// Details (JSON payload)
    #[http(expose)]
    #[persistence(replicate)]
    #[serde(default = "default_metadata")]
    pub details: serde_json::Value,

    /// Source node that created this log
    #[http(expose)]
    #[persistence(replicate)]
    #[serde(default)]
    pub source_node: u64,

    #[lifecycle(immutable)]
    #[http(expose)]
    #[persistence(track_history)]
    #[serde(default = "chrono::Utc::now")]
    pub timestamp: DateTime<Utc>,
}

// ============================================================================
// BENCHMARK TYPES
// ============================================================================

/// BenchmarkRecord - stores benchmark results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkRecord {
    pub id: Uuid,
    pub benchmark_type: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub duration_secs: f64,
    pub total_ops: u64,
    pub successful_ops: u64,
    pub failed_ops: u64,
    pub ops_per_sec: f64,
    pub avg_latency_ms: f64,
    pub p50_latency_ms: f64,
    pub p95_latency_ms: f64,
    pub p99_latency_ms: f64,
    pub max_latency_ms: f64,
}

impl BenchmarkRecord {
    pub fn new(benchmark_type: &str) -> Self {
        Self {
            id: Uuid::new_v4(),
            benchmark_type: benchmark_type.to_string(),
            started_at: Utc::now(),
            completed_at: None,
            duration_secs: 0.0,
            total_ops: 0,
            successful_ops: 0,
            failed_ops: 0,
            ops_per_sec: 0.0,
            avg_latency_ms: 0.0,
            p50_latency_ms: 0.0,
            p95_latency_ms: 0.0,
            p99_latency_ms: 0.0,
            max_latency_ms: 0.0,
        }
    }
}
