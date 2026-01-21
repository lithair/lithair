//! Authentication providers

pub mod google;
pub mod password;

pub use google::GoogleProvider;
pub use password::PasswordProvider;

use crate::rbac::traits::AuthProvider;

/// Provider configuration
#[derive(Debug, Clone)]
pub enum ProviderConfig {
    /// No authentication
    None,

    /// Simple password authentication
    Password { password: String, default_role: String },

    /// Google OAuth2 authentication
    Google { client_id: String, client_secret: String, redirect_uri: String, default_role: String },

    /// Future: OAuth providers
    #[allow(dead_code)]
    OAuth { provider: String, client_id: String, client_secret: String },

    /// Future: LDAP
    #[allow(dead_code)]
    Ldap { server: String, base_dn: String },
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self::None
    }
}

impl ProviderConfig {
    /// Create a provider from configuration
    pub fn create_provider(&self) -> Option<Box<dyn AuthProvider>> {
        match self {
            ProviderConfig::None => None,
            ProviderConfig::Password { password, default_role } => {
                Some(Box::new(PasswordProvider::new(password.clone(), default_role.clone())))
            }
            ProviderConfig::Google { client_id, client_secret, redirect_uri, default_role } => {
                Some(Box::new(GoogleProvider::new(
                    client_id.clone(),
                    client_secret.clone(),
                    redirect_uri.clone(),
                    default_role.clone(),
                )))
            }
            _ => None, // Future providers
        }
    }
}
