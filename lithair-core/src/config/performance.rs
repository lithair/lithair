//! Performance configuration

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceConfig {
    pub cache_enabled: bool,
    pub cache_size: usize,
    pub cache_ttl: u64,
    pub batch_size: usize,
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self { cache_enabled: true, cache_size: 1000, cache_ttl: 300, batch_size: 100 }
    }
}

impl PerformanceConfig {
    pub fn merge(&mut self, other: Self) {
        *self = other;
    }
    pub fn apply_env_vars(&mut self) {
        if let Ok(enabled) = env::var("LT_CACHE_ENABLED") {
            self.cache_enabled = enabled.parse().unwrap_or(true);
        }
    }
    pub fn validate(&self) -> Result<()> {
        Ok(())
    }
}
