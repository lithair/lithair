//! Role management for RBAC

use std::collections::HashSet;

/// Role definition
#[derive(Debug, Clone)]
pub struct Role {
    /// Role name
    pub name: String,

    /// Role description
    pub description: Option<String>,

    /// Permissions granted by this role
    pub permissions: HashSet<String>,
}

impl Role {
    /// Create a new role
    pub fn new(name: String) -> Self {
        Self { name, description: None, permissions: HashSet::new() }
    }

    /// Set description
    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }

    /// Add a permission
    pub fn with_permission(mut self, permission: String) -> Self {
        self.permissions.insert(permission);
        self
    }

    /// Check if role has a permission
    pub fn has_permission(&self, permission: &str) -> bool {
        self.permissions.contains(permission)
    }
}

/// Role definition for declarative configuration
#[derive(Debug, Clone)]
pub struct RoleDefinition {
    /// Available roles
    pub roles: Vec<String>,

    /// Default role for unauthenticated users
    pub default_role: String,
}

impl Default for RoleDefinition {
    fn default() -> Self {
        Self {
            roles: vec!["Public".to_string(), "User".to_string(), "Admin".to_string()],
            default_role: "Public".to_string(),
        }
    }
}

impl RoleDefinition {
    /// Create a new role definition
    pub fn new(roles: Vec<String>, default_role: String) -> Self {
        Self { roles, default_role }
    }

    /// Check if a role exists
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }
}
