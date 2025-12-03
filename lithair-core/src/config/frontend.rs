//! Frontend configuration for static asset serving

use serde::{Deserialize, Serialize};

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
        Self {
            enabled: false,
            static_dir: None,
            watch: false,
            compress: true,
        }
    }
}

impl FrontendConfig {
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            ..Default::default()
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
}
