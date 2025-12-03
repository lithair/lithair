//! Replication configuration

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationConfig {
    pub enabled: bool,
    pub node_id: String,
    pub cluster_nodes: Vec<String>,
    pub election_timeout: u64,
    pub heartbeat_interval: u64,
    pub snapshot_threshold: usize,
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
    }
    
    pub fn validate(&self) -> Result<()> {
        Ok(())
    }
}
