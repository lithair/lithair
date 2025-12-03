//! Server configuration

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::env;

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Server listening port
    /// Env: RS_PORT
    /// Default: 8080
    pub port: u16,
    
    /// Server listening address
    /// Env: RS_HOST
    /// Default: "127.0.0.1"
    pub host: String,
    
    /// Number of Tokio worker threads
    /// Env: RS_WORKERS
    /// Default: num_cpus
    pub workers: Option<usize>,
    
    /// Enable CORS support
    /// Env: RS_CORS_ENABLED
    /// Default: false
    pub cors_enabled: bool,
    
    /// Allowed CORS origins
    /// Env: RS_CORS_ORIGINS (comma-separated)
    /// Default: ["*"]
    pub cors_origins: Vec<String>,
    
    /// Request timeout in seconds
    /// Env: RS_REQUEST_TIMEOUT
    /// Default: 30
    pub request_timeout: u64,
    
    /// Maximum request body size in bytes
    /// Env: RS_MAX_BODY_SIZE
    /// Default: 10485760 (10MB)
    pub max_body_size: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: 8080,
            host: "127.0.0.1".to_string(),
            workers: None, // Auto-detect
            cors_enabled: false,
            cors_origins: vec!["*".to_string()],
            request_timeout: 30,
            max_body_size: 10 * 1024 * 1024, // 10MB
        }
    }
}

impl ServerConfig {
    /// Merge another config into this one (other takes priority)
    pub fn merge(&mut self, other: Self) {
        self.port = other.port;
        self.host = other.host;
        self.workers = other.workers;
        self.cors_enabled = other.cors_enabled;
        self.cors_origins = other.cors_origins;
        self.request_timeout = other.request_timeout;
        self.max_body_size = other.max_body_size;
    }
    
    /// Apply environment variables
    pub fn apply_env_vars(&mut self) {
        if let Ok(port) = env::var("RS_PORT") {
            if let Ok(p) = port.parse() {
                self.port = p;
            }
        }
        
        if let Ok(host) = env::var("RS_HOST") {
            self.host = host;
        }
        
        if let Ok(workers) = env::var("RS_WORKERS") {
            if let Ok(w) = workers.parse() {
                self.workers = Some(w);
            }
        }
        
        if let Ok(enabled) = env::var("RS_CORS_ENABLED") {
            self.cors_enabled = enabled.parse().unwrap_or(false);
        }
        
        if let Ok(origins) = env::var("RS_CORS_ORIGINS") {
            self.cors_origins = origins.split(',').map(|s| s.trim().to_string()).collect();
        }
        
        if let Ok(timeout) = env::var("RS_REQUEST_TIMEOUT") {
            if let Ok(t) = timeout.parse() {
                self.request_timeout = t;
            }
        }
        
        if let Ok(size) = env::var("RS_MAX_BODY_SIZE") {
            if let Ok(s) = size.parse() {
                self.max_body_size = s;
            }
        }
    }
    
    /// Validate configuration
    pub fn validate(&self) -> Result<()> {
        if self.port == 0 {
            bail!("Invalid port: port must be between 1 and 65535");
        }
        
        if self.host.is_empty() {
            bail!("Invalid host: host cannot be empty");
        }
        
        if let Some(workers) = self.workers {
            if workers == 0 {
                bail!("Invalid workers: must be at least 1");
            }
        }
        
        if self.request_timeout == 0 {
            bail!("Invalid request_timeout: must be greater than 0");
        }
        
        if self.max_body_size == 0 {
            bail!("Invalid max_body_size: must be greater than 0");
        }
        
        Ok(())
    }
}
