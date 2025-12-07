//! Declarative RBAC configuration for Lithair
//!
//! This module provides a declarative way to configure RBAC, including:
//! - Role definitions with permissions
//! - User management
//! - Session configuration
//! - Automatic route generation for /auth/login and /auth/logout

use super::{PermissionChecker, Role};
use std::collections::HashMap;
use std::sync::Arc;

/// User for authentication
/// 
/// This is a simple struct for demo/development purposes.
/// In production, you should implement proper password hashing (bcrypt, argon2).
#[derive(Debug, Clone)]
pub struct RbacUser {
    pub username: String,
    pub password: String, // TODO: Should be hashed in production!
    pub role: String,
    pub active: bool,
}

impl RbacUser {
    pub fn new(username: impl Into<String>, password: impl Into<String>, role: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
            role: role.into(),
            active: true,
        }
    }
    
    pub fn verify_password(&self, password: &str) -> bool {
        self.password == password
    }
}

/// Declarative Server-wide RBAC configuration
pub struct ServerRbacConfig {
    /// Role definitions: role_name -> Vec<permissions>
    pub roles: Vec<(String, Vec<String>)>,
    
    /// Users for authentication
    pub users: Vec<RbacUser>,
    
    /// Session store directory path
    pub session_store_path: Option<String>,
    
    /// Session duration in seconds (default: 8 hours)
    pub session_duration: u64,
}

impl Default for ServerRbacConfig {
    fn default() -> Self {
        Self {
            roles: vec![
                ("Admin".to_string(), vec!["*".to_string()]),
                ("User".to_string(), vec!["Read".to_string()]),
            ],
            users: vec![],
            session_store_path: None,
            session_duration: 28800, // 8 hours
        }
    }
}

impl ServerRbacConfig {
    /// Create a new RBAC configuration
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Add a role with permissions
    pub fn with_role(mut self, role: impl Into<String>, permissions: Vec<String>) -> Self {
        self.roles.push((role.into(), permissions));
        self
    }
    
    /// Add multiple roles from definitions
    pub fn with_roles(mut self, roles: Vec<(String, Vec<String>)>) -> Self {
        self.roles = roles;
        self
    }
    
    /// Add a user
    pub fn with_user(mut self, user: RbacUser) -> Self {
        self.users.push(user);
        self
    }
    
    /// Add multiple users
    pub fn with_users(mut self, users: Vec<RbacUser>) -> Self {
        self.users = users;
        self
    }
    
    /// Set session store path
    pub fn with_session_store(mut self, path: impl Into<String>) -> Self {
        self.session_store_path = Some(path.into());
        self
    }
    
    /// Set session duration in seconds
    pub fn with_session_duration(mut self, duration: u64) -> Self {
        self.session_duration = duration;
        self
    }
    
    /// Create Role objects from definitions
    pub fn create_roles(&self) -> Vec<Role> {
        self.roles
            .iter()
            .map(|(name, permissions)| {
                let mut role = Role::new(name.clone());
                for perm in permissions {
                    role = role.with_permission(perm.clone());
                }
                role
            })
            .collect()
    }
    
    /// Create a permission checker from role definitions
    pub fn create_permission_checker(&self) -> Arc<dyn PermissionChecker> {
        Arc::new(DeclarativePermissionChecker::new(self.create_roles()))
    }
}

/// Declarative permission checker that uses Role definitions
pub struct DeclarativePermissionChecker {
    roles: HashMap<String, Role>,
}

impl DeclarativePermissionChecker {
    pub fn new(roles: Vec<Role>) -> Self {
        let role_map = roles
            .into_iter()
            .map(|role| (role.name.clone(), role))
            .collect();
        
        Self { roles: role_map }
    }
}

impl PermissionChecker for DeclarativePermissionChecker {
    fn has_permission(&self, role: &str, permission: &str) -> bool {
        let role_obj = match self.roles.get(role) {
            Some(r) => r,
            None => return false,
        };
        
        // Check for wildcard permission (admin has everything)
        if role_obj.has_permission("*") {
            return true;
        }
        
        // Check for specific permission
        role_obj.has_permission(permission)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_rbac_config() {
        let config = ServerRbacConfig::new()
            .with_role("Admin", vec!["*".to_string()])
            .with_role("Editor", vec!["Read".to_string(), "Write".to_string()])
            .with_user(RbacUser::new("admin", "pass", "Admin"));

        // Default has 2 roles (Admin, User), with_role adds 2 more (Admin duplicate, Editor)
        // Total: 4 roles (with_role doesn't deduplicate)
        assert_eq!(config.roles.len(), 4);
        assert_eq!(config.users.len(), 1);
    }
    
    #[test]
    fn test_permission_checker() {
        let config = ServerRbacConfig::new()
            .with_roles(vec![
                ("Admin".to_string(), vec!["*".to_string()]),
                ("Editor".to_string(), vec!["Read".to_string(), "Write".to_string()]),
            ]);
        
        let checker = config.create_permission_checker();
        
        assert!(checker.has_permission("Admin", "Anything"));
        assert!(checker.has_permission("Editor", "Read"));
        assert!(checker.has_permission("Editor", "Write"));
        assert!(!checker.has_permission("Editor", "Delete"));
    }
}
