//! Secure storage for MFA secrets (Event-Sourced)

use super::event_log::MfaEventLog;
use super::events::MfaEvent;
use super::{MfaStatus, TotpSecret};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

/// User MFA data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMfaData {
    /// TOTP secret
    pub secret: TotpSecret,

    /// MFA status
    pub status: MfaStatus,

    /// Backup codes (optional, for account recovery)
    #[serde(default)]
    pub backup_codes: Vec<String>,
}

/// Thread-safe MFA storage (Event-Sourced)
pub struct MfaStorage {
    /// Event log for persistence
    event_log: Arc<MfaEventLog>,
}

impl MfaStorage {
    /// Create new MFA storage with event sourcing
    pub fn new(storage_path: impl Into<PathBuf>) -> Result<Self> {
        let storage_path = storage_path.into();

        // Create event log file path
        let log_file = storage_path.join("mfa_events.log");

        let event_log = MfaEventLog::new(log_file)?;

        Ok(Self { event_log: Arc::new(event_log) })
    }

    /// Get user MFA data (reconstructed from events)
    pub async fn get(&self, username: &str) -> Result<Option<UserMfaData>> {
        if let Some(user_state) = self.event_log.get_user_state(username).await {
            if let Some(secret) = user_state.secret {
                return Ok(Some(UserMfaData {
                    secret,
                    status: user_state.status,
                    backup_codes: user_state.backup_codes,
                }));
            }
        }
        Ok(None)
    }

    /// Save user MFA data by emitting MfaSetupInitiated event
    ///
    /// This ONLY emits the setup event with the generated secret.
    /// To enable MFA, call `enable()` separately after verifying the first code.
    pub async fn save(&self, username: &str, data: UserMfaData) -> Result<()> {
        // Emit MfaSetupInitiated event (secret generated but not yet enabled)
        let event = MfaEvent::MfaSetupInitiated {
            username: username.to_string(),
            secret: data.secret,
            timestamp: chrono::Utc::now(),
        };

        self.event_log.append(event).await
    }

    /// Enable MFA for user by emitting MfaEnabled event
    pub async fn enable(&self, username: &str) -> Result<()> {
        let event =
            MfaEvent::MfaEnabled { username: username.to_string(), timestamp: chrono::Utc::now() };
        self.event_log.append(event).await
    }

    /// Delete user MFA data by emitting MfaDisabled event
    pub async fn delete(&self, username: &str) -> Result<()> {
        let event = MfaEvent::MfaDisabled {
            username: username.to_string(),
            reason: Some("user_request".to_string()),
            timestamp: chrono::Utc::now(),
        };
        self.event_log.append(event).await
    }

    /// Check if user has MFA enabled
    pub async fn is_enabled(&self, username: &str) -> bool {
        self.event_log.is_enabled(username).await
    }

    /// Record successful code verification
    pub async fn record_verification_success(&self, username: &str) -> Result<()> {
        let event = MfaEvent::MfaCodeVerified {
            username: username.to_string(),
            timestamp: chrono::Utc::now(),
        };
        self.event_log.append(event).await
    }

    /// Record failed code verification
    pub async fn record_verification_failure(&self, username: &str, reason: &str) -> Result<()> {
        let event = MfaEvent::MfaCodeVerificationFailed {
            username: username.to_string(),
            reason: reason.to_string(),
            timestamp: chrono::Utc::now(),
        };
        self.event_log.append(event).await
    }

    /// Get event count (for audit/monitoring)
    pub async fn event_count(&self) -> usize {
        self.event_log.event_count().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mfa::TotpAlgorithm;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_mfa_storage_event_sourced() {
        let temp_dir = TempDir::new().unwrap();
        let storage = MfaStorage::new(temp_dir.path()).unwrap();

        let secret = TotpSecret::generate(TotpAlgorithm::SHA256, 6, 30).unwrap();
        let data = UserMfaData {
            secret: secret.clone(),
            status: MfaStatus {
                enabled: false, // Setup but not yet enabled
                required: false,
                enabled_at: None,
            },
            backup_codes: vec![],
        };

        // Save (emit MfaSetupInitiated event)
        storage.save("testuser", data.clone()).await.unwrap();

        // Not enabled yet
        assert!(!storage.is_enabled("testuser").await);

        // Enable MFA
        storage.enable("testuser").await.unwrap();

        // Now enabled
        assert!(storage.is_enabled("testuser").await);

        // Retrieve
        let retrieved = storage.get("testuser").await.unwrap().unwrap();
        assert_eq!(retrieved.secret.secret, secret.secret);
        assert!(retrieved.status.enabled);

        // Record verification
        storage.record_verification_success("testuser").await.unwrap();

        // Delete (emit MfaDisabled event)
        storage.delete("testuser").await.unwrap();
        assert!(!storage.is_enabled("testuser").await);

        // Check event count
        assert!(storage.event_count().await >= 4); // Setup, Enable, Verify, Disable
    }
}
