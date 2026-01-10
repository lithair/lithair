//! Storage configuration

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::env;

/// Schema migration mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SchemaMigrationMode {
    /// Log warnings but continue (default)
    #[default]
    Warn,
    /// Fail startup if breaking schema changes detected
    Strict,
    /// Automatically save new schema (no actual data migration yet)
    Auto,
    /// Require manual approval for all changes (creates pending, even in standalone)
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub data_dir: String,
    pub snapshot_interval: usize,
    pub compaction_enabled: bool,
    pub backup_enabled: bool,
    /// Enable schema validation at startup
    #[serde(default = "default_schema_validation")]
    pub schema_validation_enabled: bool,
    /// What to do when schema changes are detected
    #[serde(default)]
    pub schema_migration_mode: SchemaMigrationMode,
}

fn default_schema_validation() -> bool {
    true
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            data_dir: "./data".to_string(),
            snapshot_interval: 1000,
            compaction_enabled: true,
            backup_enabled: false,
            schema_validation_enabled: true,
            schema_migration_mode: SchemaMigrationMode::Warn,
        }
    }
}

impl StorageConfig {
    pub fn merge(&mut self, other: Self) { *self = other; }
    pub fn apply_env_vars(&mut self) {
        if let Ok(dir) = env::var("RS_DATA_DIR") {
            self.data_dir = dir;
        }
        if let Ok(val) = env::var("RS_SCHEMA_VALIDATION") {
            self.schema_validation_enabled = val.parse().unwrap_or(true);
        }
        if let Ok(mode) = env::var("RS_SCHEMA_MIGRATION_MODE") {
            self.schema_migration_mode = match mode.to_lowercase().as_str() {
                "strict" => SchemaMigrationMode::Strict,
                "auto" => SchemaMigrationMode::Auto,
                _ => SchemaMigrationMode::Warn,
            };
        }
    }
    pub fn validate(&self) -> Result<()> { Ok(()) }
}
