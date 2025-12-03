//! TOTP (Time-based One-Time Password) wrapper around totp-rs
//!
//! Provides Lithair-specific abstractions over the battle-tested totp-rs library

use super::TotpAlgorithm;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
pub use totp_rs::{Algorithm, Secret, TOTP};

/// TOTP secret for a user (Lithair wrapper)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TotpSecret {
    /// Internal totp-rs TOTP instance
    #[serde(skip)]
    totp: Option<TOTP>,
    
    /// Serialized secret for persistence
    pub secret: String,
    
    /// Algorithm used
    pub algorithm: TotpAlgorithm,
    
    /// Number of digits
    pub digits: u32,
    
    /// Time step in seconds
    pub step: u64,
    
    /// Issuer name (for QR code)
    #[serde(default)]
    pub issuer: Option<String>,
    
    /// Account name/username (for QR code)
    #[serde(default)]
    pub account_name: Option<String>,
}

impl TotpSecret {
    /// Generate a new random TOTP secret with issuer and account
    pub fn generate_with_account(
        algorithm: TotpAlgorithm,
        digits: u32,
        step: u64,
        issuer: &str,
        account_name: &str,
    ) -> Self {
        // Convert our algorithm to totp-rs algorithm
        let algo = match algorithm {
            TotpAlgorithm::SHA1 => Algorithm::SHA1,
            TotpAlgorithm::SHA256 => Algorithm::SHA256,
            TotpAlgorithm::SHA512 => Algorithm::SHA512,
        };
        
        // Generate secret using totp-rs
        let secret = Secret::generate_secret();
        let secret_str = secret.to_encoded().to_string();
        
        let totp = TOTP::new(
            algo,
            digits as usize,
            1, // skew (time drift tolerance)
            step,
            secret.to_bytes().unwrap(),
            Some(issuer.to_string()),
            account_name.to_string(),
        ).unwrap();
        
        Self {
            totp: Some(totp),
            secret: secret_str,
            algorithm,
            digits,
            step,
            issuer: Some(issuer.to_string()),
            account_name: Some(account_name.to_string()),
        }
    }
    
    /// Generate without account info (for backwards compat / tests)
    pub fn generate(algorithm: TotpAlgorithm, digits: u32, step: u64) -> Self {
        Self::generate_with_account(algorithm, digits, step, "Lithair", "user")
    }
    
    /// Create from existing secret (for deserialization)
    pub fn from_secret(
        secret: String,
        algorithm: TotpAlgorithm,
        digits: u32,
        step: u64,
        issuer: Option<String>,
        account_name: Option<String>,
    ) -> Result<Self> {
        let algo = match algorithm {
            TotpAlgorithm::SHA1 => Algorithm::SHA1,
            TotpAlgorithm::SHA256 => Algorithm::SHA256,
            TotpAlgorithm::SHA512 => Algorithm::SHA512,
        };
        
        let secret_bytes = Secret::Encoded(secret.clone())
            .to_bytes()
            .map_err(|e| anyhow!("Invalid secret: {}", e))?;
        
        let totp = TOTP::new(
            algo,
            digits as usize,
            1,
            step,
            secret_bytes,
            issuer.clone(),
            account_name.clone().unwrap_or_else(|| "user".to_string()),
        ).map_err(|e| anyhow!("Failed to create TOTP: {}", e))?;
        
        Ok(Self {
            totp: Some(totp),
            secret,
            algorithm,
            digits,
            step,
            issuer,
            account_name,
        })
    }
    
    /// Get or create TOTP instance
    fn get_totp(&self) -> Result<TOTP> {
        if let Some(ref totp) = self.totp {
            Ok(totp.clone())
        } else {
            // Reconstruct from serialized data
            let algo = match self.algorithm {
                TotpAlgorithm::SHA1 => Algorithm::SHA1,
                TotpAlgorithm::SHA256 => Algorithm::SHA256,
                TotpAlgorithm::SHA512 => Algorithm::SHA512,
            };
            
            let secret_bytes = Secret::Encoded(self.secret.clone())
                .to_bytes()
                .map_err(|e| anyhow!("Invalid secret: {}", e))?;
            
            TOTP::new(
                algo,
                self.digits as usize,
                1,
                self.step,
                secret_bytes,
                self.issuer.clone(),
                self.account_name.clone().unwrap_or_else(|| "user".to_string()),
            ).map_err(|e| anyhow!("Failed to create TOTP: {}", e))
        }
    }
    
    /// Generate TOTP URI for QR code
    pub fn to_uri(&self) -> Result<String> {
        let totp = self.get_totp()?;
        Ok(totp.get_url())
    }
    
    /// Get current TOTP code
    pub fn current_code(&self) -> Result<String> {
        let totp = self.get_totp()?;
        totp.generate_current()
            .map_err(|e| anyhow!("Failed to generate code: {}", e))
    }
    
    /// Get QR code for setup (uses totp-rs qr feature)
    pub fn get_qr_code(&self) -> Result<String> {
        let totp = self.get_totp()?;
        totp.get_qr_base64()
            .map_err(|e| anyhow!("Failed to generate QR code: {}", e))
    }
}

/// TOTP validator
pub struct TotpValidator;

impl TotpValidator {
    /// Validate a TOTP code
    /// 
    /// Uses totp-rs built-in validation with time drift tolerance
    pub fn validate(secret: &TotpSecret, code: &str) -> Result<bool> {
        let totp = secret.get_totp()?;
        Ok(totp.check_current(code)
            .map_err(|e| anyhow!("Validation error: {}", e))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_totp_generation() {
        let secret = TotpSecret::generate(TotpAlgorithm::SHA1, 6, 30);
        
        // Should generate a 6-digit code
        let code = secret.current_code().unwrap();
        assert_eq!(code.len(), 6);
        assert!(code.chars().all(|c| c.is_numeric()));
    }
    
    #[test]
    fn test_totp_validation() {
        let secret = TotpSecret::generate(TotpAlgorithm::SHA1, 6, 30);
        let code = secret.current_code().unwrap();
        
        // Should validate current code
        assert!(TotpValidator::validate(&secret, &code).unwrap());
        
        // Should reject invalid code
        assert!(!TotpValidator::validate(&secret, "000000").unwrap());
    }
    
    #[test]
    fn test_totp_uri() {
        let secret = TotpSecret::generate_with_account(TotpAlgorithm::SHA1, 6, 30, "Lithair", "admin");
        let uri = secret.to_uri().unwrap();
        
        assert!(uri.starts_with("otpauth://totp/"));
        assert!(uri.contains("Lithair"));
        assert!(uri.contains("admin"));
    }
    
    #[test]
    fn test_serialization() {
        let secret = TotpSecret::generate(TotpAlgorithm::SHA1, 6, 30);
        let code1 = secret.current_code().unwrap();
        
        // Serialize and deserialize
        let json = serde_json::to_string(&secret).unwrap();
        let restored: TotpSecret = serde_json::from_str(&json).unwrap();
        
        // Should still work after deserialization
        let code2 = restored.current_code().unwrap();
        
        // Codes should match (within same time window)
        assert_eq!(code1, code2);
    }
}
