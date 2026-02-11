//! Session manager with automatic cleanup and lifecycle management
//!
//! The SessionManager wraps a SessionStore and provides automatic cleanup
//! of expired sessions, along with other lifecycle management features.

use super::{Session, SessionStore};
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;

/// Session manager configuration
#[derive(Debug, Clone)]
pub struct SessionManagerConfig {
    /// Enable automatic cleanup of expired sessions
    pub auto_cleanup: bool,

    /// Interval between cleanup runs
    pub cleanup_interval: Duration,

    /// Log cleanup operations
    pub log_cleanup: bool,
}

impl Default for SessionManagerConfig {
    fn default() -> Self {
        Self {
            auto_cleanup: true,
            cleanup_interval: Duration::from_secs(300), // 5 minutes
            log_cleanup: true,
        }
    }
}

impl SessionManagerConfig {
    /// Create a new configuration with defaults
    pub fn new() -> Self {
        Self::default()
    }

    /// Set auto cleanup enabled/disabled
    pub fn with_auto_cleanup(mut self, enabled: bool) -> Self {
        self.auto_cleanup = enabled;
        self
    }

    /// Set cleanup interval
    pub fn with_cleanup_interval(mut self, interval: Duration) -> Self {
        self.cleanup_interval = interval;
        self
    }

    /// Set whether to log cleanup operations
    pub fn with_log_cleanup(mut self, enabled: bool) -> Self {
        self.log_cleanup = enabled;
        self
    }
}

/// Session manager that handles session lifecycle
///
/// Wraps a SessionStore and provides automatic cleanup of expired sessions.
///
/// # Example
///
/// ```no_run
/// use lithair_core::session::{SessionManager, MemorySessionStore};
///
/// # async fn example() -> anyhow::Result<()> {
/// // Create manager with automatic cleanup
/// let manager = SessionManager::new(MemorySessionStore::new());
///
/// // Use the store
/// let store = manager.store();
/// // Cleanup happens automatically in the background
/// # Ok(())
/// # }
/// ```
pub struct SessionManager<S: SessionStore> {
    store: Arc<S>,
    cleanup_task: Option<tokio::task::JoinHandle<()>>,
    config: SessionManagerConfig,
}

impl<S: SessionStore + 'static> SessionManager<S> {
    /// Create a new session manager with default configuration
    pub fn new(store: S) -> Self {
        Self::with_config(store, SessionManagerConfig::default())
    }

    /// Create a new session manager with custom configuration
    pub fn with_config(store: S, config: SessionManagerConfig) -> Self {
        let store = Arc::new(store);

        let cleanup_task = if config.auto_cleanup {
            let cleanup_store = store.clone();
            let interval = config.cleanup_interval;
            let log_cleanup = config.log_cleanup;

            Some(tokio::spawn(async move {
                let mut interval_timer = tokio::time::interval(interval);
                loop {
                    interval_timer.tick().await;

                    match cleanup_store.cleanup_expired().await {
                        Ok(count) if count > 0 => {
                            if log_cleanup {
                                log::info!("Auto-cleaned {} expired sessions", count);
                            }
                        }
                        Ok(_) => {
                            // No sessions to clean, silent
                        }
                        Err(e) => {
                            log::error!("Session cleanup failed: {}", e);
                        }
                    }
                }
            }))
        } else {
            None
        };

        Self { store, cleanup_task, config }
    }

    /// Get a reference to the underlying session store
    pub fn store(&self) -> Arc<S> {
        Arc::clone(&self.store)
    }

    /// Get the manager configuration
    pub fn config(&self) -> &SessionManagerConfig {
        &self.config
    }

    /// Manually trigger a cleanup (in addition to automatic cleanup)
    pub async fn cleanup_now(&self) -> Result<usize> {
        self.store.cleanup_expired().await
    }

    /// Get session count
    pub async fn session_count(&self) -> Result<usize> {
        self.store.count().await
    }

    /// Check if a session exists
    pub async fn has_session(&self, id: &str) -> Result<bool> {
        self.store.exists(id).await
    }

    /// Get a session by ID
    pub async fn get_session(&self, id: &str) -> Result<Option<Session>> {
        self.store.get(id).await
    }

    /// Store a session
    pub async fn set_session(&self, session: Session) -> Result<()> {
        self.store.set(session).await
    }

    /// Delete a session
    pub async fn delete_session(&self, id: &str) -> Result<()> {
        self.store.delete(id).await
    }
}

impl<S: SessionStore> Drop for SessionManager<S> {
    fn drop(&mut self) {
        // Abort the cleanup task when the manager is dropped
        if let Some(task) = self.cleanup_task.take() {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::MemorySessionStore;
    use chrono::Duration as ChronoDuration;

    #[tokio::test]
    async fn test_manager_creation() {
        let manager = SessionManager::new(MemorySessionStore::new());
        assert!(manager.config().auto_cleanup);
        assert_eq!(manager.config().cleanup_interval, Duration::from_secs(300));
    }

    #[tokio::test]
    async fn test_manager_with_custom_config() {
        let config = SessionManagerConfig::new()
            .with_auto_cleanup(false)
            .with_cleanup_interval(Duration::from_secs(60));

        let manager = SessionManager::with_config(MemorySessionStore::new(), config);
        assert!(!manager.config().auto_cleanup);
        assert_eq!(manager.config().cleanup_interval, Duration::from_secs(60));
    }

    #[tokio::test]
    async fn test_manager_store_operations() {
        let manager = SessionManager::new(MemorySessionStore::new());

        // Create session
        let expires_at = chrono::Utc::now() + ChronoDuration::hours(1);
        let mut session = Session::new("test-123".to_string(), expires_at);
        session.set("user_id", "alice").unwrap();

        // Store it
        manager.set_session(session.clone()).await.unwrap();

        // Retrieve it
        let retrieved = manager.get_session("test-123").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().get::<String>("user_id"), Some("alice".to_string()));

        // Check existence
        assert!(manager.has_session("test-123").await.unwrap());

        // Count
        assert_eq!(manager.session_count().await.unwrap(), 1);

        // Delete
        manager.delete_session("test-123").await.unwrap();
        assert!(!manager.has_session("test-123").await.unwrap());
    }

    #[tokio::test]
    async fn test_manual_cleanup() {
        let manager = SessionManager::new(MemorySessionStore::new());

        // Create expired session
        let expired = chrono::Utc::now() - ChronoDuration::seconds(1);
        let session = Session::new("expired".to_string(), expired);
        manager.set_session(session).await.unwrap();

        // Create valid session
        let valid = chrono::Utc::now() + ChronoDuration::hours(1);
        let session = Session::new("valid".to_string(), valid);
        manager.set_session(session).await.unwrap();

        assert_eq!(manager.session_count().await.unwrap(), 2);

        // Manual cleanup
        let removed = manager.cleanup_now().await.unwrap();
        assert_eq!(removed, 1);
        assert_eq!(manager.session_count().await.unwrap(), 1);

        // Only valid session remains
        assert!(manager.has_session("valid").await.unwrap());
        assert!(!manager.has_session("expired").await.unwrap());
    }

    #[tokio::test]
    async fn test_auto_cleanup_disabled() {
        let config = SessionManagerConfig::new().with_auto_cleanup(false);
        let manager = SessionManager::with_config(MemorySessionStore::new(), config);

        // Create expired session
        let expired = chrono::Utc::now() - ChronoDuration::seconds(1);
        let session = Session::new("expired".to_string(), expired);
        manager.set_session(session).await.unwrap();

        // Wait a bit (no auto cleanup should happen)
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Session should still be there (no auto cleanup)
        assert_eq!(manager.session_count().await.unwrap(), 1);

        // Manual cleanup still works
        let removed = manager.cleanup_now().await.unwrap();
        assert_eq!(removed, 1);
    }
}
