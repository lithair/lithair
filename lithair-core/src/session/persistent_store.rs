//! Persistent session store using Lithair's event sourcing
//!
//! This store provides full event sourcing with .raftlog files,
//! audit trail, and ACID guarantees using Lithair's EventStore.

use super::events::{SessionCreated, SessionData, SessionDeleted, SessionState};
use super::{Session, SessionStore};
use crate::engine::{Event, EventStore, FileStorage};
use anyhow::Result;
use chrono::Utc;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// Persistent session store using Lithair's EventStore
#[derive(Clone)]
pub struct PersistentSessionStore {
    event_store: Arc<std::sync::Mutex<EventStore>>,
    state: Arc<std::sync::RwLock<SessionState>>,
}

impl PersistentSessionStore {
    /// Create a new persistent session store with event sourcing
    pub fn new(data_path: PathBuf) -> Result<Self> {
        // Create data directory if it doesn't exist
        std::fs::create_dir_all(&data_path)?;

        // Create FileStorage for .raftlog files
        let storage = FileStorage::new(
            data_path
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("session data path contains invalid UTF-8"))?,
        )?;

        // Create EventStore
        let event_store = EventStore::with_storage(storage)?;

        // Initialize state
        let state = Arc::new(std::sync::RwLock::new(HashMap::new()));

        // Replay events to rebuild state
        let events = event_store.get_all_events()?;
        let mut state_guard = state.write().expect("session state lock poisoned");

        for event_json in events {
            // Try to parse as SessionCreated, SessionUpdated or SessionDeleted
            if let Ok(event) = serde_json::from_str::<SessionCreated>(&event_json) {
                event.apply(&mut *state_guard);
            } else if let Ok(event) =
                serde_json::from_str::<super::events::SessionUpdated>(&event_json)
            {
                event.apply(&mut *state_guard);
            } else if let Ok(event) = serde_json::from_str::<SessionDeleted>(&event_json) {
                event.apply(&mut *state_guard);
            }
        }

        let count = state_guard.len();
        drop(state_guard);

        log::info!("Loaded {} sessions from event store", count);

        Ok(Self { event_store: Arc::new(std::sync::Mutex::new(event_store)), state })
    }

    /// Apply an event and persist it
    fn apply_event<E: Event<State = SessionState>>(&self, event: E) -> Result<()> {
        // Apply to state
        let mut state = self.state.write().expect("session state lock poisoned");
        event.apply(&mut *state);
        drop(state);

        // Persist event
        let mut store = self.event_store.lock().expect("event store lock poisoned");
        store.append_event(&event)?;

        // CRITICAL: Force flush to create .raftlog files immediately
        store.flush_events()?;

        Ok(())
    }
}

#[async_trait::async_trait]
impl SessionStore for PersistentSessionStore {
    async fn get(&self, session_id: &str) -> Result<Option<Session>> {
        let state = self.state.read().expect("session state lock poisoned");

        match state.get(session_id) {
            Some(data) => Ok(Some(data.clone().into())),
            None => Ok(None),
        }
    }

    async fn set(&self, session: Session) -> Result<()> {
        let session_data = SessionData::from(&session);

        // Check if session exists to determine event type
        let state = self.state.read().expect("session state lock poisoned");
        let exists = state.contains_key(&session.id);
        drop(state);

        if exists {
            // Session exists, use SessionUpdated
            let event = super::events::SessionUpdated {
                event_type: "SessionUpdated.v1".to_string(),
                session_id: session.id.clone(),
                user_id: Some(session_data.user_id.clone()),
                role: Some(session_data.role.clone()),
                expires_at: Some(session.expires_at),
                data: Some(session.data.clone()),
            };
            self.apply_event(event)?;
        } else {
            // New session, use SessionCreated
            let event = SessionCreated {
                event_type: "SessionCreated.v1".to_string(),
                session_id: session.id.clone(),
                user_id: Some(session_data.user_id.clone()),
                role: Some(session_data.role.clone()),
                expires_at: Some(session.expires_at),
                data: Some(session.data.clone()),
            };
            self.apply_event(event)?;
        }

        Ok(())
    }

    async fn delete(&self, session_id: &str) -> Result<()> {
        let event = SessionDeleted {
            event_type: "SessionDeleted.v1".to_string(),
            session_id: session_id.to_string(),
            user_id: None,
            role: None,
            expires_at: None,
            data: None,
        };

        self.apply_event(event)?;

        Ok(())
    }

    async fn cleanup_expired(&self) -> Result<usize> {
        let now = Utc::now();
        let mut removed = 0;

        let expired_ids: Vec<String> = {
            let state = self.state.read().expect("session state lock poisoned");
            state
                .iter()
                .filter(|(_, data)| data.expires_at <= now)
                .map(|(id, _)| id.clone())
                .collect()
        };

        for session_id in expired_ids {
            let event = SessionDeleted {
                event_type: "SessionDeleted.v1".to_string(),
                session_id,
                user_id: None,
                role: None,
                expires_at: None,
                data: None,
            };
            self.apply_event(event)?;
            removed += 1;
        }

        if removed > 0 {
            log::info!("Cleaned up {} expired sessions", removed);
        }

        Ok(removed)
    }

    async fn count(&self) -> Result<usize> {
        let state = self.state.read().expect("session state lock poisoned");
        Ok(state.len())
    }
}
