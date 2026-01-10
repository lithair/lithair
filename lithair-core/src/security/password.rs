//! Secure Password Hashing for Lithair
//!
//! Uses Argon2id (OWASP recommended) for password hashing.
//! This module provides cryptographically secure password storage
//! that is resistant to GPU/ASIC attacks.

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};

/// Password hasher using Argon2id algorithm
///
/// Argon2id is the recommended choice for password hashing:
/// - Memory-hard (resistant to GPU attacks)
/// - Resistant to side-channel attacks
/// - Winner of the Password Hashing Competition
pub struct PasswordHasherService {
    /// Argon2 instance with configured parameters
    argon2: Argon2<'static>,
}

impl Default for PasswordHasherService {
    fn default() -> Self {
        Self::new()
    }
}

impl PasswordHasherService {
    /// Create a new password hasher with default Argon2id parameters
    ///
    /// Default parameters (OWASP recommended for 2024):
    /// - Algorithm: Argon2id
    /// - Memory: 19 MiB (19456 KiB)
    /// - Iterations: 2
    /// - Parallelism: 1
    pub fn new() -> Self {
        Self {
            argon2: Argon2::default(),
        }
    }

    /// Hash a password using Argon2id
    ///
    /// Returns the PHC string format hash (includes algorithm, salt, and hash)
    /// Example: $argon2id$v=19$m=19456,t=2,p=1$salt$hash
    ///
    /// # Example
    /// ```ignore
    /// let hasher = PasswordHasherService::new();
    /// let hash = hasher.hash_password("my_password").unwrap();
    /// assert!(hash.starts_with("$argon2id$"));
    /// ```
    pub fn hash_password(&self, password: &str) -> Result<String, PasswordError> {
        let salt = SaltString::generate(&mut OsRng);

        let password_hash = self
            .argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| PasswordError::HashingFailed(e.to_string()))?;

        Ok(password_hash.to_string())
    }

    /// Verify a password against a stored hash
    ///
    /// Returns true if the password matches the hash, false otherwise.
    /// Uses constant-time comparison to prevent timing attacks.
    ///
    /// # Example
    /// ```ignore
    /// let hasher = PasswordHasherService::new();
    /// let hash = hasher.hash_password("my_password").unwrap();
    /// assert!(hasher.verify_password("my_password", &hash).unwrap());
    /// assert!(!hasher.verify_password("wrong_password", &hash).unwrap());
    /// ```
    pub fn verify_password(&self, password: &str, hash: &str) -> Result<bool, PasswordError> {
        let parsed_hash = PasswordHash::new(hash)
            .map_err(|e| PasswordError::InvalidHash(e.to_string()))?;

        match self.argon2.verify_password(password.as_bytes(), &parsed_hash) {
            Ok(()) => Ok(true),
            Err(argon2::password_hash::Error::Password) => Ok(false),
            Err(e) => Err(PasswordError::VerificationFailed(e.to_string())),
        }
    }

    /// Check if a hash needs rehashing (e.g., if parameters have changed)
    ///
    /// This is useful for upgrading password hashes when security parameters are increased.
    pub fn needs_rehash(&self, hash: &str) -> bool {
        // Check if the hash uses current Argon2id parameters
        // For now, always return false (no rehash needed)
        // In future, compare against current params
        !hash.starts_with("$argon2id$")
    }
}

/// Password-related errors
#[derive(Debug, Clone)]
pub enum PasswordError {
    /// Failed to hash password
    HashingFailed(String),
    /// Invalid hash format
    InvalidHash(String),
    /// Failed to verify password
    VerificationFailed(String),
}

impl std::fmt::Display for PasswordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PasswordError::HashingFailed(msg) => write!(f, "Password hashing failed: {}", msg),
            PasswordError::InvalidHash(msg) => write!(f, "Invalid password hash: {}", msg),
            PasswordError::VerificationFailed(msg) => write!(f, "Password verification failed: {}", msg),
        }
    }
}

impl std::error::Error for PasswordError {}

/// Global password hasher instance for convenience
///
/// Use this for simple cases. For custom parameters, create a new PasswordHasherService.
pub fn hash_password(password: &str) -> Result<String, PasswordError> {
    PasswordHasherService::new().hash_password(password)
}

/// Global password verification for convenience
pub fn verify_password(password: &str, hash: &str) -> Result<bool, PasswordError> {
    PasswordHasherService::new().verify_password(password, hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_password_hashing() {
        let hasher = PasswordHasherService::new();
        let password = "my_secure_password_123!";

        let hash = hasher.hash_password(password).unwrap();

        // Hash should be in PHC format
        assert!(hash.starts_with("$argon2id$"));

        // Verify correct password
        assert!(hasher.verify_password(password, &hash).unwrap());

        // Reject wrong password
        assert!(!hasher.verify_password("wrong_password", &hash).unwrap());
    }

    #[test]
    fn test_different_passwords_different_hashes() {
        let hasher = PasswordHasherService::new();

        let hash1 = hasher.hash_password("password1").unwrap();
        let hash2 = hasher.hash_password("password1").unwrap();

        // Same password should produce different hashes (due to random salt)
        assert_ne!(hash1, hash2);

        // But both should verify correctly
        assert!(hasher.verify_password("password1", &hash1).unwrap());
        assert!(hasher.verify_password("password1", &hash2).unwrap());
    }

    #[test]
    fn test_global_functions() {
        let password = "test_password";
        let hash = hash_password(password).unwrap();

        assert!(verify_password(password, &hash).unwrap());
        assert!(!verify_password("wrong", &hash).unwrap());
    }

    #[test]
    fn test_needs_rehash() {
        let hasher = PasswordHasherService::new();

        let hash = hasher.hash_password("test").unwrap();
        assert!(!hasher.needs_rehash(&hash));

        // Old bcrypt-style hash would need rehash
        assert!(hasher.needs_rehash("$2b$12$somebcrypthash"));
    }
}
