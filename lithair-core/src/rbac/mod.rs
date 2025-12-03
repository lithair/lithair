//! Lithair RBAC (Role-Based Access Control) Module
//!
//! This module provides a declarative, extensible RBAC system for Lithair applications.
//! It supports multiple authentication providers and field-level permissions.
//!
//! # Features
//! - Declarative permissions via `#[permission]` attributes
//! - Multiple auth providers (password, OAuth, SAML, custom)
//! - Field-level access control
//! - Role-based authorization
//! - Automatic middleware integration with DeclarativeServer
//!
//! # Example
//! ```rust,ignore
//! #[derive(DeclarativeModel)]
//! #[rbac(enabled = true, roles = "Admin,User", default_role = "User")]
//! pub struct Product {
//!     #[permission(read = "Public")]
//!     pub id: Uuid,
//!     
//!     #[permission(read = "Public", write = "Admin")]
//!     pub name: String,
//!     
//!     #[permission(read = "Admin", write = "Admin")]
//!     pub cost: f64,
//! }
//! ```

mod auth_handlers;
mod config;
mod context;
mod middleware;
mod permissions;
mod providers;
mod roles;
mod traits;

// Public exports
pub use auth_handlers::{handle_rbac_login, handle_rbac_logout};
pub use config::{ServerRbacConfig, RbacUser, DeclarativePermissionChecker};
pub use context::{AuthContext, RbacContext};
pub use middleware::RbacMiddleware;
pub use permissions::{FieldPermission, Permission, PermissionLevel};
pub use providers::{PasswordProvider, ProviderConfig};
pub use roles::{Role, RoleDefinition};
pub use traits::{AuthProvider, Authorizable, FieldFilter};

/// Trait for checking if a role has a specific permission
///
/// This trait allows custom permission logic for different applications.
/// Implement this trait to define your application's permission rules.
pub trait PermissionChecker: Send + Sync {
    /// Check if a given role has the specified permission
    ///
    /// # Arguments
    /// * `role` - The role name (e.g., "Customer", "Employee", "Administrator")
    /// * `permission` - The permission name (e.g., "ProductRead", "ProductWrite")
    ///
    /// # Returns
    /// `true` if the role has the permission, `false` otherwise
    fn has_permission(&self, role: &str, permission: &str) -> bool;
}

/// RBAC configuration for a model
#[derive(Debug, Clone)]
pub struct RbacConfig {
    /// Whether RBAC is enabled for this model
    pub enabled: bool,
    
    /// Available roles for this model
    pub roles: Vec<String>,
    
    /// Default role for unauthenticated users
    pub default_role: String,
    
    /// Authentication provider configuration
    pub provider: ProviderConfig,
}

impl Default for RbacConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            roles: vec!["Public".to_string()],
            default_role: "Public".to_string(),
            provider: ProviderConfig::None,
        }
    }
}

impl RbacConfig {
    /// Create a new RBAC configuration
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Enable RBAC
    pub fn enabled(mut self) -> Self {
        self.enabled = true;
        self
    }
    
    /// Set available roles
    pub fn with_roles(mut self, roles: Vec<String>) -> Self {
        self.roles = roles;
        self
    }
    
    /// Set default role
    pub fn with_default_role(mut self, role: String) -> Self {
        self.default_role = role;
        self
    }
    
    /// Set authentication provider
    pub fn with_provider(mut self, provider: ProviderConfig) -> Self {
        self.provider = provider;
        self
    }
}
