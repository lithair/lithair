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

#[cfg(test)]
mod tests {
    use super::DASHBOARD_HTML;

    /// Anti-rot guard for the frontend-management section (#149 PR 3): the
    /// embedded dashboard must wire the Frontends tab and the calls into the
    /// `/_admin/frontend/*` API. A token check, since the page is static HTML
    /// (the API itself is covered by `frontend_admin_api_test` and the role
    /// boundary by `admin_role_scoping_test`).
    #[test]
    fn dashboard_wires_frontend_management() {
        for needle in [
            "switchTab('frontend')",
            "loadFrontendView",
            "/_admin/frontend",
            "reloadAllFrontends",
            // The reload key is passed via data-key/dataset, not interpolated
            // into the inline onclick (avoids the encodeURIComponent quote bug,
            // #152 review).
            "this.dataset.key",
        ] {
            assert!(
                DASHBOARD_HTML.contains(needle),
                "dashboard.html must contain `{needle}` for the frontend section"
            );
        }
    }
}
