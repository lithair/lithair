//! Embedded Data Admin UI
//!
//! This module provides an embedded dashboard for browsing and managing data.
//! Only compiled when the `admin-ui` feature is enabled.
//!
//! # Usage
//!
//! ```rust,ignore
//! LithairServer::new()
//!     .with_model::<Article>("./data/articles", "/api/articles")
//!     .with_data_admin()           // Enable API endpoints
//!     .with_data_admin_ui("/_data") // Enable embedded dashboard
//!     .serve()
//!     .await?;
//! ```

/// The embedded dashboard HTML (single-page app with inline CSS/JS)
pub const DASHBOARD_HTML: &str = include_str!("dashboard.html");

/// Configuration for the admin UI
#[derive(Debug, Clone)]
pub struct AdminUiConfig {
    /// Path where the dashboard is served (e.g., "/_data")
    pub path: String,
    /// Whether to require authentication (uses existing RBAC if configured)
    pub require_auth: bool,
}

impl Default for AdminUiConfig {
    fn default() -> Self {
        Self { path: "/_data".to_string(), require_auth: true }
    }
}

impl AdminUiConfig {
    /// Create a new config with custom path
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into(), require_auth: true }
    }

    /// Disable authentication requirement (not recommended for production)
    pub fn no_auth(mut self) -> Self {
        self.require_auth = false;
        self
    }
}
