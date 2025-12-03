//! Permission system for RBAC

/// Permission level for operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionLevel {
    /// Public access (no authentication required)
    Public,
    /// Read access
    Read,
    /// Write access
    Write,
    /// Delete access
    Delete,
    /// Admin access
    Admin,
}

/// Field-level permission configuration
#[derive(Debug, Clone)]
pub struct FieldPermission {
    /// Field name
    pub field_name: String,
    
    /// Roles/groups that can read this field
    pub read: Vec<String>,
    
    /// Roles/groups that can write this field
    pub write: Vec<String>,
}

impl FieldPermission {
    /// Create a new field permission
    pub fn new(field_name: String) -> Self {
        Self {
            field_name,
            read: vec!["Public".to_string()],
            write: vec!["Admin".to_string()],
        }
    }
    
    /// Set read permissions
    pub fn with_read(mut self, roles: Vec<String>) -> Self {
        self.read = roles;
        self
    }
    
    /// Set write permissions
    pub fn with_write(mut self, roles: Vec<String>) -> Self {
        self.write = roles;
        self
    }
    
    /// Check if role/group can read this field
    pub fn can_read(&self, role_or_group: &str) -> bool {
        self.read.iter().any(|r| r == role_or_group || r == "Public")
    }
    
    /// Check if role/group can write this field
    pub fn can_write(&self, role_or_group: &str) -> bool {
        self.write.iter().any(|r| r == role_or_group)
    }
}

/// Generic permission type
#[derive(Debug, Clone)]
pub struct Permission {
    /// Permission name
    pub name: String,
    
    /// Required roles/groups
    pub required_roles: Vec<String>,
    
    /// Permission level
    pub level: PermissionLevel,
}

impl Permission {
    /// Create a new permission
    pub fn new(name: String, level: PermissionLevel) -> Self {
        Self {
            name,
            required_roles: vec![],
            level,
        }
    }
    
    /// Add required role
    pub fn require_role(mut self, role: String) -> Self {
        self.required_roles.push(role);
        self
    }
    
    /// Check if roles satisfy this permission
    pub fn is_satisfied_by(&self, roles: &[String]) -> bool {
        if self.required_roles.is_empty() {
            return true;
        }
        
        self.required_roles.iter().any(|req| roles.contains(req))
    }
}
