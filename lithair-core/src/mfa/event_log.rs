//! MFA Event Log - Persistent storage for MFA events

use super::events::{MfaEvent, MfaState};
use anyhow::{anyhow, Result};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Persistent event log for MFA
pub struct MfaEventLog {
    /// Path to the event log file
    log_path: PathBuf,

    /// In-memory state (reconstructed from events)
    state: Arc<RwLock<MfaState>>,

    /// Event counter for idempotency
    event_count: Arc<RwLock<usize>>,
}

impl MfaEventLog {
    /// Create new event log
    pub fn new(log_path: impl Into<PathBuf>) -> Result<Self> {
        let log_path = log_path.into();

        // Create parent directory if needed
        if let Some(parent) = log_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Load existing events
        let events = Self::load_events(&log_path)?;
        let state = MfaState::replay(&events);
        let event_count = events.len();

        log::info!("Loaded {} MFA events from log", event_count);

        Ok(Self {
            log_path,
            state: Arc::new(RwLock::new(state)),
            event_count: Arc::new(RwLock::new(event_count)),
        })
    }

    /// Load events from disk
    fn load_events(log_path: &PathBuf) -> Result<Vec<MfaEvent>> {
        if !log_path.exists() {
            return Ok(Vec::new());
        }

        let file = fs::File::open(log_path)?;
        let reader = BufReader::new(file);

        let mut events = Vec::new();
        for (line_num, line) in reader.lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            match serde_json::from_str::<MfaEvent>(&line) {
                Ok(event) => events.push(event),
                Err(e) => {
                    log::warn!("Failed to parse MFA event at line {}: {}", line_num + 1, e);
                }
            }
        }

        Ok(events)
    }

    /// Append event to log and update state
    pub async fn append(&self, event: MfaEvent) -> Result<()> {
        // Log event
        log::info!("MFA Event: {} for {}", event.event_type(), event.username());

        // Serialize event
        let json = serde_json::to_string(&event)
            .map_err(|e| anyhow!("Failed to serialize event: {}", e))?;

        // Append to file
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
            .map_err(|e| anyhow!("Failed to open event log: {}", e))?;

        writeln!(file, "{}", json).map_err(|e| anyhow!("Failed to write event: {}", e))?;

        // Update in-memory state
        {
            let mut state = self.state.write().await;
            state.apply(&event);
        }

        // Increment counter
        {
            let mut count = self.event_count.write().await;
            *count += 1;
        }

        Ok(())
    }

    /// Get current state (read-only)
    pub async fn state(&self) -> MfaState {
        let state = self.state.read().await;
        state.clone()
    }

    /// Get user MFA state
    pub async fn get_user_state(&self, username: &str) -> Option<super::events::UserMfaState> {
        let state = self.state.read().await;
        state.users.get(username).cloned()
    }

    /// Check if MFA is enabled for user
    pub async fn is_enabled(&self, username: &str) -> bool {
        let state = self.state.read().await;
        state.users.get(username).map(|u| u.status.enabled).unwrap_or(false)
    }

    /// Get event count
    pub async fn event_count(&self) -> usize {
        let count = self.event_count.read().await;
        *count
    }

    /// Get all events (for debugging/audit)
    pub fn all_events(&self) -> Result<Vec<MfaEvent>> {
        Self::load_events(&self.log_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mfa::{TotpAlgorithm, TotpSecret};
    use chrono::Utc;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_event_log_persistence() {
        let temp_file = NamedTempFile::new().unwrap();
        let log_path = temp_file.path().to_path_buf();

        let log = MfaEventLog::new(&log_path).unwrap();

        let secret =
            TotpSecret::generate_with_account(TotpAlgorithm::SHA256, 6, 30, "Test", "alice")
                .unwrap();

        // Append events
        log.append(MfaEvent::MfaSetupInitiated {
            username: "alice".to_string(),
            secret,
            timestamp: Utc::now(),
        })
        .await
        .unwrap();

        log.append(MfaEvent::MfaEnabled { username: "alice".to_string(), timestamp: Utc::now() })
            .await
            .unwrap();

        assert_eq!(log.event_count().await, 2);
        assert!(log.is_enabled("alice").await);

        // Reload from disk
        let log2 = MfaEventLog::new(&log_path).unwrap();
        assert_eq!(log2.event_count().await, 2);
        assert!(log2.is_enabled("alice").await);
    }
}
