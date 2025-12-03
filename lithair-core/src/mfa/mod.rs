//! Multi-Factor Authentication (MFA) support for Lithair
//!
//! Provides TOTP (Time-based One-Time Password) authentication
//! compatible with Google Authenticator, Authy, 1Password, etc.
//!
//! # Features
//! - TOTP generation and validation (RFC 6238)
//! - QR code generation for easy setup
//! - Secure secret storage
//! - Role-based MFA enforcement
//!
//! # Example
//! ```ignore
//! use lithair_core::mfa::MfaConfig;
//!
//! LithairServer::new()
//!     .with_rbac_config(rbac_config)
//!     .with_mfa_totp(MfaConfig {
//!         issuer: "My App".to_string(),
//!         enforce_for_roles: vec!["Admin".to_string()],
//!         optional_for_roles: vec!["Editor".to_string()],
//!         ..Default::default()
//!     })
//!     .serve()
//!     .await?;
//! ```

mod totp;
mod qrcode_gen;
mod storage;
pub mod handlers;
pub mod events;
pub mod event_log;
pub mod migration;

pub use totp::{TotpSecret, TotpValidator};
pub use qrcode_gen::generate_qr_code;
pub use storage::{MfaStorage, UserMfaData};
pub use events::{MfaEvent, MfaState, UserMfaState};
pub use event_log::MfaEventLog;
pub use migration::{migrate_json_to_events, MigrationStats};

use serde::{Deserialize, Serialize};

/// Configuration for MFA/TOTP support
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MfaConfig {
    /// Issuer name displayed in authenticator apps (e.g., "Lithair Blog")
    pub issuer: String,
    
    /// Roles that MUST use MFA (enforced at login)
    #[serde(default)]
    pub enforce_for_roles: Vec<String>,
    
    /// Roles that CAN enable MFA (optional)
    #[serde(default)]
    pub optional_for_roles: Vec<String>,
    
    /// TOTP algorithm (default: SHA256, secure and widely supported)
    #[serde(default = "default_algorithm")]
    pub algorithm: TotpAlgorithm,
    
    /// Number of digits in TOTP code (default: 6)
    #[serde(default = "default_digits")]
    pub digits: u32,
    
    /// Time step in seconds (default: 30)
    #[serde(default = "default_step")]
    pub step: u64,
    
    /// Storage path for MFA secrets (default: "./mfa_secrets")
    #[serde(default = "default_storage_path")]
    pub storage_path: String,
}

impl Default for MfaConfig {
    fn default() -> Self {
        Self {
            issuer: "Lithair".to_string(),
            enforce_for_roles: Vec::new(),
            optional_for_roles: Vec::new(),
            algorithm: default_algorithm(),
            digits: default_digits(),
            step: default_step(),
            storage_path: default_storage_path(),
        }
    }
}

/// TOTP algorithm
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TotpAlgorithm {
    /// SHA1 (most compatible, default)
    SHA1,
    /// SHA256
    SHA256,
    /// SHA512
    SHA512,
}

impl Default for TotpAlgorithm {
    fn default() -> Self {
        Self::SHA1
    }
}

fn default_algorithm() -> TotpAlgorithm {
    TotpAlgorithm::SHA256  // SHA256 is more secure than SHA1 and widely supported
}

fn default_digits() -> u32 {
    6
}

fn default_step() -> u64 {
    30
}

fn default_storage_path() -> String {
    "./mfa_secrets".to_string()
}

/// MFA status for a user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MfaStatus {
    /// Whether MFA is enabled for this user
    pub enabled: bool,
    
    /// Whether MFA is required for this user's role
    pub required: bool,
    
    /// When MFA was enabled (if enabled)
    pub enabled_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl Default for MfaStatus {
    fn default() -> Self {
        Self {
            enabled: false,
            required: false,
            enabled_at: None,
        }
    }
}
