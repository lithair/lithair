//! Test utilities for Lithair applications
//!
//! Provides [`TestHandler`] for integration-testing DeclarativeModel types
//! without starting a real HTTP server.
//!
//! # Example
//!
//! ```rust,ignore
//! use lithair_core::testing::TestHandler;
//!
//! #[tokio::test]
//! async fn test_crud() {
//!     let h = TestHandler::<Product>::new().await.unwrap();
//!
//!     // Create (runs validation + lifecycle)
//!     let product = Product { id: "p1".into(), name: "Widget".into(), price: 9.99 };
//!     h.create(product.clone()).await.unwrap();
//!
//!     // Get by ID
//!     assert!(h.get("p1").await.is_some());
//!
//!     // List all
//!     assert_eq!(h.list().await.len(), 1);
//!
//!     // Count
//!     assert_eq!(h.count().await, 1);
//!
//!     // Delete
//!     h.delete("p1").await.unwrap();
//!     assert_eq!(h.count().await, 0);
//! }
//! ```

use crate::consensus::ReplicatedModel;
use crate::http::declarative::DeclarativeHttpHandler;
use crate::http::HttpExposable;
use crate::lifecycle::LifecycleAware;
use serde::{de::DeserializeOwned, Serialize};

/// Test wrapper around [`DeclarativeHttpHandler`] with automatic temp directory management.
///
/// Creates a temporary event store directory that is cleaned up when dropped.
/// Provides typed methods that go through validation and lifecycle checks,
/// without needing to construct HTTP requests.
pub struct TestHandler<T>
where
    T: HttpExposable + LifecycleAware + ReplicatedModel,
{
    handler: DeclarativeHttpHandler<T>,
    _temp_dir: tempfile::TempDir,
}

impl<T> TestHandler<T>
where
    T: HttpExposable + LifecycleAware + ReplicatedModel + Serialize + DeserializeOwned,
{
    /// Create a new test handler with a temporary event store directory.
    pub async fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let temp_dir = tempfile::TempDir::new()?;
        let event_store_path =
            format!("{}/{}.events", temp_dir.path().display(), T::http_base_path());
        let handler = DeclarativeHttpHandler::<T>::new(&event_store_path)?;
        Ok(Self { handler, _temp_dir: temp_dir })
    }

    /// Create an item. Runs validation and lifecycle checks before inserting.
    ///
    /// Returns the item on success, or a validation/lifecycle error message.
    pub async fn create(&self, mut item: T) -> Result<T, String> {
        item.validate()?;
        item.apply_lifecycle()?;
        self.handler.apply_replicated_item(item.clone()).await.map(|_| item)
    }

    /// Get a single item by ID, or None if not found.
    pub async fn get(&self, id: &str) -> Option<T> {
        self.handler.get_by_id(id).await
    }

    /// List all items in storage.
    pub async fn list(&self) -> Vec<T> {
        self.handler.get_all_items().await
    }

    /// Return items matching a predicate.
    pub async fn query<F>(&self, predicate: F) -> Vec<T>
    where
        F: Fn(&T) -> bool,
    {
        self.handler.query(predicate).await
    }

    /// Count items in storage.
    pub async fn count(&self) -> usize {
        self.handler.storage_count().await
    }

    /// Delete an item by ID. Returns Ok(true) if found, Ok(false) if not found.
    pub async fn delete(&self, id: &str) -> Result<bool, String> {
        self.handler.apply_replicated_delete(id).await
    }

    /// Get a reference to the underlying handler for advanced usage.
    pub fn inner(&self) -> &DeclarativeHttpHandler<T> {
        &self.handler
    }
}

// --- Legacy Test Models (kept for backward compatibility) ---

use crate::engine::Event;
use serde::Deserialize;
use std::collections::HashMap;

/// Hello World message structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloMessage {
    pub id: String,
    pub text: String,
    pub timestamp: u64,
}

/// Events for the Hello World application
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HelloEvent {
    MessageStored { id: String, text: String, timestamp: u64 },
}

impl Event for HelloEvent {
    type State = HelloWorldState;

    fn apply(&self, state: &mut Self::State) {
        match self {
            HelloEvent::MessageStored { id, text, timestamp } => {
                let message =
                    HelloMessage { id: id.clone(), text: text.clone(), timestamp: *timestamp };
                state.messages.insert(id.clone(), message);
                state.message_count += 1;
            }
        }
    }
}

/// Application state for Hello World
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct HelloWorldState {
    pub messages: HashMap<String, HelloMessage>,
    pub message_count: u64,
}
