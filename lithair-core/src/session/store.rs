//! Session storage trait and types

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Session data - flexible key-value store
pub type SessionData = HashMap<String, serde_json::Value>;

/// User session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Unique session ID
    pub id: String,

    /// Session data (flexible key-value store)
    pub data: SessionData,

    /// Session creation time
    pub created_at: DateTime<Utc>,

    /// Session expiration time
    pub expires_at: DateTime<Utc>,

    /// Last access time (for sliding expiration)
    pub last_accessed_at: DateTime<Utc>,
}

impl Session {
    /// Create a new session with the given ID and expiration
    pub fn new(id: String, expires_at: DateTime<Utc>) -> Self {
        let now = Utc::now();
        Self { id, data: HashMap::new(), created_at: now, expires_at, last_accessed_at: now }
    }

    /// Check if the session is expired
    pub fn is_expired(&self) -> bool {
        Utc::now() >= self.expires_at
    }

    /// Update last accessed time
    pub fn touch(&mut self) {
        self.last_accessed_at = Utc::now();
    }

    /// Get a value from session data
    pub fn get<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Option<T> {
        self.data.get(key).and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    /// Set a value in session data
    pub fn set<T: Serialize>(&mut self, key: impl Into<String>, value: T) -> Result<()> {
        let json_value = serde_json::to_value(value)?;
        self.data.insert(key.into(), json_value);
        Ok(())
    }

    /// Remove a value from session data
    pub fn remove(&mut self, key: &str) -> Option<serde_json::Value> {
        self.data.remove(key)
    }

    /// Check if a key exists
    pub fn contains(&self, key: &str) -> bool {
        self.data.contains_key(key)
    }

    /// Clear all session data
    pub fn clear(&mut self) {
        self.data.clear();
    }

    /// Get the number of items in session
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Check if session data is empty
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

/// Session storage trait
///
/// Implement this trait to provide custom session storage backends
/// (Memory, Redis, PostgreSQL, etc.)
#[async_trait::async_trait]
pub trait SessionStore: Send + Sync {
    /// Get a session by ID
    async fn get(&self, id: &str) -> Result<Option<Session>>;

    /// Store a session
    async fn set(&self, session: Session) -> Result<()>;

    /// Delete a session by ID
    async fn delete(&self, id: &str) -> Result<()>;

    /// Check if a session exists
    async fn exists(&self, id: &str) -> Result<bool> {
        Ok(self.get(id).await?.is_some())
    }

    /// Clean up expired sessions
    /// Returns the number of sessions deleted
    async fn cleanup_expired(&self) -> Result<usize>;

    /// Get the total number of sessions
    async fn count(&self) -> Result<usize>;
}

// Implement SessionStore for Arc<S> to allow using Arc directly
#[async_trait::async_trait]
impl<S: SessionStore> SessionStore for std::sync::Arc<S> {
    async fn get(&self, id: &str) -> Result<Option<Session>> {
        (**self).get(id).await
    }

    async fn set(&self, session: Session) -> Result<()> {
        (**self).set(session).await
    }

    async fn delete(&self, id: &str) -> Result<()> {
        (**self).delete(id).await
    }

    async fn exists(&self, id: &str) -> Result<bool> {
        (**self).exists(id).await
    }

    async fn cleanup_expired(&self) -> Result<usize> {
        (**self).cleanup_expired().await
    }

    async fn count(&self) -> Result<usize> {
        (**self).count().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_session_creation() {
        let expires_at = Utc::now() + Duration::hours(1);
        let session = Session::new("test-id".to_string(), expires_at);

        assert_eq!(session.id, "test-id");
        assert!(session.data.is_empty());
        assert!(!session.is_expired());
    }

    #[test]
    fn test_session_data() {
        let expires_at = Utc::now() + Duration::hours(1);
        let mut session = Session::new("test-id".to_string(), expires_at);

        // Set values
        session.set("user_id", "123").unwrap();
        session.set("username", "alice").unwrap();
        session.set("count", 42).unwrap();

        // Get values
        assert_eq!(session.get::<String>("user_id"), Some("123".to_string()));
        assert_eq!(session.get::<String>("username"), Some("alice".to_string()));
        assert_eq!(session.get::<i32>("count"), Some(42));

        // Check existence
        assert!(session.contains("user_id"));
        assert!(!session.contains("nonexistent"));

        // Remove
        session.remove("count");
        assert!(!session.contains("count"));

        // Length
        assert_eq!(session.len(), 2);
    }

    #[test]
    fn test_session_expiration() {
        let expires_at = Utc::now() - Duration::seconds(1);
        let session = Session::new("test-id".to_string(), expires_at);

        assert!(session.is_expired());
    }

    #[test]
    fn test_session_touch() {
        let expires_at = Utc::now() + Duration::hours(1);
        let mut session = Session::new("test-id".to_string(), expires_at);

        let initial_access = session.last_accessed_at;
        std::thread::sleep(std::time::Duration::from_millis(10));

        session.touch();
        assert!(session.last_accessed_at > initial_access);
    }
}
