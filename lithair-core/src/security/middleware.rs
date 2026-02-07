//! RBAC Middleware for Lithair HTTP Server
//!
//! This middleware provides automatic authentication and authorization for all HTTP requests.
//! It integrates with the Lithair HTTP server and event sourcing system.
//!
//! ## Security Features
//! - **JWT**: HMAC-SHA256 with constant-time signature verification
//! - **Sessions**: UUID v4 (cryptographically random)
//! - **Passwords**: Argon2id via password module

use super::password::verify_password as argon2_verify;
use super::{AuthContext, Permission, SecurityError, SecurityState, Session};
use crate::http::HttpRequest;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use sha2::Sha256;
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
    pub exp: u64,
    pub iat: u64,
}

/// RBAC Middleware that enforces authentication and authorization
///
/// Generic over Permission type to allow applications to define their own permission systems.
#[derive(Debug)]
pub struct RBACMiddleware<P: Permission> {
    security_state: Arc<RwLock<SecurityState<P>>>,
    jwt_secret: String,
    session_timeout: u64,
}

impl<P: Permission> RBACMiddleware<P> {
    /// Create new RBAC middleware with a 24-hour default session timeout
    pub fn new(security_state: Arc<RwLock<SecurityState<P>>>, jwt_secret: String) -> Self {
        Self { security_state, jwt_secret, session_timeout: 24 * 60 * 60 }
    }

    /// Set session timeout in seconds
    pub fn with_session_timeout(mut self, timeout_seconds: u64) -> Self {
        self.session_timeout = timeout_seconds;
        self
    }

    /// Authenticate directly from a JWT token string and return an AuthContext
    pub fn authenticate_token(&self, token: &str) -> Result<AuthContext<P>, SecurityError> {
        let claims = self.validate_jwt(token)?;
        self.validate_session(&claims.session_id)?;
        let permissions = self.get_user_permissions(claims.user_id)?;
        let (team_id, organization_id) = self.get_user_context(claims.user_id)?;

        Ok(AuthContext {
            user_id: claims.user_id,
            session_id: claims.session_id,
            permissions,
            team_id,
            organization_id,
        })
    }

    /// Authenticate a request by extracting the JWT from the Authorization header
    pub fn authenticate_request(
        &self,
        request: &HttpRequest,
    ) -> Result<AuthContext<P>, SecurityError> {
        let token = self.extract_jwt_token(request)?;
        self.authenticate_token(&token)
    }

    /// Authorize an action with optional object-level checks
    ///
    /// Permission resolution order:
    /// 1. Global permission check
    /// 2. Object ownership check (if object_id and owner_id provided)
    /// 3. Team membership check (if object_id and owner_id provided)
    pub fn authorize_action(
        &self,
        auth: &AuthContext<P>,
        resource: &str,
        action: P,
        object_id: Option<u32>,
        owner_id: Option<u32>,
    ) -> Result<(), SecurityError> {
        if auth.permissions.contains(&action) {
            log::debug!(
                "Access granted: user {} -> {} {:?} (object: {:?})",
                auth.user_id,
                resource,
                action,
                object_id
            );
            return Ok(());
        }

        if let (Some(_), Some(owner)) = (object_id, owner_id) {
            if self.check_ownership_permission(auth, owner)? {
                log::debug!(
                    "Access granted (ownership): user {} -> {} {:?} (object: {:?})",
                    auth.user_id,
                    resource,
                    action,
                    object_id
                );
                return Ok(());
            }

            if self.check_team_permission(auth, owner)? {
                log::debug!(
                    "Access granted (team): user {} -> {} {:?} (object: {:?})",
                    auth.user_id,
                    resource,
                    action,
                    object_id
                );
                return Ok(());
            }
        }

        log::warn!(
            "Access denied: user {} -> {} {:?} (object: {:?})",
            auth.user_id,
            resource,
            action,
            object_id
        );
        Err(SecurityError::AccessDenied)
    }

    /// Create JWT token for a user session
    pub fn create_jwt_token(
        &self,
        user_id: u32,
        session_id: String,
    ) -> Result<String, SecurityError> {
        let now = self.now();

        let claims = JwtClaims { user_id, session_id, exp: now + self.session_timeout, iat: now };

        let header = r#"{"alg":"HS256","typ":"JWT"}"#;
        let header_b64 = self.base64url_encode(header.as_bytes());

        let payload = format!(
            r#"{{"user_id":{},"session_id":"{}","exp":{},"iat":{}}}"#,
            claims.user_id, claims.session_id, claims.exp, claims.iat
        );
        let payload_b64 = self.base64url_encode(payload.as_bytes());

        let signature = self.create_jwt_signature(&header_b64, &payload_b64);

        Ok(format!("{}.{}.{}", header_b64, payload_b64, signature))
    }

    /// Authenticate user with email/password and create a new session
    pub fn authenticate_user(
        &self,
        email: &str,
        password: &str,
        ip_address: &str,
        user_agent: &str,
    ) -> Result<(String, AuthContext<P>), SecurityError> {
        let (user_id, user_team_id, user_org_id) = {
            let state = self.read_state()?;

            let user = state
                .users
                .values()
                .find(|u| u.email == email && u.is_active)
                .ok_or(SecurityError::InvalidCredentials)?;

            if !self.verify_password(password, &user.password_hash) {
                return Err(SecurityError::InvalidCredentials);
            }

            (user.id, user.team_id, user.organization_id)
        };

        let permissions = {
            let state = self.read_state()?;
            state.get_user_permissions(user_id)
        };

        let session_id = self.generate_session_id();
        let now = self.now();

        let session = Session {
            id: session_id.clone(),
            user_id,
            created_at: now,
            expires_at: now + self.session_timeout,
            last_activity: now,
            ip_address: ip_address.to_string(),
            user_agent: user_agent.to_string(),
        };

        {
            let mut state = self.write_state()?;
            state.active_sessions.insert(session_id.clone(), session);
        }

        let token = self.create_jwt_token(user_id, session_id.clone())?;

        let auth_context = AuthContext {
            user_id,
            session_id,
            permissions,
            team_id: user_team_id,
            organization_id: user_org_id,
        };

        log::info!("User authenticated: {} ({})", email, user_id);

        Ok((token, auth_context))
    }

    /// Logout user by removing their session
    pub fn logout_user(&self, session_id: &str) -> Result<(), SecurityError> {
        let mut state = self.write_state()?;

        if let Some(session) = state.active_sessions.remove(session_id) {
            log::info!("User logged out: {} (session: {})", session.user_id, session_id);
            Ok(())
        } else {
            Err(SecurityError::SessionExpired)
        }
    }

    // --- Private helpers ---

    /// Acquire a read lock on the security state
    fn read_state(
        &self,
    ) -> Result<std::sync::RwLockReadGuard<'_, SecurityState<P>>, SecurityError> {
        self.security_state.read().map_err(|_| SecurityError::AuthenticationFailed)
    }

    /// Acquire a write lock on the security state
    fn write_state(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, SecurityState<P>>, SecurityError> {
        self.security_state.write().map_err(|_| SecurityError::AuthenticationFailed)
    }

    /// Get current UNIX timestamp
    fn now(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before UNIX epoch")
            .as_secs()
    }

    /// Extract JWT token from Authorization header
    fn extract_jwt_token(&self, request: &HttpRequest) -> Result<String, SecurityError> {
        let auth_header =
            request.header("Authorization").ok_or(SecurityError::AuthenticationFailed)?;

        auth_header
            .strip_prefix("Bearer ")
            .map(|t| t.to_string())
            .ok_or(SecurityError::InvalidToken)
    }

    /// Validate JWT token and extract claims
    fn validate_jwt(&self, token: &str) -> Result<JwtClaims, SecurityError> {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            return Err(SecurityError::InvalidToken);
        }

        // Verify signature using constant-time comparison (prevents timing attacks)
        self.verify_jwt_signature(parts[0], parts[1], parts[2])?;

        let payload = self.base64url_decode(parts[1]).map_err(|_| SecurityError::InvalidToken)?;
        let payload_str = String::from_utf8(payload).map_err(|_| SecurityError::InvalidToken)?;
        let claims = self.parse_jwt_claims(&payload_str)?;

        if claims.exp < self.now() {
            return Err(SecurityError::SessionExpired);
        }

        Ok(claims)
    }

    /// Validate that a session exists and is not expired
    fn validate_session(&self, session_id: &str) -> Result<Session, SecurityError> {
        let state = self.read_state()?;

        let session = state.active_sessions.get(session_id).ok_or(SecurityError::SessionExpired)?;

        if session.expires_at < self.now() {
            return Err(SecurityError::SessionExpired);
        }

        Ok(session.clone())
    }

    /// Get all permissions for a user by aggregating role permissions
    fn get_user_permissions(
        &self,
        user_id: u32,
    ) -> Result<std::collections::HashSet<P>, SecurityError> {
        let state = self.read_state()?;
        Ok(state.get_user_permissions(user_id))
    }

    /// Get user team and organization context
    fn get_user_context(&self, user_id: u32) -> Result<(Option<u32>, Option<u32>), SecurityError> {
        let state = self.read_state()?;
        let user = state.users.get(&user_id).ok_or(SecurityError::UserNotFound)?;
        Ok((user.team_id, user.organization_id))
    }

    /// Check if the authenticated user owns the object
    fn check_ownership_permission(
        &self,
        auth: &AuthContext<P>,
        owner_id: u32,
    ) -> Result<bool, SecurityError> {
        Ok(auth.user_id == owner_id)
    }

    /// Check if the authenticated user and the owner are in the same team
    fn check_team_permission(
        &self,
        auth: &AuthContext<P>,
        owner_id: u32,
    ) -> Result<bool, SecurityError> {
        let state = self.read_state()?;
        Ok(state.users_same_team(auth.user_id, owner_id))
    }

    fn base64url_encode(&self, data: &[u8]) -> String {
        URL_SAFE_NO_PAD.encode(data)
    }

    fn base64url_decode(&self, data: &str) -> Result<Vec<u8>, String> {
        URL_SAFE_NO_PAD.decode(data).map_err(|e| format!("Base64 decode error: {}", e))
    }

    fn create_jwt_signature(&self, header: &str, payload: &str) -> String {
        let message = format!("{}.{}", header, payload);

        let mut mac = HmacSha256::new_from_slice(self.jwt_secret.as_bytes())
            .expect("HMAC accepts keys of any size");
        mac.update(message.as_bytes());

        let result = mac.finalize();
        self.base64url_encode(&result.into_bytes())
    }

    /// Verify JWT signature using constant-time comparison (prevents timing attacks)
    fn verify_jwt_signature(
        &self,
        header: &str,
        payload: &str,
        signature: &str,
    ) -> Result<(), SecurityError> {
        let message = format!("{}.{}", header, payload);

        let mut mac = HmacSha256::new_from_slice(self.jwt_secret.as_bytes())
            .expect("HMAC accepts keys of any size");
        mac.update(message.as_bytes());

        let signature_bytes =
            self.base64url_decode(signature).map_err(|_| SecurityError::InvalidToken)?;

        mac.verify_slice(&signature_bytes).map_err(|_| SecurityError::InvalidToken)
    }

    fn parse_jwt_claims(&self, payload: &str) -> Result<JwtClaims, SecurityError> {
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
        argon2_verify(password, hash).unwrap_or(false)
    }

    fn generate_session_id(&self) -> String {
        format!("sess_{}", Uuid::new_v4())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, PartialEq, Eq, Hash, Debug)]
    #[allow(dead_code, clippy::enum_variant_names)]
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

    fn create_middleware() -> RBACMiddleware<TestPermission> {
        let security_state = Arc::new(RwLock::new(SecurityState::default()));
        RBACMiddleware::new(security_state, "test_secret".to_string())
    }

    #[test]
    fn test_rbac_middleware_creation() {
        let middleware = create_middleware();
        assert_eq!(middleware.session_timeout, 24 * 60 * 60);
    }

    #[test]
    fn test_jwt_token_creation_and_validation() {
        let middleware = create_middleware();
        let token = middleware.create_jwt_token(1, "session_123".to_string()).unwrap();

        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);

        // Validate the token we just created
        let claims = middleware.validate_jwt(&token).unwrap();
        assert_eq!(claims.user_id, 1);
        assert_eq!(claims.session_id, "session_123");
    }

    #[test]
    fn test_jwt_invalid_signature_rejected() {
        let middleware = create_middleware();
        let token = middleware.create_jwt_token(1, "session_123".to_string()).unwrap();

        // Tamper with the signature
        let parts: Vec<&str> = token.split('.').collect();
        let tampered = format!("{}.{}.invalid_signature", parts[0], parts[1]);

        assert!(middleware.validate_jwt(&tampered).is_err());
    }

    #[test]
    fn test_jwt_malformed_token_rejected() {
        let middleware = create_middleware();
        assert!(middleware.validate_jwt("not.a.valid.jwt.token").is_err());
        assert!(middleware.validate_jwt("only_one_part").is_err());
        assert!(middleware.validate_jwt("").is_err());
    }

    #[test]
    fn test_session_id_generation() {
        let middleware = create_middleware();

        let session_id1 = middleware.generate_session_id();
        let session_id2 = middleware.generate_session_id();

        assert_ne!(session_id1, session_id2);
        assert!(session_id1.starts_with("sess_"));
        assert!(session_id2.starts_with("sess_"));
    }

    #[test]
    fn test_extract_jwt_token_missing_header() {
        let middleware = create_middleware();
        let request = HttpRequest::new(
            crate::http::HttpMethod::GET,
            "/test".to_string(),
            crate::http::HttpVersion::Http1_1,
            std::collections::HashMap::new(),
            Vec::new(),
        );
        assert!(middleware.extract_jwt_token(&request).is_err());
    }
}
