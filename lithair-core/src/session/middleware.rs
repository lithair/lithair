//! Session middleware for HTTP requests

use super::cookie::SessionCookie;
use super::{Session, SessionConfig, SessionStore};
use anyhow::Result;
use http::Request;
use std::sync::Arc;

/// Session middleware
///
/// Extracts session ID from HTTP requests (Cookie or Bearer token)
/// and loads the session from the store.
pub struct SessionMiddleware<S: SessionStore> {
    store: Arc<S>,
    #[allow(dead_code)]
    config: SessionConfig,
    cookie: Option<SessionCookie>,
    bearer_enabled: bool,
}

impl<S: SessionStore> SessionMiddleware<S> {
    /// Create a new session middleware
    pub fn new(store: Arc<S>, config: SessionConfig) -> Self {
        let cookie = if config.cookie_enabled && config.cookie_config.enabled {
            Some(SessionCookie::new(config.cookie_config.clone()))
        } else {
            None
        };

        Self { store, config: config.clone(), cookie, bearer_enabled: config.bearer_enabled }
    }

    /// Extract session from HTTP request
    ///
    /// Tries the Bearer token first (if enabled), then the cookie (if enabled)
    pub async fn extract_session<B>(&self, req: &Request<B>) -> Result<Option<Session>> {
        // Try to extract session ID
        let session_id = self.extract_session_id(req);

        if let Some(id) = session_id {
            // Load session from store
            if let Some(mut session) = self.store.get(&id).await? {
                // Check if expired
                if session.is_expired() {
                    // Delete expired session
                    self.store.delete(&id).await?;
                    return Ok(None);
                }

                // Update last accessed time
                session.touch();
                self.store.set(session.clone()).await?;

                return Ok(Some(session));
            }
        }

        Ok(None)
    }

    /// Extract session ID from request — the same extractor as the session
    /// gate / route guards (Bearer first, then the configured cookie), gated
    /// by this middleware's `bearer_enabled` / `cookie_enabled` flags.
    fn extract_session_id<B>(&self, req: &Request<B>) -> Option<String> {
        let cookie_name = self.cookie.as_ref().map(|c| c.config().effective_name());
        crate::http::declarative::extract_session_token_from(
            req,
            self.bearer_enabled,
            cookie_name.as_deref(),
        )
    }

    /// Get the session store
    pub fn store(&self) -> Arc<S> {
        Arc::clone(&self.store)
    }
}

// Update SessionConfig to include auth method flags
impl SessionConfig {
    /// Enable cookie-based authentication
    pub fn with_cookie_auth(mut self, enabled: bool) -> Self {
        self.cookie_enabled = enabled;
        self
    }

    /// Enable Bearer token authentication
    pub fn with_bearer_auth(mut self, enabled: bool) -> Self {
        self.bearer_enabled = enabled;
        self
    }

    /// Preset: Cookie-only authentication
    pub fn cookie_only() -> Self {
        Self::default().with_cookie_auth(true).with_bearer_auth(false)
    }

    /// Preset: Bearer-only authentication
    pub fn bearer_only() -> Self {
        Self::default().with_cookie_auth(false).with_bearer_auth(true)
    }

    /// Preset: Hybrid authentication (both Cookie and Bearer)
    pub fn hybrid() -> Self {
        Self::default().with_cookie_auth(true).with_bearer_auth(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::MemorySessionStore;
    use bytes::Bytes;
    use chrono::{Duration, Utc};
    use http::Request;
    use http_body_util::Full;

    #[tokio::test]
    async fn test_extract_from_cookie() {
        let store = Arc::new(MemorySessionStore::new());
        let config = SessionConfig::cookie_only();
        let middleware = SessionMiddleware::new(store.clone(), config);

        // Create a session
        let expires_at = Utc::now() + Duration::hours(1);
        let mut session = Session::new("cookie-session-123".to_string(), expires_at);
        session.set("user_id", "alice").unwrap();
        store.set(session.clone()).await.unwrap();

        // Build request with cookie
        let req = Request::builder()
            .header("cookie", "session_token=cookie-session-123")
            .body(Full::new(Bytes::new()))
            .unwrap();

        // Extract session
        let extracted = middleware.extract_session(&req).await.unwrap();
        assert!(extracted.is_some());

        let extracted = extracted.unwrap();
        assert_eq!(extracted.id, "cookie-session-123");
        assert_eq!(extracted.get::<String>("user_id"), Some("alice".to_string()));
    }

    #[tokio::test]
    async fn test_extract_from_bearer() {
        let store = Arc::new(MemorySessionStore::new());
        let config = SessionConfig::bearer_only();
        let middleware = SessionMiddleware::new(store.clone(), config);

        // Create a session
        let expires_at = Utc::now() + Duration::hours(1);
        let mut session = Session::new("bearer-token-456".to_string(), expires_at);
        session.set("user_id", "bob").unwrap();
        store.set(session.clone()).await.unwrap();

        // Build request with Bearer token
        let req = Request::builder()
            .header("authorization", "Bearer bearer-token-456")
            .body(Full::new(Bytes::new()))
            .unwrap();

        // Extract session
        let extracted = middleware.extract_session(&req).await.unwrap();
        assert!(extracted.is_some());

        let extracted = extracted.unwrap();
        assert_eq!(extracted.id, "bearer-token-456");
        assert_eq!(extracted.get::<String>("user_id"), Some("bob".to_string()));
    }

    #[tokio::test]
    async fn test_hybrid_priority() {
        let store = Arc::new(MemorySessionStore::new());
        let config = SessionConfig::hybrid();
        let middleware = SessionMiddleware::new(store.clone(), config);

        // Create two sessions
        let expires_at = Utc::now() + Duration::hours(1);

        let mut cookie_session = Session::new("cookie-session".to_string(), expires_at);
        cookie_session.set("source", "cookie").unwrap();
        store.set(cookie_session).await.unwrap();

        let mut bearer_session = Session::new("bearer-session".to_string(), expires_at);
        bearer_session.set("source", "bearer").unwrap();
        store.set(bearer_session).await.unwrap();

        // Request with BOTH cookie and bearer
        let req = Request::builder()
            .header("cookie", "session_token=cookie-session")
            .header("authorization", "Bearer bearer-session")
            .body(Full::new(Bytes::new()))
            .unwrap();

        // Bearer wins — same priority as the session gate and route guards.
        let extracted = middleware.extract_session(&req).await.unwrap();
        assert!(extracted.is_some());

        let extracted = extracted.unwrap();
        assert_eq!(extracted.get::<String>("source"), Some("bearer".to_string()));
    }

    /// The `bearer_enabled` / `cookie_enabled` flags still gate each source.
    #[tokio::test]
    async fn disabled_sources_are_ignored() {
        let store = Arc::new(MemorySessionStore::new());
        let expires_at = Utc::now() + Duration::hours(1);
        store.set(Session::new("s1".to_string(), expires_at)).await.unwrap();

        let cookie_only = SessionMiddleware::new(store.clone(), SessionConfig::cookie_only());
        let bearer_req = Request::builder()
            .header("authorization", "bearer s1")
            .body(Full::new(Bytes::new()))
            .unwrap();
        assert!(cookie_only.extract_session(&bearer_req).await.unwrap().is_none());

        let bearer_only = SessionMiddleware::new(store.clone(), SessionConfig::bearer_only());
        let cookie_req = Request::builder()
            .header("cookie", "session_token=s1")
            .body(Full::new(Bytes::new()))
            .unwrap();
        assert!(bearer_only.extract_session(&cookie_req).await.unwrap().is_none());
        // Lower-case scheme is accepted (harmonized with the gate).
        assert!(bearer_only.extract_session(&bearer_req).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_expired_session() {
        let store = Arc::new(MemorySessionStore::new());
        let config = SessionConfig::cookie_only();
        let middleware = SessionMiddleware::new(store.clone(), config);

        // Create expired session
        let expires_at = Utc::now() - Duration::seconds(1);
        let session = Session::new("expired-session".to_string(), expires_at);
        store.set(session).await.unwrap();

        // Request with expired session
        let req = Request::builder()
            .header("cookie", "session_token=expired-session")
            .body(Full::new(Bytes::new()))
            .unwrap();

        // Should return None and delete the session
        let extracted = middleware.extract_session(&req).await.unwrap();
        assert!(extracted.is_none());

        // Session should be deleted from store
        assert!(!store.exists("expired-session").await.unwrap());
    }
}
