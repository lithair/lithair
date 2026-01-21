//! Google OAuth2 authentication provider

use crate::rbac::context::AuthContext;
use crate::rbac::traits::AuthProvider;
use anyhow::{anyhow, Result};
use bytes::Bytes;
use http::Request;
use http_body_util::Full;
use serde::{Deserialize, Serialize};

/// Google OAuth2 provider
#[derive(Clone)]
#[allow(dead_code)]
pub struct GoogleProvider {
    /// OAuth2 client ID
    client_id: String,

    /// OAuth2 client secret
    client_secret: String,

    /// Redirect URI (e.g., "http://localhost:3000/auth/google/callback")
    redirect_uri: String,

    /// Default role for authenticated users
    default_role: String,
}

/// Google user info response
#[derive(Debug, Deserialize, Serialize)]
#[allow(dead_code)]
pub(crate) struct GoogleUserInfo {
    id: String,
    email: String,
    verified_email: bool,
    name: String,
    given_name: Option<String>,
    family_name: Option<String>,
    picture: Option<String>,
}

/// OAuth2 token response
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
    token_type: String,
    scope: Option<String>,
    refresh_token: Option<String>,
}

#[allow(dead_code)]
impl GoogleProvider {
    /// Create a new Google OAuth2 provider
    pub fn new(
        client_id: String,
        client_secret: String,
        redirect_uri: String,
        default_role: String,
    ) -> Self {
        Self { client_id, client_secret, redirect_uri, default_role }
    }

    /// Get the OAuth2 authorization URL
    pub fn get_auth_url(&self, state: &str) -> String {
        format!(
            "https://accounts.google.com/o/oauth2/v2/auth?\
             client_id={}&\
             redirect_uri={}&\
             response_type=code&\
             scope=openid%20email%20profile&\
             state={}",
            urlencoding::encode(&self.client_id),
            urlencoding::encode(&self.redirect_uri),
            urlencoding::encode(state)
        )
    }

    /// Exchange authorization code for access token
    pub async fn exchange_code(&self, code: &str) -> Result<String> {
        let client = reqwest::Client::new();

        let params = [
            ("code", code),
            ("client_id", &self.client_id),
            ("client_secret", &self.client_secret),
            ("redirect_uri", &self.redirect_uri),
            ("grant_type", "authorization_code"),
        ];

        let response = client
            .post("https://oauth2.googleapis.com/token")
            .form(&params)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to exchange code: {}", e))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow!("Token exchange failed: {}", error_text));
        }

        let token_response: TokenResponse = response
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse token response: {}", e))?;

        Ok(token_response.access_token)
    }

    /// Get user info from Google
    pub async fn get_user_info(&self, access_token: &str) -> Result<GoogleUserInfo> {
        let client = reqwest::Client::new();

        let response = client
            .get("https://www.googleapis.com/oauth2/v2/userinfo")
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to get user info: {}", e))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow!("User info request failed: {}", error_text));
        }

        let user_info: GoogleUserInfo =
            response.json().await.map_err(|e| anyhow!("Failed to parse user info: {}", e))?;

        Ok(user_info)
    }
}

impl AuthProvider for GoogleProvider {
    fn name(&self) -> &str {
        "google"
    }

    fn authenticate(&self, request: &Request<Full<Bytes>>) -> Result<AuthContext> {
        // Extract access token from Authorization header
        let token = request
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .ok_or_else(|| anyhow!("Missing or invalid Authorization header"))?;

        // In a real implementation, you would:
        // 1. Validate the token with Google
        // 2. Get user info
        // 3. Map user to roles
        //
        // For now, we'll accept any Bearer token as valid
        // and return a basic auth context

        Ok(AuthContext {
            user_id: Some(token.to_string()),
            roles: vec![self.default_role.clone()],
            groups: vec![],
            authenticated: true,
            provider: "google".to_string(),
            mfa_verified: false,
            metadata: std::collections::HashMap::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_url_generation() {
        let provider = GoogleProvider::new(
            "test-client-id".to_string(),
            "test-secret".to_string(),
            "http://localhost:3000/callback".to_string(),
            "User".to_string(),
        );

        let url = provider.get_auth_url("random-state");

        assert!(url.contains("accounts.google.com"));
        assert!(url.contains("client_id=test-client-id"));
        assert!(url.contains("state=random-state"));
    }
}
