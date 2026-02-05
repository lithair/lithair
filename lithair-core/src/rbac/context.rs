//! Authentication and authorization context

use std::collections::HashMap;

/// Authentication context containing user identity and permissions
#[derive(Debug, Clone)]
pub struct AuthContext {
    /// User identifier (from LDAP, OAuth, etc.)
    pub user_id: Option<String>,

    /// User roles (application-level)
    pub roles: Vec<String>,

    /// User groups (from identity provider like LDAP)
    pub groups: Vec<String>,

    /// Whether the user is authenticated
    pub authenticated: bool,

    /// Authentication provider name
    pub provider: String,

    /// Whether MFA was verified (if required)
    pub mfa_verified: bool,

    /// Provider-specific metadata
    pub metadata: HashMap<String, String>,
}

impl Default for AuthContext {
    fn default() -> Self {
        Self {
            user_id: None,
            roles: vec!["Public".to_string()],
            groups: vec![],
            authenticated: false,
            provider: "none".to_string(),
            mfa_verified: false,
            metadata: HashMap::new(),
        }
    }
}

impl AuthContext {
    /// Create an unauthenticated context
    pub fn unauthenticated() -> Self {
        Self::default()
    }

    /// Create an authenticated context
    pub fn authenticated(user_id: String, roles: Vec<String>, provider: String) -> Self {
        Self {
            user_id: Some(user_id),
            roles,
            groups: vec![],
            authenticated: true,
            provider,
            mfa_verified: false,
            metadata: HashMap::new(),
        }
    }

    /// Check if user has a specific role
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }

    /// Check if user is in a specific group
    pub fn has_group(&self, group: &str) -> bool {
        self.groups.iter().any(|g| g == group)
    }

    /// Check if user has role OR group
    pub fn has_role_or_group(&self, name: &str) -> bool {
        self.has_role(name) || self.has_group(name)
    }
}

/// RBAC context for request processing
#[derive(Debug, Clone)]
pub struct RbacContext {
    /// Authentication context
    pub auth: AuthContext,

    /// Request path
    pub path: String,

    /// HTTP method
    pub method: String,
}

impl RbacContext {
    /// Create a new RBAC context
    pub fn new(auth: AuthContext, path: String, method: String) -> Self {
        Self { auth, path, method }
    }
}
