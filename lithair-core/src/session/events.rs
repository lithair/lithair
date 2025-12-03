//! Session events for event sourcing

use super::Session;
use crate::engine::{Event, EventDeserializer};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Session state for event sourcing
pub type SessionState = HashMap<String, SessionData>;

/// Session data stored in state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    pub id: String,
    pub user_id: String,
    pub role: String,
    pub expires_at: DateTime<Utc>,
    pub data: HashMap<String, serde_json::Value>,
}

/// Unified session event structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCreated {
    pub event_type: String,
    pub session_id: String,
    pub user_id: Option<String>,
    pub role: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub data: Option<HashMap<String, serde_json::Value>>,
}

impl Event for SessionCreated {
    type State = SessionState;

    fn apply(&self, state: &mut Self::State) {
        state.insert(
            self.session_id.clone(),
            SessionData {
                id: self.session_id.clone(),
                user_id: self.user_id.clone().unwrap_or_default(),
                role: self.role.clone().unwrap_or_default(),
                expires_at: self.expires_at.unwrap_or_else(|| chrono::Utc::now()),
                data: self.data.clone().unwrap_or_default(),
            },
        );
    }

    fn idempotence_key(&self) -> Option<String> {
        Some(format!("session_created_{}", self.session_id))
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.session_id.clone())
    }

    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    fn from_json(json: &str) -> crate::engine::EngineResult<Self>
    where
        Self: Sized,
    {
        serde_json::from_str(json).map_err(|e| {
            crate::engine::EngineError::SerializationError(format!(
                "Failed to deserialize SessionCreated: {}",
                e
            ))
        })
    }
}

/// Event: Session deleted
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDeleted {
    pub event_type: String,
    pub session_id: String,
    pub user_id: Option<String>,
    pub role: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub data: Option<HashMap<String, serde_json::Value>>,
}

impl Event for SessionDeleted {
    type State = SessionState;

    fn apply(&self, state: &mut Self::State) {
        state.remove(&self.session_id);
    }

    fn idempotence_key(&self) -> Option<String> {
        Some(format!("session_deleted_{}", self.session_id))
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.session_id.clone())
    }

    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    fn from_json(json: &str) -> crate::engine::EngineResult<Self>
    where
        Self: Sized,
    {
        serde_json::from_str(json).map_err(|e| {
            crate::engine::EngineError::SerializationError(format!(
                "Failed to deserialize SessionDeleted: {}",
                e
            ))
        })
    }
}

/// Event: Session updated
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionUpdated {
    pub event_type: String,
    pub session_id: String,
    pub user_id: Option<String>,
    pub role: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub data: Option<HashMap<String, serde_json::Value>>,
}

impl Event for SessionUpdated {
    type State = SessionState;

    fn apply(&self, state: &mut Self::State) {
        if let Some(session) = state.get_mut(&self.session_id) {
            if let Some(data) = &self.data {
                session.data = data.clone();
            }
        }
    }

    fn idempotence_key(&self) -> Option<String> {
        Some(format!(
            "session_updated_{}_{}",
            self.session_id,
            chrono::Utc::now().timestamp_millis()
        ))
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.session_id.clone())
    }

    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    fn from_json(json: &str) -> crate::engine::EngineResult<Self>
    where
        Self: Sized,
    {
        serde_json::from_str(json).map_err(|e| {
            crate::engine::EngineError::SerializationError(format!(
                "Failed to deserialize SessionUpdated: {}",
                e
            ))
        })
    }
}

/// Convert Session to SessionData
impl From<&Session> for SessionData {
    fn from(session: &Session) -> Self {
        Self {
            id: session.id.clone(),
            user_id: session.get::<String>("user_id").unwrap_or_default(),
            role: session.get::<String>("role").unwrap_or_default(),
            expires_at: session.expires_at,
            data: session.data.clone(),
        }
    }
}

/// Convert SessionData to Session
impl From<SessionData> for Session {
    fn from(data: SessionData) -> Self {
        let mut session = Session::new(data.id, data.expires_at);
        session.data = data.data;
        session
    }
}

#[derive(Default)]
#[allow(dead_code)]
pub struct SessionCreatedDeserializer;

impl EventDeserializer for SessionCreatedDeserializer {
    type State = SessionState;

    fn event_type(&self) -> &str {
        std::any::type_name::<SessionCreated>()
    }

    fn apply_from_json(&self, state: &mut Self::State, payload_json: &str) -> Result<(), String> {
        let mut event: SessionCreated = serde_json::from_str(payload_json)
            .map_err(|e| format!("Failed to deserialize SessionCreated payload: {}", e))?;

        // Exemple d'upcasting interne : normaliser les anciens event_type non versionnés
        if event.event_type.is_empty() || event.event_type == "SessionCreated" {
            event.event_type = "SessionCreated.v1".to_string();
        }

        event.apply(state);
        Ok(())
    }
}

#[derive(Default)]
#[allow(dead_code)]
pub struct SessionUpdatedDeserializer;

impl EventDeserializer for SessionUpdatedDeserializer {
    type State = SessionState;

    fn event_type(&self) -> &str {
        std::any::type_name::<SessionUpdated>()
    }

    fn apply_from_json(&self, state: &mut Self::State, payload_json: &str) -> Result<(), String> {
        let mut event: SessionUpdated = serde_json::from_str(payload_json)
            .map_err(|e| format!("Failed to deserialize SessionUpdated payload: {}", e))?;

        if event.event_type.is_empty() || event.event_type == "SessionUpdated" {
            event.event_type = "SessionUpdated.v1".to_string();
        }

        event.apply(state);
        Ok(())
    }
}

#[derive(Default)]
#[allow(dead_code)]
pub struct SessionDeletedDeserializer;

impl EventDeserializer for SessionDeletedDeserializer {
    type State = SessionState;

    fn event_type(&self) -> &str {
        std::any::type_name::<SessionDeleted>()
    }

    fn apply_from_json(&self, state: &mut Self::State, payload_json: &str) -> Result<(), String> {
        let mut event: SessionDeleted = serde_json::from_str(payload_json)
            .map_err(|e| format!("Failed to deserialize SessionDeleted payload: {}", e))?;

        if event.event_type.is_empty() || event.event_type == "SessionDeleted" {
            event.event_type = "SessionDeleted.v1".to_string();
        }

        event.apply(state);
        Ok(())
    }
}
