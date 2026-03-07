//! Frontend configuration for static asset serving

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::env;

/// Frontend configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontendConfig {
    /// Whether frontend serving is enabled
    pub enabled: bool,

    /// Directory containing static assets to serve
    pub static_dir: Option<String>,

    /// Whether to watch static directory for changes (hot-reload)
    pub watch: bool,

    /// Whether to compress assets (gzip)
    pub compress: bool,
}

impl Default for FrontendConfig {
    fn default() -> Self {
        Self { enabled: false, static_dir: None, watch: false, compress: true }
    }
}

impl FrontendConfig {
    pub fn enabled() -> Self {
        Self { enabled: true, ..Default::default() }
    }

    pub fn merge(&mut self, other: Self) {
        *self = other;
    }

    pub fn apply_env_vars(&mut self) {
        if let Ok(enabled) = env::var("LT_FRONTEND_ENABLED") {
            self.enabled = enabled.parse().unwrap_or(false);
        }

        if let Ok(static_dir) = env::var("LT_FRONTEND_STATIC_DIR") {
            self.static_dir = Some(static_dir);
            self.enabled = true;
        }

        if let Ok(watch) = env::var("LT_FRONTEND_WATCH") {
            self.watch = watch.parse().unwrap_or(false);
        }

        if let Ok(compress) = env::var("LT_FRONTEND_COMPRESS") {
            self.compress = compress.parse().unwrap_or(true);
        }
    }

    pub fn with_static_dir(mut self, dir: impl Into<String>) -> Self {
        self.static_dir = Some(dir.into());
        self.enabled = true;
        self
    }

    pub fn with_hot_reload(mut self) -> Self {
        self.watch = true;
        self
    }

    pub fn validate(&self) -> Result<()> {
        if self.enabled {
            match self.static_dir.as_deref() {
                Some(dir) if !dir.trim().is_empty() => {}
                _ => bail!("Frontend is enabled but no static_dir is configured"),
            }
        }

        Ok(())
    }
}
