//! Lithair session store with event sourcing
//!
//! This implementation uses Lithair's SCC2 engine to provide:
//! - Event-sourced session management
//! - Memory-served performance
//! - Persistent sessions across restarts
//! - Complete audit trail
//! - Automatic snapshots

use super::store::{Session, SessionData, SessionStore};
use crate::engine::Scc2Engine;
use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Session state managed by event sourcing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    /// All active sessions
    sessions: HashMap<String, Session>,
    
    /// Total sessions created (for metrics)
    total_created: u64,
    
    /// Total sessions deleted (for metrics)
    total_deleted: u64,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            sessions: HashMap::new(),
            total_created: 0,
            total_deleted: 0,
        }
    }
}

/// Session events for event sourcing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionEvent {
    /// Session created
    Created {
        id: String,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    },
    
    /// Session data updated
    DataSet {
        id: String,
        key: String,
        value: serde_json::Value,
    },
    
    /// Session data removed
    DataRemoved {
        id: String,
        key: String,
    },
    
    /// Session accessed (updates last_accessed_at)
    Accessed {
        id: String,
        timestamp: DateTime<Utc>,
    },
    
    /// Session deleted
    Deleted {
        id: String,
        reason: String,
    },
    
    /// Bulk cleanup of expired sessions
    ExpiredCleaned {
        session_ids: Vec<String>,
        count: usize,
    },
}

impl SessionState {
    /// Apply a session event to the state
    fn apply_event(&mut self, event: SessionEvent) {
        match event {
            SessionEvent::Created { id, created_at, expires_at } => {
                // Security: Verify ID doesn't already exist
                if self.sessions.contains_key(&id) {
                    log::error!("🚨 SECURITY: Duplicate session ID detected: {}", id);
                    return;
                }
                
                let session = Session {
                    id: id.clone(),
                    data: HashMap::new(),
                    created_at,
                    expires_at,
                    last_accessed_at: created_at,
                };
                
                self.sessions.insert(id, session);
                self.total_created += 1;
                
                log::debug!("Session created (total: {})", self.sessions.len());
            }
            
            SessionEvent::DataSet { id, key, value } => {
                if let Some(session) = self.sessions.get_mut(&id) {
                    session.data.insert(key, value);
                } else {
                    log::warn!("Attempted to set data on non-existent session: {}", id);
                }
            }
            
            SessionEvent::DataRemoved { id, key } => {
                if let Some(session) = self.sessions.get_mut(&id) {
                    session.data.remove(&key);
                }
            }
            
            SessionEvent::Accessed { id, timestamp } => {
                if let Some(session) = self.sessions.get_mut(&id) {
                    session.last_accessed_at = timestamp;
                }
            }
            
            SessionEvent::Deleted { id, reason } => {
                if self.sessions.remove(&id).is_some() {
                    self.total_deleted += 1;
                    log::debug!("Session deleted: {} (reason: {})", id, reason);
                }
            }
            
            SessionEvent::ExpiredCleaned { session_ids, count } => {
                for id in session_ids {
                    self.sessions.remove(&id);
                }
                self.total_deleted += count as u64;
                log::info!("Cleaned {} expired sessions", count);
            }
        }
    }
}

/// Lithair session store with event sourcing
///
/// Uses SCC2Engine for ultra-fast, persistent session storage.
/// All session operations are event-sourced for complete audit trail.
///
/// # Example
///
/// ```no_run
/// use lithair_core::session::LithairSessionStore;
///
/// # async fn example() -> anyhow::Result<()> {
/// let store = LithairSessionStore::new("./data/sessions.events").await?;
/// # Ok(())
/// # }
/// ```
pub struct LithairSessionStore {
    engine: Arc<SCC2Engine<SessionState>>,
}

impl LithairSessionStore {
    /// Create a new Lithair session store
    ///
    /// # Arguments
    ///
    /// * `event_store_path` - Path to the event log file
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use lithair_core::session::LithairSessionStore;
    /// # async fn example() -> anyhow::Result<()> {
    /// let store = LithairSessionStore::new("./data/sessions.events").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn new(event_store_path: impl AsRef<str>) -> Result<Self> {
        let engine = SCC2Engine::new(event_store_path.as_ref())?;
        
        // Load existing sessions from event log
        let state = engine.read_state().await;
        log::info!(
            "Lithair session store initialized: {} active sessions, {} total created",
            state.sessions.len(),
            state.total_created
        );
        
        Ok(Self {
            engine: Arc::new(engine),
        })
    }
    
    /// Get session metrics
    pub async fn metrics(&self) -> SessionMetrics {
        let state = self.engine.read_state().await;
        SessionMetrics {
            active_sessions: state.sessions.len(),
            total_created: state.total_created,
            total_deleted: state.total_deleted,
        }
    }
    
    /// Create a new session with the given ID and expiration
    pub async fn create_session(&self, id: String, expires_at: DateTime<Utc>) -> Result<Session> {
        let created_at = Utc::now();
        
        let event = SessionEvent::Created {
            id: id.clone(),
            created_at,
            expires_at,
        };
        
        self.engine.apply_event(event).await?;
        
        Ok(Session {
            id,
            data: HashMap::new(),
            created_at,
            expires_at,
            last_accessed_at: created_at,
        })
    }
}

#[async_trait::async_trait]
impl SessionStore for LithairSessionStore {
    async fn get(&self, id: &str) -> Result<Option<Session>> {
        let state = self.engine.read_state().await;
        Ok(state.sessions.get(id).cloned())
    }
    
    async fn set(&self, session: Session) -> Result<()> {
        // Check if session exists
        let state = self.engine.read_state().await;
        let exists = state.sessions.contains_key(&session.id);
        drop(state);
        
        if !exists {
            // Create new session
            let event = SessionEvent::Created {
                id: session.id.clone(),
                created_at: session.created_at,
                expires_at: session.expires_at,
            };
            self.engine.apply_event(event).await?;
        }
        
        // Update all data fields
        for (key, value) in session.data.iter() {
            let event = SessionEvent::DataSet {
                id: session.id.clone(),
                key: key.clone(),
                value: value.clone(),
            };
            self.engine.apply_event(event).await?;
        }
        
        // Update last accessed time
        let event = SessionEvent::Accessed {
            id: session.id.clone(),
            timestamp: session.last_accessed_at,
        };
        self.engine.apply_event(event).await?;
        
        Ok(())
    }
    
    async fn delete(&self, id: &str) -> Result<()> {
        let event = SessionEvent::Deleted {
            id: id.to_string(),
            reason: "manual_delete".to_string(),
        };
        
        self.engine.apply_event(event).await?;
        Ok(())
    }
    
    async fn exists(&self, id: &str) -> Result<bool> {
        let state = self.engine.read_state().await;
        Ok(state.sessions.contains_key(id))
    }
    
    async fn cleanup_expired(&self) -> Result<usize> {
        let state = self.engine.read_state().await;
        
        // Find expired sessions
        let expired: Vec<String> = state
            .sessions
            .iter()
            .filter(|(_, session)| session.is_expired())
            .map(|(id, _)| id.clone())
            .collect();
        
        let count = expired.len();
        drop(state);
        
        if count > 0 {
            let event = SessionEvent::ExpiredCleaned {
                session_ids: expired,
                count,
            };
            
            self.engine.apply_event(event).await?;
        }
        
        Ok(count)
    }
    
    async fn count(&self) -> Result<usize> {
        let state = self.engine.read_state().await;
        Ok(state.sessions.len())
    }
}

/// Session metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetrics {
    /// Number of currently active sessions
    pub active_sessions: usize,
    
    /// Total sessions created since start
    pub total_created: u64,
    
    /// Total sessions deleted since start
    pub total_deleted: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use tempfile::TempDir;
    
    #[tokio::test]
    async fn test_lithair_store_basic() {
        let temp_dir = TempDir::new().unwrap();
        let event_path = temp_dir.path().join("sessions.events");
        
        let store = LithairSessionStore::new(event_path.to_str().unwrap())
            .await
            .unwrap();
        
        // Create session
        let expires_at = Utc::now() + Duration::hours(1);
        let session = store.create_session("test-123".to_string(), expires_at)
            .await
            .unwrap();
        
        // Retrieve session
        let retrieved = store.get("test-123").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, "test-123");
    }
    
    #[tokio::test]
    async fn test_lithair_store_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let event_path = temp_dir.path().join("sessions.events");
        
        // Create store and add session
        {
            let store = LithairSessionStore::new(event_path.to_str().unwrap())
                .await
                .unwrap();
            
            let expires_at = Utc::now() + Duration::hours(1);
            let mut session = store.create_session("persistent".to_string(), expires_at)
                .await
                .unwrap();
            
            session.set("user_id", "alice").unwrap();
            store.set(session).await.unwrap();
        }
        
        // Reload store from same event log
        {
            let store = LithairSessionStore::new(event_path.to_str().unwrap())
                .await
                .unwrap();
            
            let session = store.get("persistent").await.unwrap();
            assert!(session.is_some());
            
            let session = session.unwrap();
            assert_eq!(session.get::<String>("user_id"), Some("alice".to_string()));
        }
    }
    
    #[tokio::test]
    async fn test_lithair_store_cleanup() {
        let temp_dir = TempDir::new().unwrap();
        let event_path = temp_dir.path().join("sessions.events");
        
        let store = LithairSessionStore::new(event_path.to_str().unwrap())
            .await
            .unwrap();
        
        // Create expired session
        let expired = Utc::now() - Duration::seconds(1);
        store.create_session("expired".to_string(), expired).await.unwrap();
        
        // Create valid session
        let valid = Utc::now() + Duration::hours(1);
        store.create_session("valid".to_string(), valid).await.unwrap();
        
        assert_eq!(store.count().await.unwrap(), 2);
        
        // Cleanup
        let removed = store.cleanup_expired().await.unwrap();
        assert_eq!(removed, 1);
        assert_eq!(store.count().await.unwrap(), 1);
        
        // Valid session should still exist
        assert!(store.exists("valid").await.unwrap());
        assert!(!store.exists("expired").await.unwrap());
    }
    
    #[tokio::test]
    async fn test_lithair_store_metrics() {
        let temp_dir = TempDir::new().unwrap();
        let event_path = temp_dir.path().join("sessions.events");
        
        let store = LithairSessionStore::new(event_path.to_str().unwrap())
            .await
            .unwrap();
        
        // Create sessions
        let expires = Utc::now() + Duration::hours(1);
        store.create_session("s1".to_string(), expires).await.unwrap();
        store.create_session("s2".to_string(), expires).await.unwrap();
        
        // Delete one
        store.delete("s1").await.unwrap();
        
        let metrics = store.metrics().await;
        assert_eq!(metrics.active_sessions, 1);
        assert_eq!(metrics.total_created, 2);
        assert_eq!(metrics.total_deleted, 1);
    }
}
