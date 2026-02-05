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
        let cookie = if config.cookie_enabled {
            Some(SessionCookie::new(config.cookie_config.clone()))
        } else {
            None
        };

        Self { store, config: config.clone(), cookie, bearer_enabled: config.bearer_enabled }
    }

    /// Extract session from HTTP request
    ///
    /// Tries Cookie first (if enabled), then Bearer token (if enabled)
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

    /// Extract session ID from request
    ///
    /// Priority: Cookie > Bearer token
    fn extract_session_id<B>(&self, req: &Request<B>) -> Option<String> {
        // 1. Try Cookie (if enabled)
        if let Some(ref cookie) = self.cookie {
            if let Some(id) = self.extract_from_cookie(req, cookie) {
                log::debug!("Session ID extracted from cookie");
                return Some(id);
            }
        }

        // 2. Try Bearer token (if enabled)
        if self.bearer_enabled {
            if let Some(id) = self.extract_from_bearer(req) {
                log::debug!("Session ID extracted from Bearer token");
                return Some(id);
            }
        }

        None
    }

    /// Extract session ID from Cookie header
    fn extract_from_cookie<B>(&self, req: &Request<B>, cookie: &SessionCookie) -> Option<String> {
        req.headers()
            .get("cookie")
            .and_then(|v| v.to_str().ok())
            .and_then(|header| cookie.extract_from_header(header))
    }

    /// Extract session ID from Authorization Bearer header
    fn extract_from_bearer<B>(&self, req: &Request<B>) -> Option<String> {
        req.headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .map(|s| s.trim().to_string())
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
            .header("cookie", "session_id=cookie-session-123")
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
            .header("cookie", "session_id=cookie-session")
            .header("authorization", "Bearer bearer-session")
            .body(Full::new(Bytes::new()))
            .unwrap();

        // Cookie should have priority
        let extracted = middleware.extract_session(&req).await.unwrap();
        assert!(extracted.is_some());

        let extracted = extracted.unwrap();
        assert_eq!(extracted.get::<String>("source"), Some("cookie".to_string()));
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
            .header("cookie", "session_id=expired-session")
            .body(Full::new(Bytes::new()))
            .unwrap();

        // Should return None and delete the session
        let extracted = middleware.extract_session(&req).await.unwrap();
        assert!(extracted.is_none());

        // Session should be deleted from store
        assert!(!store.exists("expired-session").await.unwrap());
    }
}
