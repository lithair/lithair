//! MFA Events for Event Sourcing
//!
//! All MFA operations are recorded as events for audit trail and replay capability

use super::{MfaStatus, TotpSecret};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// MFA Event - represents a change in MFA state
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MfaEvent {
    /// User initiated MFA setup (secret generated but not yet enabled)
    MfaSetupInitiated { username: String, secret: TotpSecret, timestamp: DateTime<Utc> },

    /// User enabled MFA (verified first code successfully)
    MfaEnabled { username: String, timestamp: DateTime<Utc> },

    /// User disabled MFA
    MfaDisabled {
        username: String,
        reason: Option<String>, // e.g., "user_request", "admin_reset"
        timestamp: DateTime<Utc>,
    },

    /// TOTP code verified successfully during login
    MfaCodeVerified { username: String, timestamp: DateTime<Utc> },

    /// TOTP code verification failed
    MfaCodeVerificationFailed {
        username: String,
        reason: String, // e.g., "invalid_code", "expired"
        timestamp: DateTime<Utc>,
    },

    /// Backup codes generated
    BackupCodesGenerated { username: String, codes_count: usize, timestamp: DateTime<Utc> },

    /// Backup code used
    BackupCodeUsed { username: String, timestamp: DateTime<Utc> },
}

impl MfaEvent {
    /// Get the username associated with this event
    pub fn username(&self) -> &str {
        match self {
            MfaEvent::MfaSetupInitiated { username, .. } => username,
            MfaEvent::MfaEnabled { username, .. } => username,
            MfaEvent::MfaDisabled { username, .. } => username,
            MfaEvent::MfaCodeVerified { username, .. } => username,
            MfaEvent::MfaCodeVerificationFailed { username, .. } => username,
            MfaEvent::BackupCodesGenerated { username, .. } => username,
            MfaEvent::BackupCodeUsed { username, .. } => username,
        }
    }

    /// Get the timestamp of this event
    pub fn timestamp(&self) -> DateTime<Utc> {
        match self {
            MfaEvent::MfaSetupInitiated { timestamp, .. } => *timestamp,
            MfaEvent::MfaEnabled { timestamp, .. } => *timestamp,
            MfaEvent::MfaDisabled { timestamp, .. } => *timestamp,
            MfaEvent::MfaCodeVerified { timestamp, .. } => *timestamp,
            MfaEvent::MfaCodeVerificationFailed { timestamp, .. } => *timestamp,
            MfaEvent::BackupCodesGenerated { timestamp, .. } => *timestamp,
            MfaEvent::BackupCodeUsed { timestamp, .. } => *timestamp,
        }
    }

    /// Get event type as string (for logging/audit)
    pub fn event_type(&self) -> &'static str {
        match self {
            MfaEvent::MfaSetupInitiated { .. } => "mfa_setup_initiated",
            MfaEvent::MfaEnabled { .. } => "mfa_enabled",
            MfaEvent::MfaDisabled { .. } => "mfa_disabled",
            MfaEvent::MfaCodeVerified { .. } => "mfa_code_verified",
            MfaEvent::MfaCodeVerificationFailed { .. } => "mfa_code_verification_failed",
            MfaEvent::BackupCodesGenerated { .. } => "backup_codes_generated",
            MfaEvent::BackupCodeUsed { .. } => "backup_code_used",
        }
    }
}

/// MFA State - reconstructed from events
#[derive(Debug, Clone, Default)]
pub struct MfaState {
    /// Username → MFA data
    pub users: std::collections::HashMap<String, UserMfaState>,
}

/// Per-user MFA state
#[derive(Debug, Clone)]
pub struct UserMfaState {
    pub secret: Option<TotpSecret>,
    pub status: MfaStatus,
    pub backup_codes: Vec<String>,
    pub last_verification: Option<DateTime<Utc>>,
    pub failed_attempts: usize,
}

impl MfaState {
    /// Apply an event to the state (event sourcing replay)
    pub fn apply(&mut self, event: &MfaEvent) {
        let username = event.username().to_string();

        match event {
            MfaEvent::MfaSetupInitiated { secret, .. } => {
                let user_state = self.users.entry(username).or_insert_with(|| UserMfaState {
                    secret: None,
                    status: MfaStatus::default(),
                    backup_codes: Vec::new(),
                    last_verification: None,
                    failed_attempts: 0,
                });

                user_state.secret = Some(secret.clone());
            }

            MfaEvent::MfaEnabled { timestamp, .. } => {
                if let Some(user_state) = self.users.get_mut(&username) {
                    user_state.status.enabled = true;
                    user_state.status.enabled_at = Some(*timestamp);
                }
            }

            MfaEvent::MfaDisabled { .. } => {
                if let Some(user_state) = self.users.get_mut(&username) {
                    user_state.status.enabled = false;
                    user_state.status.enabled_at = None;
                    user_state.secret = None; // Clear secret on disable
                    user_state.backup_codes.clear();
                }
            }

            MfaEvent::MfaCodeVerified { timestamp, .. } => {
                if let Some(user_state) = self.users.get_mut(&username) {
                    user_state.last_verification = Some(*timestamp);
                    user_state.failed_attempts = 0; // Reset failed attempts on success
                }
            }

            MfaEvent::MfaCodeVerificationFailed { .. } => {
                // Create user entry if doesn't exist (user may have attempted MFA without setup)
                let user_state = self.users.entry(username).or_insert_with(|| UserMfaState {
                    secret: None,
                    status: MfaStatus::default(),
                    backup_codes: Vec::new(),
                    last_verification: None,
                    failed_attempts: 0,
                });
                user_state.failed_attempts += 1;
            }

            MfaEvent::BackupCodesGenerated { .. } => {
                // Backup codes management (to be implemented)
            }

            MfaEvent::BackupCodeUsed { .. } => {
                // Backup code usage tracking
            }
        }
    }

    /// Replay multiple events to reconstruct state
    pub fn replay(events: &[MfaEvent]) -> Self {
        let mut state = Self::default();
        for event in events {
            state.apply(event);
        }
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mfa::TotpAlgorithm;

    #[test]
    fn test_event_sourcing_replay() {
        let now = Utc::now();

        let secret = crate::mfa::TotpSecret::generate_with_account(
            TotpAlgorithm::SHA256,
            6,
            30,
            "Test",
            "user",
        )
        .unwrap();

        let events = vec![
            MfaEvent::MfaSetupInitiated {
                username: "alice".to_string(),
                secret: secret.clone(),
                timestamp: now,
            },
            MfaEvent::MfaEnabled { username: "alice".to_string(), timestamp: now },
            MfaEvent::MfaCodeVerified { username: "alice".to_string(), timestamp: now },
        ];

        let state = MfaState::replay(&events);

        assert!(state.users.contains_key("alice"));
        let alice_state = &state.users["alice"];
        assert!(alice_state.status.enabled);
        assert!(alice_state.secret.is_some());
        assert_eq!(alice_state.failed_attempts, 0);
    }

    #[test]
    fn test_failed_attempts_tracking() {
        let now = Utc::now();

        let events = vec![
            MfaEvent::MfaCodeVerificationFailed {
                username: "bob".to_string(),
                reason: "invalid_code".to_string(),
                timestamp: now,
            },
            MfaEvent::MfaCodeVerificationFailed {
                username: "bob".to_string(),
                reason: "invalid_code".to_string(),
                timestamp: now,
            },
        ];

        let state = MfaState::replay(&events);

        assert_eq!(state.users["bob"].failed_attempts, 2);
    }
}
