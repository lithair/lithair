//! Logging configuration

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub format: String,
    pub file_enabled: bool,
    pub file_path: String,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            format: "json".to_string(),
            file_enabled: false,
            file_path: "./logs".to_string(),
        }
    }
}

impl LoggingConfig {
    pub fn merge(&mut self, other: Self) {
        *self = other;
    }
    pub fn apply_env_vars(&mut self) {
        if let Ok(level) = env::var("RS_LOG_LEVEL") {
            self.level = level;
        }
    }
    pub fn validate(&self) -> Result<()> {
        Ok(())
    }
}
