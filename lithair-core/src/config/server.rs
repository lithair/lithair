//! Server configuration

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::env;

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Server listening port
    /// Env: LT_PORT
    /// Default: 8080
    pub port: u16,

    /// Server listening address
    /// Env: LT_HOST
    /// Default: "127.0.0.1"
    pub host: String,

    /// Number of Tokio worker threads
    /// Env: LT_WORKERS
    /// Default: num_cpus
    pub workers: Option<usize>,

    /// Enable CORS support
    /// Env: LT_COLT_ENABLED
    /// Default: false
    pub cors_enabled: bool,

    /// Allowed CORS origins
    /// Env: LT_COLT_ORIGINS (comma-separated)
    /// Default: ["*"]
    pub cors_origins: Vec<String>,

    /// Request timeout in seconds
    /// Env: LT_REQUEST_TIMEOUT
    /// Default: 30
    pub request_timeout: u64,

    /// Maximum request body size in bytes
    /// Env: LT_MAX_BODY_SIZE
    /// Default: 10485760 (10MB)
    pub max_body_size: usize,

    /// Path to TLS certificate PEM file
    /// Env: LT_TLS_CERT
    /// Default: None (plain HTTP)
    pub tls_cert_path: Option<String>,

    /// Path to TLS private key PEM file
    /// Env: LT_TLS_KEY
    /// Default: None (plain HTTP)
    pub tls_key_path: Option<String>,
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
            tls_cert_path: None,
            tls_key_path: None,
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
        self.tls_cert_path = other.tls_cert_path;
        self.tls_key_path = other.tls_key_path;
    }

    /// Apply environment variables
    pub fn apply_env_vars(&mut self) {
        if let Ok(port) = env::var("LT_PORT") {
            if let Ok(p) = port.parse() {
                self.port = p;
            }
        }

        if let Ok(host) = env::var("LT_HOST") {
            self.host = host;
        }

        if let Ok(workers) = env::var("LT_WORKERS") {
            if let Ok(w) = workers.parse() {
                self.workers = Some(w);
            }
        }

        if let Ok(enabled) = env::var("LT_COLT_ENABLED") {
            self.cors_enabled = enabled.parse().unwrap_or(false);
        }

        if let Ok(origins) = env::var("LT_COLT_ORIGINS") {
            self.cors_origins = origins.split(',').map(|s| s.trim().to_string()).collect();
        }

        if let Ok(timeout) = env::var("LT_REQUEST_TIMEOUT") {
            if let Ok(t) = timeout.parse() {
                self.request_timeout = t;
            }
        }

        if let Ok(size) = env::var("LT_MAX_BODY_SIZE") {
            if let Ok(s) = size.parse() {
                self.max_body_size = s;
            }
        }

        if let Ok(cert) = env::var("LT_TLS_CERT") {
            self.tls_cert_path = Some(cert);
        }

        if let Ok(key) = env::var("LT_TLS_KEY") {
            self.tls_key_path = Some(key);
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

        // TLS: both cert and key must be set together
        match (&self.tls_cert_path, &self.tls_key_path) {
            (Some(cert), Some(key)) => {
                if !std::path::Path::new(cert).exists() {
                    bail!("TLS certificate file not found: {}", cert);
                }
                if !std::path::Path::new(key).exists() {
                    bail!("TLS key file not found: {}", key);
                }
            }
            (Some(_), None) => {
                bail!("LT_TLS_CERT is set but LT_TLS_KEY is missing; both are required for TLS");
            }
            (None, Some(_)) => {
                bail!("LT_TLS_KEY is set but LT_TLS_CERT is missing; both are required for TLS");
            }
            (None, None) => {} // Plain HTTP, fine
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_has_no_tls() {
        let cfg = ServerConfig::default();
        assert!(cfg.tls_cert_path.is_none());
        assert!(cfg.tls_key_path.is_none());
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_tls_cert_without_key_fails() {
        let cfg =
            ServerConfig { tls_cert_path: Some("cert.pem".to_string()), ..Default::default() };
        let err = cfg.validate().unwrap_err();
        assert!(err.to_string().contains("LT_TLS_KEY is missing"));
    }

    #[test]
    fn test_tls_key_without_cert_fails() {
        let cfg = ServerConfig { tls_key_path: Some("key.pem".to_string()), ..Default::default() };
        let err = cfg.validate().unwrap_err();
        assert!(err.to_string().contains("LT_TLS_CERT is missing"));
    }

    #[test]
    fn test_tls_cert_file_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let cert_path = dir.path().join("cert.pem");
        let key_path = dir.path().join("key.pem");

        let cfg = ServerConfig {
            tls_cert_path: Some(cert_path.to_str().unwrap().to_string()),
            tls_key_path: Some(key_path.to_str().unwrap().to_string()),
            ..Default::default()
        };
        let err = cfg.validate().unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn test_tls_both_files_exist() {
        let dir = tempfile::tempdir().unwrap();
        let cert_path = dir.path().join("cert.pem");
        let key_path = dir.path().join("key.pem");
        std::fs::write(&cert_path, "fake cert").unwrap();
        std::fs::write(&key_path, "fake key").unwrap();

        let cfg = ServerConfig {
            tls_cert_path: Some(cert_path.to_str().unwrap().to_string()),
            tls_key_path: Some(key_path.to_str().unwrap().to_string()),
            ..Default::default()
        };
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_merge_preserves_tls() {
        let mut base = ServerConfig::default();
        let other = ServerConfig {
            tls_cert_path: Some("cert.pem".to_string()),
            tls_key_path: Some("key.pem".to_string()),
            ..Default::default()
        };
        base.merge(other);
        assert_eq!(base.tls_cert_path.as_deref(), Some("cert.pem"));
        assert_eq!(base.tls_key_path.as_deref(), Some("key.pem"));
    }

    #[test]
    fn test_apply_env_vars_tls() {
        let mut cfg = ServerConfig::default();
        std::env::set_var("LT_TLS_CERT", "/tmp/test-cert.pem");
        std::env::set_var("LT_TLS_KEY", "/tmp/test-key.pem");
        cfg.apply_env_vars();
        assert_eq!(cfg.tls_cert_path.as_deref(), Some("/tmp/test-cert.pem"));
        assert_eq!(cfg.tls_key_path.as_deref(), Some("/tmp/test-key.pem"));
        std::env::remove_var("LT_TLS_CERT");
        std::env::remove_var("LT_TLS_KEY");
    }
}
