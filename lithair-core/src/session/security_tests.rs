//! Security tests for session management
//!
//! These tests validate critical security properties of the session system.

#[cfg(test)]
mod tests {
    use super::super::*;
    use chrono::Duration;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_expired_session_rejected() {
        let store = Arc::new(MemorySessionStore::new());

        // Create expired session
        let expires_at = chrono::Utc::now() - Duration::seconds(1);
        let session = Session::new("expired-123".to_string(), expires_at);
        store.set(session).await.unwrap();

        // Try to retrieve - should be None after cleanup
        let retrieved = store.get("expired-123").await.unwrap();
        assert!(retrieved.is_some()); // Still there before cleanup

        // Cleanup should remove it
        let removed = store.cleanup_expired().await.unwrap();
        assert_eq!(removed, 1);

        // Now it should be gone
        let retrieved = store.get("expired-123").await.unwrap();
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_session_isolation() {
        let store = Arc::new(MemorySessionStore::new());

        // Create two sessions
        let expires_at = chrono::Utc::now() + Duration::hours(1);

        let mut session1 = Session::new("user1-session".to_string(), expires_at);
        session1.set("user_id", "alice").unwrap();
        session1.set("secret", "alice-secret").unwrap();

        let mut session2 = Session::new("user2-session".to_string(), expires_at);
        session2.set("user_id", "bob").unwrap();
        session2.set("secret", "bob-secret").unwrap();

        store.set(session1).await.unwrap();
        store.set(session2).await.unwrap();

        // Verify isolation - session1 cannot access session2 data
        let retrieved1 = store.get("user1-session").await.unwrap().unwrap();
        assert_eq!(retrieved1.get::<String>("user_id"), Some("alice".to_string()));
        assert_eq!(retrieved1.get::<String>("secret"), Some("alice-secret".to_string()));

        let retrieved2 = store.get("user2-session").await.unwrap().unwrap();
        assert_eq!(retrieved2.get::<String>("user_id"), Some("bob".to_string()));
        assert_eq!(retrieved2.get::<String>("secret"), Some("bob-secret".to_string()));

        // Cross-contamination check
        assert_ne!(retrieved1.get::<String>("secret"), retrieved2.get::<String>("secret"));
    }

    #[tokio::test]
    async fn test_concurrent_session_access() {
        let store = Arc::new(MemorySessionStore::new());

        // Create session
        let expires_at = chrono::Utc::now() + Duration::hours(1);
        let mut session = Session::new("concurrent-test".to_string(), expires_at);
        session.set("counter", 0).unwrap();
        store.set(session).await.unwrap();

        // Spawn multiple concurrent readers
        let mut handles = vec![];
        for _ in 0..10 {
            let store_clone = store.clone();
            let handle = tokio::spawn(async move {
                for _ in 0..100 {
                    let session = store_clone.get("concurrent-test").await.unwrap();
                    assert!(session.is_some());
                }
            });
            handles.push(handle);
        }

        // Wait for all to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Session should still be valid
        let final_session = store.get("concurrent-test").await.unwrap();
        assert!(final_session.is_some());
    }

    #[tokio::test]
    async fn test_session_deletion_is_immediate() {
        let store = Arc::new(MemorySessionStore::new());

        // Create session
        let expires_at = chrono::Utc::now() + Duration::hours(1);
        let session = Session::new("delete-test".to_string(), expires_at);
        store.set(session).await.unwrap();

        // Verify it exists
        assert!(store.exists("delete-test").await.unwrap());

        // Delete it
        store.delete("delete-test").await.unwrap();

        // Should be immediately gone
        assert!(!store.exists("delete-test").await.unwrap());

        // Get should return None
        let retrieved = store.get("delete-test").await.unwrap();
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_session_touch_updates_last_accessed() {
        let expires_at = chrono::Utc::now() + Duration::hours(1);
        let mut session = Session::new("touch-test".to_string(), expires_at);

        let initial_access = session.last_accessed_at;

        // Wait a bit
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Touch the session
        session.touch();

        // Last accessed should be updated
        assert!(session.last_accessed_at > initial_access);
    }

    #[tokio::test]
    async fn test_session_data_type_safety() {
        let expires_at = chrono::Utc::now() + Duration::hours(1);
        let mut session = Session::new("type-test".to_string(), expires_at);

        // Set different types
        session.set("string", "hello").unwrap();
        session.set("number", 42).unwrap();
        session.set("bool", true).unwrap();
        session.set("vec", vec![1, 2, 3]).unwrap();

        // Retrieve with correct types
        assert_eq!(session.get::<String>("string"), Some("hello".to_string()));
        assert_eq!(session.get::<i32>("number"), Some(42));
        assert_eq!(session.get::<bool>("bool"), Some(true));
        assert_eq!(session.get::<Vec<i32>>("vec"), Some(vec![1, 2, 3]));

        // Wrong type should return None
        assert_eq!(session.get::<i32>("string"), None);
        assert_eq!(session.get::<String>("number"), None);
    }

    #[tokio::test]
    async fn test_cleanup_only_removes_expired() {
        let store = Arc::new(MemorySessionStore::new());

        // Create mix of expired and valid sessions
        let expired = chrono::Utc::now() - Duration::seconds(1);
        let valid = chrono::Utc::now() + Duration::hours(1);

        store.set(Session::new("expired1".to_string(), expired)).await.unwrap();
        store.set(Session::new("expired2".to_string(), expired)).await.unwrap();
        store.set(Session::new("valid1".to_string(), valid)).await.unwrap();
        store.set(Session::new("valid2".to_string(), valid)).await.unwrap();

        assert_eq!(store.count().await.unwrap(), 4);

        // Cleanup
        let removed = store.cleanup_expired().await.unwrap();
        assert_eq!(removed, 2);

        // Only valid sessions remain
        assert_eq!(store.count().await.unwrap(), 2);
        assert!(store.exists("valid1").await.unwrap());
        assert!(store.exists("valid2").await.unwrap());
        assert!(!store.exists("expired1").await.unwrap());
        assert!(!store.exists("expired2").await.unwrap());
    }
}
