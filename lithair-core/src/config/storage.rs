//! Storage configuration

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub data_dir: String,
    pub snapshot_interval: usize,
    pub compaction_enabled: bool,
    pub backup_enabled: bool,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            data_dir: "./data".to_string(),
            snapshot_interval: 1000,
            compaction_enabled: true,
            backup_enabled: false,
        }
    }
}

impl StorageConfig {
    pub fn merge(&mut self, other: Self) { *self = other; }
    pub fn apply_env_vars(&mut self) {
        if let Ok(dir) = env::var("RS_DATA_DIR") {
            self.data_dir = dir;
        }
    }
    pub fn validate(&self) -> Result<()> { Ok(()) }
}
