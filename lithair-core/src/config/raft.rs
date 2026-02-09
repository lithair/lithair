//! Raft cluster configuration
//!
//! Configures the Raft consensus endpoints with optional authentication.

use serde::{Deserialize, Serialize};
use std::env;

/// Configuration for Raft cluster endpoints
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaftConfig {
    /// Enable Raft endpoints
    pub enabled: bool,
    /// Base path for Raft endpoints (default: "/raft")
    /// Endpoints will be: {path}/leader, {path}/heartbeat, {path}/election
    pub path: String,
    /// Require authentication for Raft endpoints
    pub auth_required: bool,
    /// Secret token for Raft endpoint authentication
    /// If set, all Raft requests must include header: `X-Raft-Token: <token>`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<String>,
    /// Heartbeat interval in seconds (leader sends heartbeats to followers)
    pub heartbeat_interval_secs: u64,
    /// Election timeout in seconds (followers start election if no heartbeat)
    pub election_timeout_secs: u64,
}

impl Default for RaftConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            path: "/raft".to_string(),
            auth_required: false,
            auth_token: None,
            heartbeat_interval_secs: 2,
            election_timeout_secs: 5,
        }
    }
}

impl RaftConfig {
    /// Create a new RaftConfig with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the base path for Raft endpoints
    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }

    /// Enable authentication with the given token
    pub fn with_auth(mut self, token: impl Into<String>) -> Self {
        self.auth_required = true;
        self.auth_token = Some(token.into());
        self
    }

    /// Set heartbeat interval
    pub fn with_heartbeat_interval(mut self, secs: u64) -> Self {
        self.heartbeat_interval_secs = secs;
        self
    }

    /// Set election timeout
    pub fn with_election_timeout(mut self, secs: u64) -> Self {
        self.election_timeout_secs = secs;
        self
    }

    /// Apply environment variables
    pub fn apply_env_vars(&mut self) {
        if let Ok(enabled) = env::var("LITHAIR_RAFT_ENABLED") {
            self.enabled = enabled.parse().unwrap_or(true);
        }

        if let Ok(path) = env::var("LITHAIR_RAFT_PATH") {
            self.path = path;
        }

        if let Ok(token) = env::var("LITHAIR_RAFT_TOKEN") {
            if !token.is_empty() {
                self.auth_required = true;
                self.auth_token = Some(token);
            }
        }

        if let Ok(interval) = env::var("LITHAIR_RAFT_HEARTBEAT_INTERVAL") {
            if let Ok(secs) = interval.parse() {
                self.heartbeat_interval_secs = secs;
            }
        }

        if let Ok(timeout) = env::var("LITHAIR_RAFT_ELECTION_TIMEOUT") {
            if let Ok(secs) = timeout.parse() {
                self.election_timeout_secs = secs;
            }
        }
    }

    /// Get the full path for the leader endpoint
    pub fn leader_path(&self) -> String {
        format!("{}/leader", self.path)
    }

    /// Get the full path for the heartbeat endpoint
    pub fn heartbeat_path(&self) -> String {
        format!("{}/heartbeat", self.path)
    }

    /// Get the full path for the election endpoint
    pub fn election_path(&self) -> String {
        format!("{}/election", self.path)
    }

    /// Check if a request path matches any Raft endpoint
    pub fn matches_path(&self, uri: &str) -> bool {
        uri.starts_with(&self.path)
    }

    /// Validate authentication token from request header
    ///
    /// Uses constant-time comparison to prevent timing attacks.
    pub fn validate_token(&self, token: Option<&str>) -> bool {
        if !self.auth_required {
            return true;
        }

        match (&self.auth_token, token) {
            (Some(expected), Some(provided)) => {
                // HMAC-based constant-time comparison to prevent timing attacks
                // (including length-based side channels).
                // By computing HMAC(key, provided) vs HMAC(key, expected),
                // the comparison is constant-time regardless of input lengths.
                use hmac::{Hmac, Mac};
                use sha2::Sha256;
                type HmacSha256 = Hmac<Sha256>;

                let key = expected.as_bytes();
                let mut mac_provided =
                    HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
                mac_provided.update(provided.as_bytes());
                let hash_provided = mac_provided.finalize().into_bytes();

                let mut mac_expected =
                    HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
                mac_expected.update(expected.as_bytes());
                let hash_expected = mac_expected.finalize().into_bytes();

                // Constant-time comparison of fixed-size hashes
                hash_provided
                    .as_slice()
                    .iter()
                    .zip(hash_expected.as_slice())
                    .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                    == 0
            }
            (None, _) => true,
            (Some(_), None) => false,
        }
    }

    /// Merge another config into this one (other takes priority)
    pub fn merge(&mut self, other: Self) {
        *self = other;
    }

    /// Validate the configuration
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.auth_required && self.auth_token.is_none() {
            anyhow::bail!("Raft auth_required is true but no auth_token is set");
        }
        if self.heartbeat_interval_secs >= self.election_timeout_secs {
            anyhow::bail!("Raft heartbeat_interval_secs must be less than election_timeout_secs");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = RaftConfig::default();
        assert_eq!(config.path, "/raft");
        assert!(!config.auth_required);
        assert!(config.auth_token.is_none());
    }

    #[test]
    fn test_with_auth() {
        let config = RaftConfig::new().with_path("/_internal/raft").with_auth("secret-token-123");

        assert_eq!(config.path, "/_internal/raft");
        assert!(config.auth_required);
        assert_eq!(config.auth_token, Some("secret-token-123".to_string()));
    }

    #[test]
    fn test_path_helpers() {
        let config = RaftConfig::new().with_path("/cluster");
        assert_eq!(config.leader_path(), "/cluster/leader");
        assert_eq!(config.heartbeat_path(), "/cluster/heartbeat");
        assert_eq!(config.election_path(), "/cluster/election");
    }

    #[test]
    fn test_token_validation() {
        let config = RaftConfig::new().with_auth("my-secret");

        assert!(!config.validate_token(None));
        assert!(!config.validate_token(Some("wrong-token")));
        assert!(config.validate_token(Some("my-secret")));
    }

    #[test]
    fn test_no_auth_required() {
        let config = RaftConfig::new();
        assert!(config.validate_token(None));
        assert!(config.validate_token(Some("anything")));
    }

    #[test]
    fn test_validate_valid_config() {
        let config = RaftConfig::new()
            .with_auth("my-secret")
            .with_heartbeat_interval(2)
            .with_election_timeout(5);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_auth_required_without_token() {
        let mut config = RaftConfig::new();
        config.auth_required = true;
        config.auth_token = None;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_heartbeat_ge_election() {
        let config = RaftConfig::new().with_heartbeat_interval(5).with_election_timeout(3);
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_matches_path() {
        let config = RaftConfig::new().with_path("/cluster/raft");
        assert!(config.matches_path("/cluster/raft/leader"));
        assert!(config.matches_path("/cluster/raft/heartbeat"));
        assert!(!config.matches_path("/other/path"));
    }

    #[test]
    fn test_merge() {
        let mut config = RaftConfig::new().with_path("/old");
        let other = RaftConfig::new().with_path("/new").with_auth("token");
        config.merge(other);
        assert_eq!(config.path, "/new");
        assert!(config.auth_required);
    }
}
