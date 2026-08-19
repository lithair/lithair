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
//! use lithair_core::session::{MemorySessionStore, SessionManager};
//!
//! # async fn example() -> anyhow::Result<()> {
//! // Build a session manager backed by an in-memory store. Wire it into
//! // a `LithairServer` via `LithairServer::new().with_sessions(manager)`
//! // (see `lithair_core::app::LithairServer`).
//! let manager = SessionManager::new(MemorySessionStore::new());
//! let _store = manager.store();
//! # Ok(())
//! # }
//! ```

mod cookie;
mod events;
mod manager;
mod memory;
mod middleware;
mod persistent_store;
mod store;

#[cfg(test)]
mod security_tests;

pub(crate) use cookie::{cookie_value, cross_site_request_blocked};
pub use cookie::{effective_cookie_config, CookieConfig, CrossSiteCheck, SessionCookie};
pub use manager::{SessionManager, SessionManagerConfig};
pub use memory::MemorySessionStore;
pub use middleware::SessionMiddleware;
pub use persistent_store::PersistentSessionStore;
pub use store::{Session, SessionStore};

/// Default name of the cookie carrying the session id
/// ([`CookieConfig::default`]).
///
/// The effective name — this default, a `with_session_cookie(...)` override,
/// or its `__Host-`-prefixed form — is what the login route sets, the logout
/// route clears, and the session gate / route guards / `SessionMiddleware`
/// read (issue #219).
pub const SESSION_COOKIE_NAME: &str = "session_token";

use chrono::Duration;
use std::sync::Arc;

/// One of the two session-store shapes Lithair currently stores in the
/// builder's `Arc<dyn Any>` slot and that both the require-session gate
/// (`http/declarative.rs::has_valid_session`) and the boot-time validation
/// (`app/mod.rs::serve`) recognize.
///
/// Centralizing the recognized set means the gate and the fail-fast check
/// share a single source of truth — if a new shape is added, both sides
/// gain support together rather than drifting (issue #80 was caused by a
/// drift between the constructor surface and the gate's known shapes).
pub(crate) enum RecognizedSessionStore {
    /// A raw `Arc<PersistentSessionStore>` — the shape produced by the
    /// RBAC builder path (`with_rbac_config(...)`).
    Persistent(Arc<PersistentSessionStore>),
    /// An `Arc<SessionManager<PersistentSessionStore>>` — the shape
    /// produced by `with_sessions(SessionManager::new(store_by_value))`
    /// or `with_sessions(SessionManager::from_arc(arc_store))`.
    Manager(Arc<SessionManager<PersistentSessionStore>>),
    /// A raw `Arc<MemorySessionStore>` — the in-memory store documented for
    /// development/testing.
    Memory(Arc<MemorySessionStore>),
    /// An `Arc<SessionManager<MemorySessionStore>>` — the shape produced by
    /// `with_sessions(SessionManager::new(MemorySessionStore::new()))`, the
    /// exact pattern in this module's doc example.
    MemoryManager(Arc<SessionManager<MemorySessionStore>>),
}

impl RecognizedSessionStore {
    /// Attempt to identify the concrete shape of a registered session
    /// store. Returns `Some` if it matches one of the built-in shapes the
    /// framework knows how to look sessions up in; `None` otherwise.
    ///
    /// Covers both built-in stores (`PersistentSessionStore`,
    /// `MemorySessionStore`), each either raw or wrapped in a
    /// `SessionManager`. A fully custom `SessionStore` impl is not recognized
    /// — the builder stores the manager as `Arc<dyn Any>`, which cannot be
    /// downcast to a trait object, so recognition must enumerate concrete
    /// types (issue #143 review).
    ///
    /// The `Arc::downcast` calls consume the `Arc`, so this clones once
    /// per attempt — cheap, only happens on misses.
    pub(crate) fn recognize(store_any: &Arc<dyn std::any::Any + Send + Sync>) -> Option<Self> {
        if let Ok(s) = store_any.clone().downcast::<PersistentSessionStore>() {
            return Some(Self::Persistent(s));
        }
        if let Ok(m) = store_any.clone().downcast::<SessionManager<PersistentSessionStore>>() {
            return Some(Self::Manager(m));
        }
        if let Ok(s) = store_any.clone().downcast::<MemorySessionStore>() {
            return Some(Self::Memory(s));
        }
        if let Ok(m) = store_any.clone().downcast::<SessionManager<MemorySessionStore>>() {
            return Some(Self::MemoryManager(m));
        }
        None
    }

    /// Look up a session by id in whichever underlying store this shape
    /// wraps. Returns the session only if found AND not expired — both
    /// callers (gate, future audit hooks) need the same liveness check,
    /// so we centralize it here.
    pub(crate) async fn get_live_session(&self, id: &str) -> Option<Session> {
        match self {
            Self::Persistent(store) => {
                store.get(id).await.ok().flatten().filter(|s| !s.is_expired())
            }
            Self::Manager(manager) => {
                manager.get_session(id).await.ok().flatten().filter(|s| !s.is_expired())
            }
            Self::Memory(store) => store.get(id).await.ok().flatten().filter(|s| !s.is_expired()),
            Self::MemoryManager(manager) => {
                manager.get_session(id).await.ok().flatten().filter(|s| !s.is_expired())
            }
        }
    }
}

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SameSitePolicy {
    /// Strict - cookie only sent to same site
    Strict,

    /// Lax - cookie sent on top-level navigation (the framework default)
    #[default]
    Lax,

    /// None - cookie sent on all requests (requires Secure)
    None,
}

impl SameSitePolicy {
    /// The attribute value as it appears in `Set-Cookie`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Strict => "Strict",
            Self::Lax => "Lax",
            Self::None => "None",
        }
    }
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

    /// D1: the middleware default and the RBAC login share one cookie name.
    #[test]
    fn default_cookie_name_is_the_canonical_session_token() {
        assert_eq!(SessionConfig::default().cookie_config.name, SESSION_COOKIE_NAME);
        assert_eq!(CookieConfig::default().name, "session_token");
        assert_eq!(SameSitePolicy::default(), SameSitePolicy::Lax);
    }
}
