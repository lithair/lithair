//! RBAC Middleware for Lithair HTTP Server
//!
//! This middleware provides automatic authentication and authorization for all HTTP requests.
//! It integrates seamlessly with the Lithair HTTP server and event sourcing system.
//!
//! ## Security Features
//! - **JWT**: HMAC-SHA256 signatures (cryptographically secure)
//! - **Sessions**: UUID v4 (cryptographically random)
//! - **Passwords**: Argon2id via password module

use super::password::verify_password as argon2_verify;
use super::{AuthContext, Permission, SecurityError, SecurityEvent, SecurityState, Session};
use crate::http::HttpRequest;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::marker::PhantomData;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// Type alias for HMAC-SHA256
type HmacSha256 = Hmac<Sha256>;

/// JWT claims structure
#[derive(Debug, Clone)]
pub struct JwtClaims {
    pub user_id: u32,
    pub session_id: String,
    pub exp: u64, // Expiration timestamp
    pub iat: u64, // Issued at timestamp
}

/// RBAC Middleware that enforces authentication and authorization
///
/// Generic over Permission type to allow applications to define their own permission systems
#[derive(Debug)]
pub struct RBACMiddleware<P: Permission> {
    security_state: Arc<RwLock<SecurityState<P>>>,
    jwt_secret: String,
    session_timeout: u64, // Session timeout in seconds
    _phantom: PhantomData<P>,
}

impl<P: Permission> RBACMiddleware<P> {
    /// Create new RBAC middleware
    pub fn new(security_state: Arc<RwLock<SecurityState<P>>>, jwt_secret: String) -> Self {
        Self {
            security_state,
            jwt_secret,
            session_timeout: 24 * 60 * 60, // 24 hours default
            _phantom: PhantomData,
        }
    }

    /// Authenticate directly from a JWT token string and return an AuthContext
    pub fn authenticate_token(&self, token: &str) -> Result<AuthContext<P>, SecurityError> {
        // 1. Validate and decode JWT
        let claims = self.validate_jwt(token)?;

        // 2. Verify session is active
        let _session = self.validate_session(&claims.session_id)?;

        // 3. Get user permissions
        let permissions = self.get_user_permissions(claims.user_id)?;

        // 4. Get user team/org info
        let (team_id, organization_id) = self.get_user_context(claims.user_id)?;

        Ok(AuthContext {
            user_id: claims.user_id,
            session_id: claims.session_id,
            permissions,
            team_id,
            organization_id,
        })
    }

    /// Set session timeout
    pub fn with_session_timeout(mut self, timeout_seconds: u64) -> Self {
        self.session_timeout = timeout_seconds;
        self
    }

    /// Authenticate a request and return auth context
    pub fn authenticate_request(
        &self,
        request: &HttpRequest,
    ) -> Result<AuthContext<P>, SecurityError> {
        // 1. Extract JWT token from Authorization header
        let token = self.extract_jwt_token(request)?;

        // 2. Validate and decode JWT
        let claims = self.validate_jwt(&token)?;

        // 3. Verify session is active
        let _session = self.validate_session(&claims.session_id)?;

        // 4. Get user permissions
        let permissions = self.get_user_permissions(claims.user_id)?;

        // 5. Get user team/org info
        let (team_id, organization_id) = self.get_user_context(claims.user_id)?;

        Ok(AuthContext {
            user_id: claims.user_id,
            session_id: claims.session_id,
            permissions,
            team_id,
            organization_id,
        })
    }

    /// Authorize an action with optional object-level checks
    pub fn authorize_action(
        &self,
        auth: &AuthContext<P>,
        resource: &str,
        action: P,
        object_id: Option<u32>,
        owner_id: Option<u32>,
    ) -> Result<(), SecurityError> {
        // 1. Check global permission first
        if auth.permissions.contains(&action) {
            self.log_access_granted(auth, resource, &action, object_id);
            return Ok(());
        }

        // 2. Check object-level permissions
        if let (Some(_obj_id), Some(owner)) = (object_id, owner_id) {
            // Check ownership-based permissions
            if self.check_ownership_permission(auth, &action, owner)? {
                self.log_access_granted(auth, resource, &action, object_id);
                return Ok(());
            }

            // Check team-based permissions
            if self.check_team_permission(auth, &action, owner)? {
                self.log_access_granted(auth, resource, &action, object_id);
                return Ok(());
            }
        }

        // 3. Access denied - log and return error
        self.log_access_denied(auth, resource, &action, object_id, "Insufficient permissions");
        Err(SecurityError::AccessDenied)
    }

    /// Extract JWT token from Authorization header
    fn extract_jwt_token(&self, request: &HttpRequest) -> Result<String, SecurityError> {
        let auth_header =
            request.header("Authorization").ok_or(SecurityError::AuthenticationFailed)?;

        if !auth_header.starts_with("Bearer ") {
            return Err(SecurityError::InvalidToken);
        }

        Ok(auth_header[7..].to_string()) // Remove "Bearer " prefix
    }

    /// Validate JWT token and extract claims
    fn validate_jwt(&self, token: &str) -> Result<JwtClaims, SecurityError> {
        // Simple JWT validation (in production, use a proper JWT library)
        // For now, we'll implement basic validation

        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            return Err(SecurityError::InvalidToken);
        }

        // Decode payload (base64url)
        let payload = self.base64url_decode(parts[1]).map_err(|_| SecurityError::InvalidToken)?;

        // Parse JSON payload
        let payload_str = String::from_utf8(payload).map_err(|_| SecurityError::InvalidToken)?;

        // Simple JSON parsing for claims
        let claims = self.parse_jwt_claims(&payload_str)?;

        // Verify signature (simplified)
        let expected_signature = self.create_jwt_signature(parts[0], parts[1]);
        let provided_signature = parts[2];

        if expected_signature != provided_signature {
            return Err(SecurityError::InvalidToken);
        }

        // Check expiration
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();

        if claims.exp < now {
            return Err(SecurityError::SessionExpired);
        }

        Ok(claims)
    }

    /// Validate session is active
    fn validate_session(&self, session_id: &str) -> Result<Session, SecurityError> {
        let state = self.security_state.read().unwrap();

        let session = state.active_sessions.get(session_id).ok_or(SecurityError::SessionExpired)?;

        // Check session expiration
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();

        if session.expires_at < now {
            return Err(SecurityError::SessionExpired);
        }

        Ok(session.clone())
    }

    /// Get user permissions from security state
    fn get_user_permissions(
        &self,
        user_id: u32,
    ) -> Result<std::collections::HashSet<P>, SecurityError> {
        let state = self.security_state.read().unwrap();
        Ok(state.get_user_permissions(user_id))
    }

    /// Get user team and organization context
    fn get_user_context(&self, user_id: u32) -> Result<(Option<u32>, Option<u32>), SecurityError> {
        let state = self.security_state.read().unwrap();

        let user = state.users.get(&user_id).ok_or(SecurityError::UserNotFound)?;

        Ok((user.team_id, user.organization_id))
    }

    /// Check ownership-based permission
    ///
    /// This is a generic implementation that relies on applications to define
    /// their own ownership-based permission logic through the Permission trait
    fn check_ownership_permission(
        &self,
        auth: &AuthContext<P>,
        _action: &P,
        owner_id: u32,
    ) -> Result<bool, SecurityError> {
        // Simple ownership check: if user owns the object, they have access
        // Applications can override this logic by implementing custom middleware
        Ok(auth.user_id == owner_id)
    }

    /// Check team-based permission
    ///
    /// This is a generic implementation that relies on applications to define
    /// their own team-based permission logic through the Permission trait
    fn check_team_permission(
        &self,
        auth: &AuthContext<P>,
        _action: &P,
        owner_id: u32,
    ) -> Result<bool, SecurityError> {
        // Check if users are in same team
        let state = self.security_state.read().unwrap();

        // Simple team check: if users are in same team, they have access
        // Applications can override this logic by implementing custom middleware
        Ok(state.users_same_team(auth.user_id, owner_id))
    }

    /// Log successful access
    fn log_access_granted(
        &self,
        auth: &AuthContext<P>,
        resource: &str,
        action: &P,
        object_id: Option<u32>,
    ) {
        let _event: SecurityEvent<P> = SecurityEvent::AccessGranted {
            user_id: auth.user_id,
            resource: resource.to_string(),
            action: format!("{:?}", action),
            object_id,
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
        };

        // In a full implementation, this would persist the event
        println!(
            "🔓 Access granted: user {} -> {} {:?} (object: {:?})",
            auth.user_id, resource, action, object_id
        );
    }

    /// Log access denial
    fn log_access_denied(
        &self,
        auth: &AuthContext<P>,
        resource: &str,
        action: &P,
        object_id: Option<u32>,
        reason: &str,
    ) {
        let _event: SecurityEvent<P> = SecurityEvent::AccessDenied {
            user_id: auth.user_id,
            resource: resource.to_string(),
            action: format!("{:?}", action),
            object_id,
            reason: reason.to_string(),
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
        };

        // In a full implementation, this would persist the event
        println!(
            "🔒 Access denied: user {} -> {} {:?} (object: {:?}) - {}",
            auth.user_id, resource, action, object_id, reason
        );
    }

    /// Create JWT token for user
    pub fn create_jwt_token(
        &self,
        user_id: u32,
        session_id: String,
    ) -> Result<String, SecurityError> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();

        let claims = JwtClaims { user_id, session_id, exp: now + self.session_timeout, iat: now };

        // Create JWT header
        let header = r#"{"alg":"HS256","typ":"JWT"}"#;
        let header_b64 = self.base64url_encode(header.as_bytes());

        // Create JWT payload
        let payload = format!(
            r#"{{"user_id":{},"session_id":"{}","exp":{},"iat":{}}}"#,
            claims.user_id, claims.session_id, claims.exp, claims.iat
        );
        let payload_b64 = self.base64url_encode(payload.as_bytes());

        // Create signature
        let signature = self.create_jwt_signature(&header_b64, &payload_b64);

        Ok(format!("{}.{}.{}", header_b64, payload_b64, signature))
    }

    /// Authenticate user with email/password
    pub fn authenticate_user(
        &self,
        email: &str,
        password: &str,
        ip_address: &str,
        user_agent: &str,
    ) -> Result<(String, AuthContext<P>), SecurityError> {
        // First, find and verify user (immutable borrow)
        let (user_id, user_team_id, user_org_id) = {
            let state = self.security_state.read().unwrap();

            // Find user by email
            let user = state
                .users
                .values()
                .find(|u| u.email == email && u.is_active)
                .ok_or(SecurityError::InvalidCredentials)?;

            // Verify password (in production, use proper password hashing)
            if !self.verify_password(password, &user.password_hash) {
                return Err(SecurityError::InvalidCredentials);
            }

            (user.id, user.team_id, user.organization_id)
        };

        // Now get permissions and create session (separate borrows)
        let permissions = {
            let state = self.security_state.read().unwrap();
            state.get_user_permissions(user_id)
        };

        // Create new session
        let session_id = self.generate_session_id();
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();

        let session = Session {
            id: session_id.clone(),
            user_id,
            created_at: now,
            expires_at: now + self.session_timeout,
            last_activity: now,
            ip_address: ip_address.to_string(),
            user_agent: user_agent.to_string(),
        };

        // Store session (mutable borrow)
        {
            let mut state = self.security_state.write().unwrap();
            state.active_sessions.insert(session_id.clone(), session);
        }

        // Create JWT token
        let token = self.create_jwt_token(user_id, session_id.clone())?;

        // Create auth context
        let auth_context = AuthContext {
            user_id,
            session_id: session_id.clone(),
            permissions,
            team_id: user_team_id,
            organization_id: user_org_id,
        };

        // Log authentication event
        println!("🔑 User authenticated: {} ({})", email, user_id);

        Ok((token, auth_context))
    }

    /// Logout user
    pub fn logout_user(&self, session_id: &str) -> Result<(), SecurityError> {
        let mut state = self.security_state.write().unwrap();

        if let Some(session) = state.active_sessions.remove(session_id) {
            println!("👋 User logged out: {} (session: {})", session.user_id, session_id);
            Ok(())
        } else {
            Err(SecurityError::SessionExpired)
        }
    }

    // Helper methods for JWT processing

    fn base64url_encode(&self, data: &[u8]) -> String {
        URL_SAFE_NO_PAD.encode(data)
    }

    fn base64url_decode(&self, data: &str) -> Result<Vec<u8>, String> {
        URL_SAFE_NO_PAD.decode(data).map_err(|e| format!("Base64 decode error: {}", e))
    }

    fn create_jwt_signature(&self, header: &str, payload: &str) -> String {
        // HMAC-SHA256 signature (cryptographically secure)
        let message = format!("{}.{}", header, payload);

        let mut mac = HmacSha256::new_from_slice(self.jwt_secret.as_bytes())
            .expect("HMAC can take key of any size");
        mac.update(message.as_bytes());

        let result = mac.finalize();
        self.base64url_encode(&result.into_bytes())
    }

    fn parse_jwt_claims(&self, payload: &str) -> Result<JwtClaims, SecurityError> {
        // Simple JSON parsing for JWT claims
        // In production, use a proper JSON parser

        // Extract values using simple string parsing
        let user_id = self
            .extract_json_number(payload, "user_id")
            .ok_or(SecurityError::InvalidToken)? as u32;

        let session_id = self
            .extract_json_string(payload, "session_id")
            .ok_or(SecurityError::InvalidToken)?;

        let exp = self.extract_json_number(payload, "exp").ok_or(SecurityError::InvalidToken)?;

        let iat = self.extract_json_number(payload, "iat").ok_or(SecurityError::InvalidToken)?;

        Ok(JwtClaims { user_id, session_id, exp, iat })
    }

    fn extract_json_number(&self, json: &str, key: &str) -> Option<u64> {
        let pattern = format!("\"{}\":", key);
        if let Some(start) = json.find(&pattern) {
            let start = start + pattern.len();
            let end = json[start..]
                .find(',')
                .unwrap_or(json[start..].find('}').unwrap_or(json.len() - start));
            let value_str = json[start..start + end].trim();
            value_str.parse().ok()
        } else {
            None
        }
    }

    fn extract_json_string(&self, json: &str, key: &str) -> Option<String> {
        let pattern = format!("\"{}\":\"", key);
        if let Some(start) = json.find(&pattern) {
            let start = start + pattern.len();
            let end = json[start..].find('"').unwrap_or(0);
            Some(json[start..start + end].to_string())
        } else {
            None
        }
    }

    fn verify_password(&self, password: &str, hash: &str) -> bool {
        // Use Argon2id for secure password verification
        // If hash is in Argon2 format, use argon2_verify
        // Otherwise, fall back to plaintext comparison for migration period
        if hash.starts_with("$argon2") {
            argon2_verify(password, hash).unwrap_or(false)
        } else {
            // Legacy plaintext comparison - log warning
            // TODO: Remove after migration to Argon2 hashes
            log::warn!("Using plaintext password comparison - please migrate to Argon2 hashes");
            password == hash
        }
    }

    fn generate_session_id(&self) -> String {
        // Cryptographically secure session ID using UUID v4
        format!("sess_{}", Uuid::new_v4())
    }
}

// Note: Using the real base64 crate with URL_SAFE_NO_PAD engine
// This provides proper RFC 4648 base64url encoding/decoding

#[cfg(test)]
mod tests {
    use super::*;

    // Test permission type for middleware tests
    #[derive(Clone, PartialEq, Eq, Hash, Debug)]
    #[allow(dead_code)]
    #[allow(clippy::enum_variant_names)]
    enum TestPermission {
        ReadData,
        WriteData,
        DeleteData,
    }

    impl Permission for TestPermission {
        fn identifier(&self) -> &str {
            match self {
                TestPermission::ReadData => "read_data",
                TestPermission::WriteData => "write_data",
                TestPermission::DeleteData => "delete_data",
            }
        }

        fn description(&self) -> &str {
            match self {
                TestPermission::ReadData => "Read data permission",
                TestPermission::WriteData => "Write data permission",
                TestPermission::DeleteData => "Delete data permission",
            }
        }
    }

    #[test]
    fn test_rbac_middleware_creation() {
        let security_state: Arc<RwLock<SecurityState<TestPermission>>> =
            Arc::new(RwLock::new(SecurityState::default()));
        let middleware: RBACMiddleware<TestPermission> =
            RBACMiddleware::new(security_state, "test_secret".to_string());

        assert_eq!(middleware.session_timeout, 24 * 60 * 60);
    }

    #[test]
    fn test_jwt_token_creation() {
        let security_state: Arc<RwLock<SecurityState<TestPermission>>> =
            Arc::new(RwLock::new(SecurityState::default()));
        let middleware: RBACMiddleware<TestPermission> =
            RBACMiddleware::new(security_state, "test_secret".to_string());

        let token = middleware.create_jwt_token(1, "session_123".to_string()).unwrap();

        // Token should have 3 parts separated by dots
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
    }

    #[test]
    fn test_session_id_generation() {
        let security_state: Arc<RwLock<SecurityState<TestPermission>>> =
            Arc::new(RwLock::new(SecurityState::default()));
        let middleware: RBACMiddleware<TestPermission> =
            RBACMiddleware::new(security_state, "test_secret".to_string());

        let session_id1 = middleware.generate_session_id();
        let session_id2 = middleware.generate_session_id();

        assert_ne!(session_id1, session_id2);
        assert!(session_id1.starts_with("sess_"));
        assert!(session_id2.starts_with("sess_"));
    }
}
