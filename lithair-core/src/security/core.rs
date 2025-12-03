//! Lithair Security Module - Core RBAC Implementation
//!
//! This module provides Role-Based Access Control (RBAC) functionality that is built into
//! the core of Lithair. It is not optional - every Lithair application has security
//! built-in by default.
//!
//! # Features
//!
//! - **Event-Sourced Security**: All security events are persisted and auditable
//! - **Granular Permissions**: Global, object-level, and ownership-based permissions
//! - **JWT Authentication**: Stateless authentication with configurable expiration
//! - **Audit Trail**: Complete security audit log for compliance
//! - **Multi-Tenant Support**: Isolation between users, teams, and organizations

use std::collections::{HashMap, HashSet};

/// User ID type
pub type UserId = u32;

/// Role ID type  
pub type RoleId = u32;

/// Session ID type
pub type SessionId = String;

/// 🎯 **AGNOSTIC PERMISSION TRAIT**
///
/// This trait allows applications to define their own permission systems
/// without hardcoding business logic into the Lithair framework core.
///
/// # Examples
///
/// ```rust,ignore
/// // E-commerce application permissions
/// #[derive(Debug, Clone, PartialEq, Eq, Hash)]
/// pub enum ECommercePermission {
///     ProductCreateAny,
///     ProductReadAny,
///     ProductUpdateAny,
///     ProductDeleteAny,
///     AdminDashboard,
/// }
///
/// impl Permission for ECommercePermission {
///     fn identifier(&self) -> &str {
///         match self {
///             Self::ProductCreateAny => "product:create:any",
///             Self::ProductReadAny => "product:read:any",
///             Self::ProductUpdateAny => "product:update:any",
///             Self::ProductDeleteAny => "product:delete:any",
///             Self::AdminDashboard => "admin:dashboard",
///         }
///     }
///
///     fn description(&self) -> &str {
///         match self {
///             Self::ProductCreateAny => "Create any product",
///             Self::ProductReadAny => "Read all products",
///             Self::ProductUpdateAny => "Update any product",
///             Self::ProductDeleteAny => "Delete any product",
///             Self::AdminDashboard => "Access admin dashboard",
///         }
///     }
/// }
/// ```
pub trait Permission:
    Clone + PartialEq + Eq + std::hash::Hash + Send + Sync + std::fmt::Debug + 'static
{
    /// Unique identifier for this permission (e.g., "product:create:any")
    fn identifier(&self) -> &str;

    /// Human-readable description of this permission
    fn description(&self) -> &str;

    /// Optional: Permission category for grouping (defaults to "general")
    fn category(&self) -> &str {
        "general"
    }

    /// Optional: Permission level (defaults to "standard")
    fn level(&self) -> PermissionLevel {
        PermissionLevel::Standard
    }
}

/// Permission levels for hierarchical access control
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PermissionLevel {
    /// Basic user permissions
    Standard,
    /// Elevated permissions (managers, moderators)
    Elevated,
    /// Administrative permissions
    Administrative,
    /// System-level permissions (super admin)
    System,
}

/// 🎯 **PERMISSION UTILITIES**
///
/// Utility functions for working with application-defined permissions
#[allow(dead_code)]
pub struct PermissionUtils;

#[allow(dead_code)]
impl PermissionUtils {
    /// Validate that a permission identifier follows naming conventions
    pub fn validate_identifier(identifier: &str) -> bool {
        // Format: "resource:action:scope" (e.g., "product:create:any")
        let parts: Vec<&str> = identifier.split(':').collect();
        parts.len() >= 2
            && parts.len() <= 3
            && parts.iter().all(|part| {
                !part.is_empty() && part.chars().all(|c| c.is_alphanumeric() || c == '_')
            })
    }

    /// Extract resource type from permission identifier
    pub fn extract_resource(identifier: &str) -> Option<&str> {
        identifier.split(':').next()
    }

    /// Extract action from permission identifier
    pub fn extract_action(identifier: &str) -> Option<&str> {
        identifier.split(':').nth(1)
    }

    /// Extract scope from permission identifier
    pub fn extract_scope(identifier: &str) -> Option<&str> {
        identifier.split(':').nth(2)
    }
}

/// Role definition with permissions
#[derive(Debug, Clone)]
pub struct Role<P: Permission> {
    pub id: RoleId,
    pub name: String,
    pub permissions: HashSet<P>,
    pub created_at: u64,
    pub created_by: UserId,
}

/// User definition with authentication info
#[derive(Debug, Clone)]
pub struct User {
    pub id: UserId,
    pub email: String,
    pub name: String,
    pub password_hash: String,
    pub team_id: Option<u32>,
    pub organization_id: Option<u32>,
    pub created_at: u64,
    pub last_login: Option<u64>,
    pub is_active: bool,
}

/// Active user session
#[derive(Debug, Clone)]
pub struct Session {
    pub id: SessionId,
    pub user_id: UserId,
    pub created_at: u64,
    pub expires_at: u64,
    pub last_activity: u64,
    pub ip_address: String,
    pub user_agent: String,
}

/// Authentication context for a request
#[derive(Debug, Clone)]
pub struct AuthContext<P: Permission> {
    pub user_id: UserId,
    pub session_id: SessionId,
    pub permissions: HashSet<P>,
    pub team_id: Option<u32>,
    pub organization_id: Option<u32>,
}

/// Security events for audit trail
#[derive(Debug, Clone)]
pub enum SecurityEvent<P: Permission> {
    // User lifecycle
    UserCreated {
        user_id: UserId,
        email: String,
        name: String,
        created_by: UserId,
        timestamp: u64,
    },

    UserUpdated {
        user_id: UserId,
        email: String,
        name: String,
        updated_by: UserId,
        timestamp: u64,
    },

    UserDeleted {
        user_id: UserId,
        deleted_by: UserId,
        timestamp: u64,
    },

    // Role management
    RoleCreated {
        role_id: RoleId,
        name: String,
        permissions: Vec<P>,
        created_by: UserId,
        timestamp: u64,
    },

    RoleUpdated {
        role_id: RoleId,
        name: String,
        permissions: Vec<P>,
        updated_by: UserId,
        timestamp: u64,
    },

    RoleDeleted {
        role_id: RoleId,
        deleted_by: UserId,
        timestamp: u64,
    },

    // User-role assignments
    UserRoleAssigned {
        user_id: UserId,
        role_id: RoleId,
        assigned_by: UserId,
        timestamp: u64,
    },

    UserRoleRevoked {
        user_id: UserId,
        role_id: RoleId,
        revoked_by: UserId,
        timestamp: u64,
    },

    // Authentication events
    UserAuthenticated {
        user_id: UserId,
        session_id: SessionId,
        ip_address: String,
        user_agent: String,
        timestamp: u64,
    },

    UserLoggedOut {
        user_id: UserId,
        session_id: SessionId,
        timestamp: u64,
    },

    AuthenticationFailed {
        email: String,
        ip_address: String,
        reason: String,
        timestamp: u64,
    },

    // Access control events
    AccessGranted {
        user_id: UserId,
        resource: String,
        action: String,
        object_id: Option<u32>,
        timestamp: u64,
    },

    AccessDenied {
        user_id: UserId,
        resource: String,
        action: String,
        object_id: Option<u32>,
        reason: String,
        timestamp: u64,
    },

    // Object ownership events
    ObjectOwnershipAssigned {
        object_type: String,
        object_id: u32,
        owner_id: UserId,
        assigned_by: UserId,
        timestamp: u64,
    },

    ObjectOwnershipTransferred {
        object_type: String,
        object_id: u32,
        from_owner: UserId,
        to_owner: UserId,
        transferred_by: UserId,
        timestamp: u64,
    },
}

// JSON serialization for SecurityEvent will be implemented later if needed

/// Security state containing all security-related data
///
/// Generic over Permission type to allow applications to define their own permission systems
#[derive(Debug, Clone)]
pub struct SecurityState<P: Permission> {
    pub users: HashMap<UserId, User>,
    pub roles: HashMap<RoleId, Role<P>>,
    pub user_roles: HashMap<UserId, HashSet<RoleId>>,
    pub active_sessions: HashMap<SessionId, Session>,
    pub object_ownership: HashMap<String, HashMap<u32, UserId>>, // object_type -> object_id -> owner_id
    pub team_memberships: HashMap<u32, HashSet<UserId>>,         // team_id -> user_ids
    pub organization_memberships: HashMap<u32, HashSet<UserId>>, // org_id -> user_ids
}

impl<P: Permission> Default for SecurityState<P> {
    fn default() -> Self {
        Self {
            users: HashMap::new(),
            roles: HashMap::new(),
            user_roles: HashMap::new(),
            active_sessions: HashMap::new(),
            object_ownership: HashMap::new(),
            team_memberships: HashMap::new(),
            organization_memberships: HashMap::new(),
        }
    }
}

impl<P: Permission> SecurityState<P> {
    /// Create a new empty security state
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply a security event to update the state
    pub fn apply_security_event(&mut self, event: &SecurityEvent<P>) {
        match event {
            SecurityEvent::UserCreated { user_id, email, name, timestamp, .. } => {
                let user = User {
                    id: *user_id,
                    email: email.clone(),
                    name: name.clone(),
                    password_hash: String::new(), // Will be set separately
                    team_id: None,
                    organization_id: None,
                    created_at: *timestamp,
                    last_login: None,
                    is_active: true,
                };
                self.users.insert(*user_id, user);
            }

            SecurityEvent::RoleCreated { role_id, name, permissions, created_by, timestamp } => {
                let role = Role {
                    id: *role_id,
                    name: name.clone(),
                    permissions: permissions.iter().cloned().collect(),
                    created_at: *timestamp,
                    created_by: *created_by,
                };
                self.roles.insert(*role_id, role);
            }

            SecurityEvent::UserRoleAssigned { user_id, role_id, .. } => {
                self.user_roles.entry(*user_id).or_default().insert(*role_id);
            }

            SecurityEvent::UserRoleRevoked { user_id, role_id, .. } => {
                if let Some(user_roles) = self.user_roles.get_mut(user_id) {
                    user_roles.remove(role_id);
                }
            }

            SecurityEvent::ObjectOwnershipAssigned { object_type, object_id, owner_id, .. } => {
                self.object_ownership
                    .entry(object_type.clone())
                    .or_default()
                    .insert(*object_id, *owner_id);
            }

            SecurityEvent::ObjectOwnershipTransferred {
                object_type, object_id, to_owner, ..
            } => {
                self.object_ownership
                    .entry(object_type.clone())
                    .or_default()
                    .insert(*object_id, *to_owner);
            }

            // TODO: Implement other event applications
            _ => {}
        }
    }

    /// Get all permissions for a user
    pub fn get_user_permissions(&self, user_id: UserId) -> HashSet<P> {
        let mut permissions = HashSet::new();

        if let Some(role_ids) = self.user_roles.get(&user_id) {
            for role_id in role_ids {
                if let Some(role) = self.roles.get(role_id) {
                    permissions.extend(role.permissions.iter().cloned());
                }
            }
        }

        permissions
    }

    /// Check if a user owns an object
    pub fn user_owns_object(&self, user_id: UserId, object_type: &str, object_id: u32) -> bool {
        self.object_ownership
            .get(object_type)
            .and_then(|objects| objects.get(&object_id))
            .map(|owner_id| *owner_id == user_id)
            .unwrap_or(false)
    }

    /// Check if users are in the same team
    pub fn users_same_team(&self, user1: UserId, user2: UserId) -> bool {
        let user1_team = self.users.get(&user1).and_then(|u| u.team_id);
        let user2_team = self.users.get(&user2).and_then(|u| u.team_id);

        match (user1_team, user2_team) {
            (Some(team1), Some(team2)) => team1 == team2,
            _ => false,
        }
    }

    /// Alias for user_owns_object for compatibility
    pub fn check_ownership(&self, user_id: UserId, object_type: &str, object_id: u32) -> bool {
        self.user_owns_object(user_id, object_type, object_id)
    }
}

/// Security errors
#[derive(Debug)]
pub enum SecurityError {
    AuthenticationFailed,
    AccessDenied,
    InvalidToken,
    SessionExpired,
    UserNotFound,
    RoleNotFound,
    PermissionNotFound,
    InvalidCredentials,
}

impl std::fmt::Display for SecurityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecurityError::AuthenticationFailed => write!(f, "Authentication failed"),
            SecurityError::AccessDenied => write!(f, "Access denied"),
            SecurityError::InvalidToken => write!(f, "Invalid token"),
            SecurityError::SessionExpired => write!(f, "Session expired"),
            SecurityError::UserNotFound => write!(f, "User not found"),
            SecurityError::RoleNotFound => write!(f, "Role not found"),
            SecurityError::PermissionNotFound => write!(f, "Permission not found"),
            SecurityError::InvalidCredentials => write!(f, "Invalid credentials"),
        }
    }
}

impl std::error::Error for SecurityError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // Example permission enum for testing
    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    #[allow(dead_code)]
    enum TestPermission {
        ReadData,
        WriteData,
        DeleteData,
        AdminAccess,
    }

    impl Permission for TestPermission {
        fn identifier(&self) -> &str {
            match self {
                Self::ReadData => "data:read",
                Self::WriteData => "data:write",
                Self::DeleteData => "data:delete",
                Self::AdminAccess => "admin:access",
            }
        }

        fn description(&self) -> &str {
            match self {
                Self::ReadData => "Read data",
                Self::WriteData => "Write data",
                Self::DeleteData => "Delete data",
                Self::AdminAccess => "Admin access",
            }
        }

        fn category(&self) -> &str {
            match self {
                Self::ReadData | Self::WriteData | Self::DeleteData => "data",
                Self::AdminAccess => "admin",
            }
        }

        fn level(&self) -> PermissionLevel {
            match self {
                Self::ReadData => PermissionLevel::Standard,
                Self::WriteData => PermissionLevel::Elevated,
                Self::DeleteData => PermissionLevel::Administrative,
                Self::AdminAccess => PermissionLevel::System,
            }
        }
    }

    #[test]
    fn test_permission_trait_implementation() {
        let permission = TestPermission::ReadData;
        assert_eq!(permission.identifier(), "data:read");
        assert_eq!(permission.description(), "Read data");
        assert_eq!(permission.category(), "data");
        assert_eq!(permission.level(), PermissionLevel::Standard);
    }

    #[test]
    fn test_permission_utils() {
        assert!(PermissionUtils::validate_identifier("product:create:any"));
        assert!(PermissionUtils::validate_identifier("user:read"));
        assert!(!PermissionUtils::validate_identifier("invalid"));
        assert!(!PermissionUtils::validate_identifier("too:many:parts:here"));

        assert_eq!(PermissionUtils::extract_resource("product:create:any"), Some("product"));
        assert_eq!(PermissionUtils::extract_action("product:create:any"), Some("create"));
        assert_eq!(PermissionUtils::extract_scope("product:create:any"), Some("any"));
    }

    #[test]
    fn test_security_state_generic() {
        let state: SecurityState<TestPermission> = SecurityState::new();

        // Test empty state
        assert_eq!(state.users.len(), 0);
        assert_eq!(state.roles.len(), 0);

        // Test user permissions (empty for new user)
        let permissions = state.get_user_permissions(1);
        assert_eq!(permissions.len(), 0);
    }

    #[test]
    fn test_role_creation_and_assignment() {
        let mut state: SecurityState<TestPermission> = SecurityState::new();

        // Create a role with permissions
        let mut role_permissions = HashSet::new();
        role_permissions.insert(TestPermission::ReadData);
        role_permissions.insert(TestPermission::WriteData);

        let role = Role {
            id: 1,
            name: "Editor".to_string(),
            permissions: role_permissions.clone(),
            created_at: 1234567890,
            created_by: 0,
        };

        state.roles.insert(1, role);

        // Assign role to user
        state.user_roles.entry(1).or_default().insert(1);

        // Test user permissions
        let user_permissions = state.get_user_permissions(1);
        assert_eq!(user_permissions.len(), 2);
        assert!(user_permissions.contains(&TestPermission::ReadData));
        assert!(user_permissions.contains(&TestPermission::WriteData));
        assert!(!user_permissions.contains(&TestPermission::DeleteData));
    }

    #[test]
    fn test_object_ownership() {
        let mut state: SecurityState<TestPermission> = SecurityState::default();

        let user_id = 1;
        let product_id = 100;

        // User doesn't own the product initially
        assert!(!state.user_owns_object(user_id, "product", product_id));

        // Assign ownership
        state
            .object_ownership
            .entry("product".to_string())
            .or_default()
            .insert(product_id, user_id);

        // Now user owns the product
        assert!(state.user_owns_object(user_id, "product", product_id));

        // Other user doesn't own it
        assert!(!state.user_owns_object(2, "product", product_id));

        // Test check_ownership alias
        assert!(state.check_ownership(user_id, "product", product_id));
    }

    #[test]
    fn test_security_event_application() {
        let mut state: SecurityState<TestPermission> = SecurityState::new();

        // Test user creation event
        let user_event = SecurityEvent::UserCreated {
            user_id: 1,
            email: "test@example.com".to_string(),
            name: "Test User".to_string(),
            created_by: 0,
            timestamp: 1234567890,
        };

        state.apply_security_event(&user_event);
        assert_eq!(state.users.len(), 1);
        assert_eq!(state.users.get(&1).unwrap().email, "test@example.com");

        // Test role creation event
        let role_event = SecurityEvent::RoleCreated {
            role_id: 1,
            name: "Test Role".to_string(),
            permissions: vec![TestPermission::ReadData, TestPermission::WriteData],
            created_by: 0,
            timestamp: 1234567890,
        };

        state.apply_security_event(&role_event);
        assert_eq!(state.roles.len(), 1);
        assert_eq!(state.roles.get(&1).unwrap().name, "Test Role");
        assert_eq!(state.roles.get(&1).unwrap().permissions.len(), 2);

        // Test role assignment event
        let assignment_event = SecurityEvent::UserRoleAssigned {
            user_id: 1,
            role_id: 1,
            assigned_by: 0,
            timestamp: 1234567890,
        };

        state.apply_security_event(&assignment_event);
        let user_permissions = state.get_user_permissions(1);
        assert_eq!(user_permissions.len(), 2);
        assert!(user_permissions.contains(&TestPermission::ReadData));
        assert!(user_permissions.contains(&TestPermission::WriteData));
    }
}
