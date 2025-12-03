//! RBAC middleware for HTTP requests

use crate::rbac::context::AuthContext;
use crate::rbac::traits::AuthProvider;
use anyhow::Result;
use bytes::Bytes;
use http::Request;
use http_body_util::Full;
use std::sync::Arc;

/// RBAC middleware
#[derive(Clone)]
pub struct RbacMiddleware {
    /// Authentication provider
    provider: Arc<dyn AuthProvider>,
}

impl RbacMiddleware {
    /// Create a new RBAC middleware
    pub fn new(provider: Arc<dyn AuthProvider>) -> Self {
        Self { provider }
    }
    
    /// Authenticate a request
    pub fn authenticate(&self, request: &Request<Full<Bytes>>) -> Result<AuthContext> {
        self.provider.authenticate(request)
    }
    
    /// Get provider name
    pub fn provider_name(&self) -> &str {
        self.provider.name()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rbac::providers::PasswordProvider;
    use http::Request;
    use http_body_util::Full;
    use bytes::Bytes;
    
    #[test]
    fn test_middleware_authentication() {
        let provider = Arc::new(PasswordProvider::new(
            "test123".to_string(),
            "User".to_string(),
        ));
        
        let middleware = RbacMiddleware::new(provider);
        
        let request = Request::builder()
            .header("X-Auth-Password", "test123")
            .header("X-Auth-Role", "Admin")
            .body(Full::new(Bytes::new()))
            .unwrap();
        
        let context = middleware.authenticate(&request).unwrap();
        
        assert!(context.authenticated);
        assert_eq!(context.roles, vec!["Admin"]);
        assert_eq!(middleware.provider_name(), "password");
    }
}
