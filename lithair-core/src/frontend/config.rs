//! Frontend Configuration for Lithair

use serde::{Deserialize, Serialize};

/// Configuration for Lithair Frontend features
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontendConfig {
    /// Enable frontend asset serving
    pub enabled: bool,

    /// Enable admin interface for asset management
    pub admin_enabled: bool,

    /// Default fallback file for directory requests
    pub fallback_file: Option<String>,

    /// Maximum asset size in bytes (default: 10MB)
    pub max_asset_size: u64,

    /// Enable compression for compressible assets
    pub compression_enabled: bool,

    /// Default cache TTL in seconds
    pub default_cache_ttl: u32,

    /// Admin endpoint prefix (default: "/admin/assets")
    pub admin_prefix: String,

    /// Static files directory to load automatically
    pub static_dir: Option<String>,

    /// Watch static directory for changes (hot reload)
    pub watch_static_dir: bool,
}

impl Default for FrontendConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            admin_enabled: false,
            fallback_file: Some("/index.html".to_string()),
            max_asset_size: 10 * 1024 * 1024, // 10MB
            compression_enabled: true,
            default_cache_ttl: 3600, // 1 hour
            admin_prefix: "/admin/assets".to_string(),
            static_dir: None,
            watch_static_dir: false,
        }
    }
}

impl FrontendConfig {
    /// Create a new frontend config with defaults
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable frontend with default settings
    pub fn enabled() -> Self {
        Self { enabled: true, ..Self::default() }
    }

    /// Enable frontend with admin interface
    pub fn with_admin() -> Self {
        Self { enabled: true, admin_enabled: true, ..Self::default() }
    }

    /// Set fallback file
    pub fn with_fallback(mut self, fallback: Option<String>) -> Self {
        self.fallback_file = fallback;
        self
    }

    /// Set maximum asset size
    pub fn with_max_size(mut self, max_size: u64) -> Self {
        self.max_asset_size = max_size;
        self
    }

    /// Set static directory to load files from
    pub fn with_static_dir<P: Into<String>>(mut self, dir: P) -> Self {
        self.static_dir = Some(dir.into());
        self
    }

    /// Enable hot reload (watch static directory for changes)
    pub fn with_hot_reload(mut self) -> Self {
        self.watch_static_dir = true;
        self
    }
}
