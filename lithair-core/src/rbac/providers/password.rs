//! Simple password-based authentication provider
//!
//! This provider authenticates users based on HTTP headers:
//! - `X-Auth-Password`: The password to check
//! - `X-Auth-Role`: The role to assign (optional, uses default if not provided)
//!
//! Similar to Apache Basic Auth but simpler and stateless.

use crate::rbac::context::AuthContext;
use crate::rbac::traits::AuthProvider;
use anyhow::{anyhow, Result};
use bytes::Bytes;
use http::Request;
use http_body_util::Full;

/// Simple password authentication provider
#[derive(Debug, Clone)]
pub struct PasswordProvider {
    /// The password to check against
    password: String,

    /// Default role for authenticated users
    default_role: String,
}

impl PasswordProvider {
    /// Create a new password provider
    pub fn new(password: String, default_role: String) -> Self {
        Self { password, default_role }
    }
}

impl AuthProvider for PasswordProvider {
    fn authenticate(&self, request: &Request<Full<Bytes>>) -> Result<AuthContext> {
        // Extract password from header
        let provided_password =
            request.headers().get("X-Auth-Password").and_then(|h| h.to_str().ok());

        // Extract requested role from header (optional)
        let requested_role = request.headers().get("X-Auth-Role").and_then(|h| h.to_str().ok());

        // Check if password matches
        if let Some(pwd) = provided_password {
            if pwd == self.password {
                // Authenticated!
                let role = requested_role.unwrap_or(&self.default_role).to_string();

                return Ok(AuthContext {
                    user_id: Some("authenticated_user".to_string()),
                    roles: vec![role],
                    groups: vec![],
                    authenticated: true,
                    provider: "password".to_string(),
                    mfa_verified: false,
                    metadata: std::collections::HashMap::new(),
                });
            }

            // Wrong password
            return Err(anyhow!("Invalid password"));
        }

        // No password provided - return unauthenticated context
        Ok(AuthContext::unauthenticated())
    }

    fn name(&self) -> &str {
        "password"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use http::Request;
    use http_body_util::Full;

    #[test]
    fn test_password_auth_success() {
        let provider = PasswordProvider::new("secret123".to_string(), "User".to_string());

        let request = Request::builder()
            .header("X-Auth-Password", "secret123")
            .header("X-Auth-Role", "Admin")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let context = provider.authenticate(&request).unwrap();

        assert!(context.authenticated);
        assert_eq!(context.roles, vec!["Admin"]);
        assert_eq!(context.provider, "password");
    }

    #[test]
    fn test_password_auth_default_role() {
        let provider = PasswordProvider::new("secret123".to_string(), "User".to_string());

        let request = Request::builder()
            .header("X-Auth-Password", "secret123")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let context = provider.authenticate(&request).unwrap();

        assert!(context.authenticated);
        assert_eq!(context.roles, vec!["User"]);
    }

    #[test]
    fn test_password_auth_failure() {
        let provider = PasswordProvider::new("secret123".to_string(), "User".to_string());

        let request = Request::builder()
            .header("X-Auth-Password", "wrong_password")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let result = provider.authenticate(&request);

        assert!(result.is_err());
    }

    #[test]
    fn test_password_auth_no_header() {
        let provider = PasswordProvider::new("secret123".to_string(), "User".to_string());

        let request = Request::builder().body(Full::new(Bytes::new())).unwrap();

        let context = provider.authenticate(&request).unwrap();

        assert!(!context.authenticated);
        assert_eq!(context.roles, vec!["Public"]);
    }
}
