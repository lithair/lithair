//! Sessions configuration

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::env;

/// Sessions configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionsConfig {
    /// Enable session management
    /// Env: RS_SESSION_ENABLED
    /// Default: true
    pub enabled: bool,

    /// Session cleanup interval in seconds
    /// Env: RS_SESSION_CLEANUP_INTERVAL
    /// Default: 300 (5 minutes)
    pub cleanup_interval: u64,

    /// Session maximum age in seconds
    /// Env: RS_SESSION_MAX_AGE
    /// Default: 3600 (1 hour)
    pub max_age: u64,

    /// Enable cookie-based sessions
    /// Env: RS_SESSION_COOKIE_ENABLED
    /// Default: true
    pub cookie_enabled: bool,

    /// Set Secure flag on cookies (HTTPS only)
    /// Env: RS_SESSION_COOKIE_SECURE
    /// Default: true
    pub cookie_secure: bool,

    /// Set HttpOnly flag on cookies (XSS protection)
    /// Env: RS_SESSION_COOKIE_HTTPONLY
    /// Default: true
    pub cookie_httponly: bool,

    /// SameSite policy: "Strict", "Lax", or "None"
    /// Env: RS_SESSION_COOKIE_SAMESITE
    /// Default: "Lax"
    pub cookie_samesite: String,
}

impl Default for SessionsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            cleanup_interval: 300,
            max_age: 3600,
            cookie_enabled: true,
            cookie_secure: true,
            cookie_httponly: true,
            cookie_samesite: "Lax".to_string(),
        }
    }
}

impl SessionsConfig {
    pub fn merge(&mut self, other: Self) {
        self.enabled = other.enabled;
        self.cleanup_interval = other.cleanup_interval;
        self.max_age = other.max_age;
        self.cookie_enabled = other.cookie_enabled;
        self.cookie_secure = other.cookie_secure;
        self.cookie_httponly = other.cookie_httponly;
        self.cookie_samesite = other.cookie_samesite;
    }

    pub fn apply_env_vars(&mut self) {
        if let Ok(enabled) = env::var("RS_SESSION_ENABLED") {
            self.enabled = enabled.parse().unwrap_or(true);
        }

        if let Ok(interval) = env::var("RS_SESSION_CLEANUP_INTERVAL") {
            if let Ok(i) = interval.parse() {
                self.cleanup_interval = i;
            }
        }

        if let Ok(max_age) = env::var("RS_SESSION_MAX_AGE") {
            if let Ok(m) = max_age.parse() {
                self.max_age = m;
            }
        }

        if let Ok(enabled) = env::var("RS_SESSION_COOKIE_ENABLED") {
            self.cookie_enabled = enabled.parse().unwrap_or(true);
        }

        if let Ok(secure) = env::var("RS_SESSION_COOKIE_SECURE") {
            self.cookie_secure = secure.parse().unwrap_or(true);
        }

        if let Ok(httponly) = env::var("RS_SESSION_COOKIE_HTTPONLY") {
            self.cookie_httponly = httponly.parse().unwrap_or(true);
        }

        if let Ok(samesite) = env::var("RS_SESSION_COOKIE_SAMESITE") {
            self.cookie_samesite = samesite;
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.cleanup_interval == 0 {
            bail!("Invalid cleanup_interval: must be greater than 0");
        }

        if self.max_age == 0 {
            bail!("Invalid max_age: must be greater than 0");
        }

        if !["Strict", "Lax", "None"].contains(&self.cookie_samesite.as_str()) {
            bail!("Invalid cookie_samesite: must be Strict, Lax, or None");
        }

        Ok(())
    }
}
