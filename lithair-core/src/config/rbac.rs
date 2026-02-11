//! RBAC configuration

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RbacConfig {
    pub enabled: bool,
    pub default_role: String,
    pub audit_enabled: bool,
    pub rate_limit_enabled: bool,
    pub max_login_attempts: usize,
    pub lockout_duration: u64,
}

impl Default for RbacConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            default_role: "guest".to_string(),
            audit_enabled: true,
            rate_limit_enabled: false,
            max_login_attempts: 5,
            lockout_duration: 300,
        }
    }
}

impl RbacConfig {
    pub fn merge(&mut self, other: Self) {
        *self = other;
    }

    pub fn apply_env_vars(&mut self) {
        if let Ok(enabled) = env::var("RS_RBAC_ENABLED") {
            self.enabled = enabled.parse().unwrap_or(false);
        }
        if let Ok(role) = env::var("RS_RBAC_DEFAULT_ROLE") {
            self.default_role = role;
        }
        if let Ok(audit) = env::var("RS_RBAC_AUDIT_ENABLED") {
            self.audit_enabled = audit.parse().unwrap_or(true);
        }
    }

    pub fn validate(&self) -> Result<()> {
        Ok(())
    }
}
