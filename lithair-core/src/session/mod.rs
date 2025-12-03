//! Session management for Lithair
//!
//! This module provides a complete session management system for web applications:
//! - Trait-based session storage (Memory, Redis, PostgreSQL, etc.)
//! - Secure cookie management
//! - HTTP middleware for automatic session injection
//! - Integration with RBAC for authentication
//!
//! # Example
//!
//! ```no_run
//! use lithair_core::session::{SessionConfig, MemorySessionStore};
//! use lithair_core::http::DeclarativeServer;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let session_config = SessionConfig::new()
//!     .with_store(MemorySessionStore::new())
//!     .with_cookie_name("app_session")
//!     .with_max_age(std::time::Duration::from_secs(3600 * 24));
//!
//! // Sessions will be automatically available in request handlers
//! # Ok(())
//! # }
//! ```

mod cookie;
mod manager;
mod memory;
mod middleware;
mod events;
mod persistent_store;
mod store;

#[cfg(test)]
mod security_tests;

pub use manager::{SessionManager, SessionManagerConfig};
pub use memory::MemorySessionStore;
pub use middleware::SessionMiddleware;
pub use persistent_store::PersistentSessionStore;
pub use store::{Session, SessionStore};

use chrono::Duration;

/// Session configuration
#[derive(Clone)]
pub struct SessionConfig {
    /// Session maximum age
    pub max_age: Duration,
    
    /// Enable cookie-based authentication
    pub cookie_enabled: bool,
    
    /// Enable Bearer token authentication
    pub bearer_enabled: bool,
    
    /// Cookie configuration
    pub cookie_config: cookie::CookieConfig,
}

/// SameSite cookie policy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SameSitePolicy {
    /// Strict - cookie only sent to same site
    Strict,
    
    /// Lax - cookie sent on top-level navigation
    Lax,
    
    /// None - cookie sent on all requests (requires Secure)
    None,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_age: Duration::hours(24),
            cookie_enabled: true,
            bearer_enabled: false,
            cookie_config: cookie::CookieConfig::default(),
        }
    }
}

impl SessionConfig {
    /// Create a new session configuration with defaults
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Set maximum age
    pub fn with_max_age(mut self, max_age: std::time::Duration) -> Self {
        self.max_age = Duration::from_std(max_age).unwrap_or(Duration::hours(24));
        self.cookie_config.max_age = Some(max_age.as_secs() as i64);
        self
    }
    
    /// Set cookie name
    pub fn with_cookie_name(mut self, name: impl Into<String>) -> Self {
        self.cookie_config.name = name.into();
        self
    }
    
    /// Set secure flag
    pub fn with_secure(mut self, secure: bool) -> Self {
        self.cookie_config.secure = secure;
        self
    }
    
    /// Set HTTP only flag
    pub fn with_http_only(mut self, http_only: bool) -> Self {
        self.cookie_config.http_only = http_only;
        self
    }
    
    /// Set SameSite policy
    pub fn with_same_site(mut self, same_site: SameSitePolicy) -> Self {
        self.cookie_config.same_site = same_site;
        self
    }
    
    /// Set cookie domain
    pub fn with_domain(mut self, domain: impl Into<String>) -> Self {
        self.cookie_config.domain = Some(domain.into());
        self
    }
    
    /// Set cookie path
    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.cookie_config.path = path.into();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_session_config_builder() {
        let config = SessionConfig::new()
            .with_cookie_name("my_session")
            .with_max_age(std::time::Duration::from_secs(3600))
            .with_secure(true)
            .with_same_site(SameSitePolicy::Strict);
        
        assert_eq!(config.cookie_config.name, "my_session");
        assert_eq!(config.max_age, Duration::hours(1));
        assert!(config.cookie_config.secure);
        assert_eq!(config.cookie_config.same_site, SameSitePolicy::Strict);
    }
}
