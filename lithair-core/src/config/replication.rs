//! Replication configuration

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::env;

// Default value functions for serde
fn default_max_resync_gap() -> u64 { 1000 }
fn default_max_concurrent_resyncs() -> usize { 2 }
fn default_resync_check_interval_ms() -> u64 { 1000 }
fn default_snapshot_send_timeout_secs() -> u64 { 30 }
fn default_resync_cooldown_secs() -> u64 { 10 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationConfig {
    pub enabled: bool,
    pub node_id: String,
    pub cluster_nodes: Vec<String>,
    pub election_timeout: u64,
    pub heartbeat_interval: u64,
    pub snapshot_threshold: usize,
    
    // === Resync Configuration ===
    /// Maximum index gap before forcing snapshot resync (default: 1000)
    /// If a follower is more than this many entries behind, use snapshot instead of log replay
    #[serde(default = "default_max_resync_gap")]
    pub max_resync_gap: u64,
    
    /// Maximum concurrent snapshot resyncs (default: 2)
    /// Limits how many followers can be resynced simultaneously to prevent overload
    #[serde(default = "default_max_concurrent_resyncs")]
    pub max_concurrent_resyncs: usize,
    
    /// Resync check interval in milliseconds (default: 1000ms)
    /// How often to check for desynced followers
    #[serde(default = "default_resync_check_interval_ms")]
    pub resync_check_interval_ms: u64,
    
    /// Snapshot send timeout in seconds (default: 30s)
    /// Maximum time to wait when sending a snapshot to a follower
    #[serde(default = "default_snapshot_send_timeout_secs")]
    pub snapshot_send_timeout_secs: u64,
    
    /// Minimum time between resyncs for the same follower in seconds (default: 10s)
    /// Prevents resync storms if a follower keeps failing
    #[serde(default = "default_resync_cooldown_secs")]
    pub resync_cooldown_secs: u64,
}

impl Default for ReplicationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            node_id: "node-1".to_string(),
            cluster_nodes: vec![],
            election_timeout: 150,
            heartbeat_interval: 50,
            snapshot_threshold: 1000,
            // Resync defaults
            max_resync_gap: default_max_resync_gap(),
            max_concurrent_resyncs: default_max_concurrent_resyncs(),
            resync_check_interval_ms: default_resync_check_interval_ms(),
            snapshot_send_timeout_secs: default_snapshot_send_timeout_secs(),
            resync_cooldown_secs: default_resync_cooldown_secs(),
        }
    }
}

impl ReplicationConfig {
    pub fn merge(&mut self, other: Self) {
        *self = other;
    }
    
    pub fn apply_env_vars(&mut self) {
        if let Ok(enabled) = env::var("RS_REPLICATION_ENABLED") {
            self.enabled = enabled.parse().unwrap_or(false);
        }
        if let Ok(node_id) = env::var("RS_NODE_ID") {
            self.node_id = node_id;
        }
        if let Ok(nodes) = env::var("RS_CLUSTER_NODES") {
            self.cluster_nodes = nodes.split(',').map(|s| s.trim().to_string()).collect();
        }
        // Resync environment variables
        if let Ok(gap) = env::var("RS_MAX_RESYNC_GAP") {
            self.max_resync_gap = gap.parse().unwrap_or(default_max_resync_gap());
        }
        if let Ok(concurrent) = env::var("RS_MAX_CONCURRENT_RESYNCS") {
            self.max_concurrent_resyncs = concurrent.parse().unwrap_or(default_max_concurrent_resyncs());
        }
        if let Ok(interval) = env::var("RS_RESYNC_CHECK_INTERVAL_MS") {
            self.resync_check_interval_ms = interval.parse().unwrap_or(default_resync_check_interval_ms());
        }
        if let Ok(timeout) = env::var("RS_SNAPSHOT_SEND_TIMEOUT_SECS") {
            self.snapshot_send_timeout_secs = timeout.parse().unwrap_or(default_snapshot_send_timeout_secs());
        }
        if let Ok(cooldown) = env::var("RS_RESYNC_COOLDOWN_SECS") {
            self.resync_cooldown_secs = cooldown.parse().unwrap_or(default_resync_cooldown_secs());
        }
    }
    
    pub fn validate(&self) -> Result<()> {
        Ok(())
    }
}
