//! Admin panel configuration

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::Path;

#[derive(Clone, Serialize, Deserialize)]
pub struct AdminConfig {
    pub enabled: bool,
    pub path: String,
    pub auth_required: bool,
    pub metrics_enabled: bool,
    pub metrics_path: String,
    /// Enable data admin API endpoints (/_admin/data/*)
    /// Provides database browsing, export, and backup functionality
    #[serde(default)]
    pub data_admin_enabled: bool,
    /// Path where the embedded data admin UI is served (requires admin-ui feature)
    /// Example: "/_data"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_admin_ui_path: Option<String>,
    /// Development-only reload token for simplified hot reload (NOT for production!)
    /// Set via LT_DEV_RELOAD_TOKEN environment variable
    #[serde(skip_serializing)]
    pub dev_reload_token: Option<String>,
}

impl std::fmt::Debug for AdminConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdminConfig")
            .field("enabled", &self.enabled)
            .field("path", &self.path)
            .field("auth_required", &self.auth_required)
            .field("metrics_enabled", &self.metrics_enabled)
            .field("metrics_path", &self.metrics_path)
            .field("data_admin_enabled", &self.data_admin_enabled)
            .field("data_admin_ui_path", &self.data_admin_ui_path)
            .field("dev_reload_token", &self.dev_reload_token.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

impl Default for AdminConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            path: "/admin".to_string(),
            auth_required: true,
            metrics_enabled: true,
            metrics_path: "/metrics".to_string(),
            data_admin_enabled: false,
            data_admin_ui_path: None,
            dev_reload_token: None,
        }
    }
}

impl AdminConfig {
    pub fn merge(&mut self, other: Self) {
        *self = other;
    }

    /// Apply environment variables and handle special cases (random generation, persistence)
    pub fn apply_env_vars(&mut self) {
        if let Ok(enabled) = env::var("LT_ADMIN_ENABLED") {
            self.enabled = enabled.parse().unwrap_or(true);
        }

        if let Ok(path) = env::var("LT_ADMIN_PATH") {
            if path == "random" {
                // Try to load existing random path from persistence file
                if let Ok(persisted_path) = Self::load_random_path() {
                    log::info!("Loaded persisted random admin path");
                    self.path = persisted_path;
                } else {
                    // Generate new random path
                    let random_path = Self::generate_random_path();
                    log::info!("Generated random admin path");

                    // Persist it for future restarts
                    if let Err(e) = Self::save_random_path(&random_path) {
                        log::warn!("Failed to persist random admin path: {}", e);
                    }

                    self.path = random_path;
                }
            } else {
                self.path = path;
            }
        }

        // Development reload token (WARNING: Development only!)
        if let Ok(token) = env::var("LT_DEV_RELOAD_TOKEN") {
            if !token.is_empty() {
                self.dev_reload_token = Some(token);
                log::warn!("DEV RELOAD TOKEN ENABLED (DEVELOPMENT ONLY - NOT FOR PRODUCTION!)");
            }
        }
    }

    pub fn validate(&self) -> Result<()> {
        Ok(())
    }

    /// Generate a cryptographically secure random admin path
    /// Format: /<random-prefix>-<6-chars>
    /// Example: /secure-a3f9k2
    fn generate_random_path() -> String {
        use rand::Rng;
        const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
        let mut rng = rand::thread_rng();

        let random_suffix: String = (0..6)
            .map(|_| {
                let idx = rng.gen_range(0..CHARSET.len());
                CHARSET[idx] as char
            })
            .collect();

        format!("/secure-{}", random_suffix)
    }

    /// Load persisted random path from file
    fn load_random_path() -> Result<String> {
        let path = Path::new(".admin-path");
        if path.exists() {
            let content = fs::read_to_string(path)?;
            Ok(content.trim().to_string())
        } else {
            anyhow::bail!("No persisted admin path found")
        }
    }

    /// Save random path to file for persistence across restarts
    fn save_random_path(admin_path: &str) -> Result<()> {
        fs::write(".admin-path", admin_path)?;
        log::info!("Persisted admin path to .admin-path");
        Ok(())
    }
}
