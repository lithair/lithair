//! Authentication providers

pub mod password;

pub use password::PasswordProvider;

use crate::rbac::traits::AuthProvider;

/// Provider configuration
#[derive(Debug, Clone, Default)]
pub enum ProviderConfig {
    /// No authentication
    #[default]
    None,

    /// Simple password authentication
    Password { password: String, default_role: String },
}

impl ProviderConfig {
    /// Create a provider from configuration
    pub fn create_provider(&self) -> Option<Box<dyn AuthProvider>> {
        match self {
            ProviderConfig::None => None,
            ProviderConfig::Password { password, default_role } => {
                Some(Box::new(PasswordProvider::new(password.clone(), default_role.clone())))
            }
        }
    }
}
