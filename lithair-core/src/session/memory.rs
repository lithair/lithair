//! In-memory session storage
//!
//! This implementation uses a thread-safe HashMap with RwLock.
//! Suitable for development and single-server deployments.
//! For production with multiple servers, use Redis or PostgreSQL.

use super::store::{Session, SessionStore};
use anyhow::Result;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// In-memory session store
///
/// Thread-safe session storage using RwLock<HashMap>.
/// Sessions are stored in memory and will be lost on server restart.
///
/// # Example
///
/// ```
/// use lithair_core::session::MemorySessionStore;
///
/// let store = MemorySessionStore::new();
/// ```
#[derive(Clone)]
pub struct MemorySessionStore {
    sessions: Arc<RwLock<HashMap<String, Session>>>,
}

impl MemorySessionStore {
    /// Create a new in-memory session store
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Get the number of sessions currently stored
    pub fn session_count(&self) -> usize {
        self.sessions.read().unwrap().len()
    }
}

impl Default for MemorySessionStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl SessionStore for MemorySessionStore {
    async fn get(&self, id: &str) -> Result<Option<Session>> {
        let sessions = self.sessions.read().unwrap();
        Ok(sessions.get(id).cloned())
    }
    
    async fn set(&self, session: Session) -> Result<()> {
        let mut sessions = self.sessions.write().unwrap();
        sessions.insert(session.id.clone(), session);
        Ok(())
    }
    
    async fn delete(&self, id: &str) -> Result<()> {
        let mut sessions = self.sessions.write().unwrap();
        sessions.remove(id);
        Ok(())
    }
    
    async fn exists(&self, id: &str) -> Result<bool> {
        let sessions = self.sessions.read().unwrap();
        Ok(sessions.contains_key(id))
    }
    
    async fn cleanup_expired(&self) -> Result<usize> {
        let mut sessions = self.sessions.write().unwrap();
        let initial_count = sessions.len();
        
        // Remove expired sessions
        sessions.retain(|_, session| !session.is_expired());
        
        let removed = initial_count - sessions.len();
        Ok(removed)
    }
    
    async fn count(&self) -> Result<usize> {
        let sessions = self.sessions.read().unwrap();
        Ok(sessions.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};
    
    #[tokio::test]
    async fn test_memory_store_basic() {
        let store = MemorySessionStore::new();
        
        // Create session
        let expires_at = Utc::now() + Duration::hours(1);
        let mut session = Session::new("test-123".to_string(), expires_at);
        session.set("user_id", "alice").unwrap();
        
        // Store session
        store.set(session.clone()).await.unwrap();
        
        // Retrieve session
        let retrieved = store.get("test-123").await.unwrap();
        assert!(retrieved.is_some());
        
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.id, "test-123");
        assert_eq!(retrieved.get::<String>("user_id"), Some("alice".to_string()));
    }
    
    #[tokio::test]
    async fn test_memory_store_delete() {
        let store = MemorySessionStore::new();
        
        let expires_at = Utc::now() + Duration::hours(1);
        let session = Session::new("test-456".to_string(), expires_at);
        
        store.set(session).await.unwrap();
        assert!(store.exists("test-456").await.unwrap());
        
        store.delete("test-456").await.unwrap();
        assert!(!store.exists("test-456").await.unwrap());
    }
    
    #[tokio::test]
    async fn test_memory_store_cleanup() {
        let store = MemorySessionStore::new();
        
        // Create expired session
        let expired = Session::new(
            "expired".to_string(),
            Utc::now() - Duration::seconds(1),
        );
        
        // Create valid session
        let valid = Session::new(
            "valid".to_string(),
            Utc::now() + Duration::hours(1),
        );
        
        store.set(expired).await.unwrap();
        store.set(valid).await.unwrap();
        
        assert_eq!(store.count().await.unwrap(), 2);
        
        // Cleanup expired
        let removed = store.cleanup_expired().await.unwrap();
        assert_eq!(removed, 1);
        assert_eq!(store.count().await.unwrap(), 1);
        
        // Valid session should still exist
        assert!(store.exists("valid").await.unwrap());
        assert!(!store.exists("expired").await.unwrap());
    }
    
    #[tokio::test]
    async fn test_memory_store_concurrent() {
        let store = MemorySessionStore::new();
        
        // Spawn multiple tasks writing sessions
        let mut handles = vec![];
        
        for i in 0..10 {
            let store_clone = store.clone();
            let handle = tokio::spawn(async move {
                let expires_at = Utc::now() + Duration::hours(1);
                let session = Session::new(format!("session-{}", i), expires_at);
                store_clone.set(session).await.unwrap();
            });
            handles.push(handle);
        }
        
        // Wait for all tasks
        for handle in handles {
            handle.await.unwrap();
        }
        
        // All sessions should be stored
        assert_eq!(store.count().await.unwrap(), 10);
    }
}
