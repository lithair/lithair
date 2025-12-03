use crate::engine::Event;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// --- Test Application Definition ---

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
