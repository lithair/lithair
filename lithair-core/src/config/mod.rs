//! Configuration system for Lithair
//!
//! This module provides a comprehensive configuration system with:
//! - Multiple configuration sources (defaults, files, environment, code)
//! - Clear supersedence hierarchy
//! - Hot-reload support for runtime changes
//! - Type-safe configuration structs
//!
//! # Configuration Hierarchy
//!
//! Configuration values are resolved in the following order (highest priority wins):
//!
//! 1. **Code** (Builder pattern) - Highest priority
//! 2. **Environment Variables** - Override file config
//! 3. **Config File** (config.toml) - Override defaults
//! 4. **Defaults** - Lowest priority
//!
//! # Example
//!
//! ```no_run
//! use lithair_core::config::LithairConfig;
//!
//! // Load with full supersedence
//! let config = LithairConfig::load()?;
//!
//! // Or load from specific file
//! let config = LithairConfig::from_file("config.toml")?;
//!
//! // Or use defaults
//! let config = LithairConfig::default();
//! # Ok::<(), anyhow::Error>(())
//! ```

pub mod server;
pub mod sessions;
pub mod rbac;
pub mod replication;
pub mod admin;
pub mod logging;
pub mod storage;
pub mod performance;
pub mod frontend;

pub use server::ServerConfig;
pub use sessions::SessionsConfig;
pub use rbac::RbacConfig;
pub use replication::ReplicationConfig;
pub use admin::AdminConfig;
pub use logging::LoggingConfig;
pub use storage::StorageConfig;
pub use performance::PerformanceConfig;
pub use frontend::FrontendConfig;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Complete Lithair configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LithairConfig {
    pub server: ServerConfig,
    pub sessions: SessionsConfig,
    pub rbac: RbacConfig,
    pub replication: ReplicationConfig,
    pub admin: AdminConfig,
    pub logging: LoggingConfig,
    pub storage: StorageConfig,
    pub performance: PerformanceConfig,
    pub frontend: FrontendConfig,
}

impl Default for LithairConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            sessions: SessionsConfig::default(),
            rbac: RbacConfig::default(),
            replication: ReplicationConfig::default(),
            admin: AdminConfig::default(),
            logging: LoggingConfig::default(),
            storage: StorageConfig::default(),
            performance: PerformanceConfig::default(),
            frontend: FrontendConfig::default(),
        }
    }
}

impl LithairConfig {
    /// Load configuration with full supersedence chain
    ///
    /// Priority order (highest to lowest):
    /// 1. Environment variables
    /// 2. Config file (config.toml)
    /// 3. Defaults
    pub fn load() -> Result<Self> {
        Self::load_from("config.toml")
    }
    
    /// Load configuration from a specific file
    pub fn load_from(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        
        // Start with defaults
        let mut config = Self::default();
        
        // Load from file if it exists
        if path.exists() {
            let file_config = Self::from_file(path)
                .with_context(|| format!("Failed to load config from {}", path.display()))?;
            config.merge(file_config);
        }
        
        // Apply environment variables (highest priority)
        config.apply_env_vars();
        
        Ok(config)
    }
    
    /// Load configuration from TOML file
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("Failed to read config file: {}", path.as_ref().display()))?;
        
        toml::from_str(&content)
            .with_context(|| format!("Failed to parse TOML config: {}", path.as_ref().display()))
    }
    
    /// Merge another config into this one (other takes priority)
    pub fn merge(&mut self, other: Self) {
        self.server.merge(other.server);
        self.sessions.merge(other.sessions);
        self.rbac.merge(other.rbac);
        self.replication.merge(other.replication);
        self.admin.merge(other.admin);
        self.logging.merge(other.logging);
        self.storage.merge(other.storage);
        self.performance.merge(other.performance);
    }
    
    /// Apply environment variables to configuration
    pub fn apply_env_vars(&mut self) {
        self.server.apply_env_vars();
        self.sessions.apply_env_vars();
        self.rbac.apply_env_vars();
        self.replication.apply_env_vars();
        self.admin.apply_env_vars();
        self.logging.apply_env_vars();
        self.storage.apply_env_vars();
        self.performance.apply_env_vars();
    }
    
    /// Validate configuration
    pub fn validate(&self) -> Result<()> {
        self.server.validate()?;
        self.sessions.validate()?;
        self.rbac.validate()?;
        self.replication.validate()?;
        self.admin.validate()?;
        self.logging.validate()?;
        self.storage.validate()?;
        self.performance.validate()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_default_config() {
        let config = LithairConfig::default();
        assert_eq!(config.server.port, 8080);
        assert_eq!(config.server.host, "127.0.0.1");
        assert!(config.sessions.enabled);
        assert!(!config.rbac.enabled);
        assert!(!config.replication.enabled);
        assert!(config.admin.enabled);
    }
    
    #[test]
    fn test_config_validation() {
        let config = LithairConfig::default();
        assert!(config.validate().is_ok());
    }
}
