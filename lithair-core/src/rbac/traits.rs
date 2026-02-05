//! Core traits for RBAC system

use crate::rbac::context::AuthContext;
use anyhow::Result;
use bytes::Bytes;
use http::Request;
use http_body_util::Full;

/// Authentication provider trait
///
/// Implement this trait to add a new authentication provider
pub trait AuthProvider: Send + Sync {
    /// Authenticate a request and return auth context
    fn authenticate(&self, request: &Request<Full<Bytes>>) -> Result<AuthContext>;

    /// Provider name for logging and identification
    fn name(&self) -> &str;

    /// Optional: Fetch user groups from external source
    fn fetch_groups(&self, _user_id: &str) -> Result<Vec<String>> {
        Ok(vec![])
    }

    /// Optional: Validate token/credentials without full authentication
    fn validate(&self, _token: &str) -> Result<bool> {
        Ok(true)
    }
}

/// Field-level permission filtering
pub trait FieldFilter {
    /// Filter fields based on auth context
    fn filter_fields(&self, context: &AuthContext) -> serde_json::Value;
}

/// Authorization trait for checking permissions
pub trait Authorizable {
    /// Check if context has permission for an action
    fn has_permission(&self, context: &AuthContext, action: &str) -> bool;
}
