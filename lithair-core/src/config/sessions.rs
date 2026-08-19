//! Sessions configuration

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::env;

/// Sessions configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionsConfig {
    /// Enable session management
    /// Env: LT_SESSION_ENABLED
    /// Default: true
    pub enabled: bool,

    /// Session cleanup interval in seconds — how often the `SessionManager`
    /// that `with_rbac_config` builds sweeps expired sessions out of the
    /// store. (`with_sessions` takes its own `SessionManagerConfig`.)
    /// Env: LT_SESSION_CLEANUP_INTERVAL
    /// Default: 300 (5 minutes)
    pub cleanup_interval: u64,

    /// Session maximum age in seconds, for the `with_sessions` /
    /// `SessionMiddleware` path. The RBAC path (`with_rbac_config`) does NOT
    /// read it: there `ServerRbacConfig::session_duration` is the lifetime
    /// of both the session and its cookie.
    /// Env: LT_SESSION_MAX_AGE
    /// Default: 3600 (1 hour)
    pub max_age: u64,

    /// Enable the session cookie. `false` = Bearer-only mode: the RBAC login
    /// returns the token in the JSON body only (no `Set-Cookie`), the logout
    /// emits no clear, and no extractor reads the `Cookie:` header
    /// (`CookieConfig::enabled`).
    /// Env: LT_SESSION_COOKIE_ENABLED
    /// Default: true
    pub cookie_enabled: bool,

    /// Set Secure flag on cookies (HTTPS only)
    /// Env: LT_SESSION_COOKIE_SECURE
    /// Default: true
    pub cookie_secure: bool,

    /// Set HttpOnly flag on cookies (XSS protection)
    /// Env: LT_SESSION_COOKIE_HTTPONLY
    /// Default: true
    pub cookie_httponly: bool,

    /// SameSite policy: "Strict", "Lax", or "None"
    /// Env: LT_SESSION_COOKIE_SAMESITE
    /// Default: "Lax"
    pub cookie_samesite: String,

    /// Cross-site request check on cookie-authenticated state-changing
    /// requests: "Enforce" (reject with 403) or "Off" (CSRF defense,
    /// `CookieConfig::cross_site_check`, issue #225).
    /// Env: LT_SESSION_CROSS_SITE_CHECK
    /// Default: "Enforce"
    #[serde(default = "default_cross_site_check")]
    pub cross_site_check: String,
}

/// Serde default: pre-1.10 TOML files omit the field.
fn default_cross_site_check() -> String {
    "Enforce".to_string()
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
            cross_site_check: "Enforce".to_string(),
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
        self.cross_site_check = other.cross_site_check;
    }

    pub fn apply_env_vars(&mut self) {
        if let Ok(enabled) = env::var("LT_SESSION_ENABLED") {
            self.enabled = enabled.parse().unwrap_or(true);
        }

        if let Ok(interval) = env::var("LT_SESSION_CLEANUP_INTERVAL") {
            if let Ok(i) = interval.parse() {
                self.cleanup_interval = i;
            }
        }

        if let Ok(max_age) = env::var("LT_SESSION_MAX_AGE") {
            if let Ok(m) = max_age.parse() {
                self.max_age = m;
            }
        }

        if let Ok(enabled) = env::var("LT_SESSION_COOKIE_ENABLED") {
            self.cookie_enabled = enabled.parse().unwrap_or(true);
        }

        if let Ok(secure) = env::var("LT_SESSION_COOKIE_SECURE") {
            self.cookie_secure = secure.parse().unwrap_or(true);
        }

        if let Ok(httponly) = env::var("LT_SESSION_COOKIE_HTTPONLY") {
            self.cookie_httponly = httponly.parse().unwrap_or(true);
        }

        if let Ok(samesite) = env::var("LT_SESSION_COOKIE_SAMESITE") {
            self.cookie_samesite = samesite;
        }

        if let Ok(check) = env::var("LT_SESSION_CROSS_SITE_CHECK") {
            self.cross_site_check = check;
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

        if !["Enforce", "Off"].contains(&self.cross_site_check.as_str()) {
            bail!("Invalid cross_site_check: must be Enforce or Off");
        }

        Ok(())
    }
}
