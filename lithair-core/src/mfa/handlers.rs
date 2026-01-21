//! Automatic MFA route handlers
//!
//! These handlers are automatically generated when using `.with_mfa_totp()`

use super::{MfaConfig, MfaStorage, TotpSecret, TotpValidator};
use anyhow::{anyhow, Result};
use bytes::Bytes;
use http_body_util::Full;
use hyper::{Request, Response, StatusCode};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// MFA setup request
#[derive(Debug, Deserialize)]
pub struct MfaSetupRequest {
    pub username: String,
}

/// MFA setup response with QR code
#[derive(Debug, Serialize)]
pub struct MfaSetupResponse {
    pub secret: String,
    pub qr_code_base64: String,
    pub uri: String,
}

/// MFA enable request (verify first code)
#[derive(Debug, Deserialize)]
pub struct MfaEnableRequest {
    pub username: String,
    pub code: String,
}

/// MFA verify request
#[derive(Debug, Deserialize)]
pub struct MfaVerifyRequest {
    pub username: String,
    pub code: String,
}

/// MFA status response
#[derive(Debug, Serialize)]
pub struct MfaStatusResponse {
    pub enabled: bool,
    pub required: bool,
}

/// Generic success response
#[derive(Debug, Serialize)]
pub struct SuccessResponse {
    pub success: bool,
    pub message: String,
}

/// Error response
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

/// Handle GET /auth/mfa/status
pub async fn handle_mfa_status(
    storage: Arc<MfaStorage>,
    _config: Arc<MfaConfig>,
    req: Request<hyper::body::Incoming>,
) -> Result<Response<Full<Bytes>>> {
    // Extract username from query params or session
    let uri = req.uri();
    let query = uri.query().unwrap_or("");

    let username = query
        .split('&')
        .find(|s| s.starts_with("username="))
        .and_then(|s| s.strip_prefix("username="))
        .ok_or_else(|| anyhow!("Missing username parameter"))?;

    let enabled = storage.is_enabled(username).await;

    // Check if MFA is required for this user's role (would need role info)
    let required = false; // TODO: Get from session/user role

    let response = MfaStatusResponse { enabled, required };
    let json = serde_json::to_string(&response)?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(json)))?)
}

/// Handle POST /auth/mfa/setup - Generate secret and QR code
pub async fn handle_mfa_setup(
    storage: Arc<MfaStorage>,
    config: Arc<MfaConfig>,
    req: Request<hyper::body::Incoming>,
) -> Result<Response<Full<Bytes>>> {
    use http_body_util::BodyExt;

    // Parse request body
    let body_bytes = req.collect().await?.to_bytes();
    let setup_req: MfaSetupRequest = serde_json::from_slice(&body_bytes)?;

    // Generate new TOTP secret
    let secret = TotpSecret::generate_with_account(
        config.algorithm,
        config.digits,
        config.step,
        &config.issuer,
        &setup_req.username,
    );

    // Generate QR code
    let qr_code = secret.get_qr_code().map_err(|e| anyhow!("Failed to generate QR code: {}", e))?;

    let uri = secret.to_uri().map_err(|e| anyhow!("Failed to generate URI: {}", e))?;

    // Save the secret with enabled=false (will be activated in /enable endpoint)
    use super::storage::UserMfaData;
    use super::MfaStatus;

    let user_data = UserMfaData {
        secret: secret.clone(),
        status: MfaStatus { enabled: false, required: false, enabled_at: None },
        backup_codes: Vec::new(),
    };

    storage.save(&setup_req.username, user_data).await?;

    let response = MfaSetupResponse { secret: secret.secret.clone(), qr_code_base64: qr_code, uri };

    let json = serde_json::to_string(&response)?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(json)))?)
}

/// Handle POST /auth/mfa/enable - Verify code and enable MFA
pub async fn handle_mfa_enable(
    storage: Arc<MfaStorage>,
    _config: Arc<MfaConfig>,
    req: Request<hyper::body::Incoming>,
) -> Result<Response<Full<Bytes>>> {
    use http_body_util::BodyExt;

    // Parse request body
    let body_bytes = req.collect().await?.to_bytes();
    let enable_req: MfaEnableRequest = serde_json::from_slice(&body_bytes)?;

    // Get the secret from a temporary storage or expect it in request
    // For now, we'll expect user already did setup and we validate
    let user_data = storage
        .get(&enable_req.username)
        .await?
        .ok_or_else(|| anyhow!("MFA not set up. Call /auth/mfa/setup first"))?;

    // Validate the code
    let valid = TotpValidator::validate(&user_data.secret, &enable_req.code)?;

    if !valid {
        // Record failed verification attempt
        storage
            .record_verification_failure(&enable_req.username, "invalid_code")
            .await?;

        return Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(r#"{"error":"Invalid code"}"#)))?);
    }

    // Record successful verification
    storage.record_verification_success(&enable_req.username).await?;

    // Enable MFA (emit MfaEnabled event)
    storage.enable(&enable_req.username).await?;

    let response =
        SuccessResponse { success: true, message: "MFA enabled successfully".to_string() };

    let json = serde_json::to_string(&response)?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(json)))?)
}

/// Handle POST /auth/mfa/disable - Disable MFA
pub async fn handle_mfa_disable(
    storage: Arc<MfaStorage>,
    _config: Arc<MfaConfig>,
    req: Request<hyper::body::Incoming>,
) -> Result<Response<Full<Bytes>>> {
    use http_body_util::BodyExt;

    // Parse request body
    let body_bytes = req.collect().await?.to_bytes();
    let disable_req: MfaVerifyRequest = serde_json::from_slice(&body_bytes)?;

    // Verify code before disabling (security)
    let user_data = storage
        .get(&disable_req.username)
        .await?
        .ok_or_else(|| anyhow!("MFA not enabled"))?;

    let valid = TotpValidator::validate(&user_data.secret, &disable_req.code)?;

    if !valid {
        // Record failed verification attempt
        storage
            .record_verification_failure(&disable_req.username, "invalid_code")
            .await?;

        return Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(r#"{"error":"Invalid code"}"#)))?);
    }

    // Record successful verification before disabling
    storage.record_verification_success(&disable_req.username).await?;

    // Delete MFA data (emit MfaDisabled event)
    storage.delete(&disable_req.username).await?;

    let response =
        SuccessResponse { success: true, message: "MFA disabled successfully".to_string() };

    let json = serde_json::to_string(&response)?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(json)))?)
}

/// Handle POST /auth/mfa/verify - Verify MFA code during login
pub async fn handle_mfa_verify(
    storage: Arc<MfaStorage>,
    _config: Arc<MfaConfig>,
    req: Request<hyper::body::Incoming>,
) -> Result<Response<Full<Bytes>>> {
    use http_body_util::BodyExt;

    // Parse request body
    let body_bytes = req.collect().await?.to_bytes();
    let verify_req: MfaVerifyRequest = serde_json::from_slice(&body_bytes)?;

    // Get user MFA data
    let user_data = storage
        .get(&verify_req.username)
        .await?
        .ok_or_else(|| anyhow!("MFA not enabled for this user"))?;

    if !user_data.status.enabled {
        return Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(r#"{"error":"MFA not enabled"}"#)))?);
    }

    // Validate code
    let valid = TotpValidator::validate(&user_data.secret, &verify_req.code)?;

    // Record verification attempt
    if valid {
        storage.record_verification_success(&verify_req.username).await?;
    } else {
        storage
            .record_verification_failure(&verify_req.username, "invalid_code")
            .await?;
    }

    let response = SuccessResponse {
        success: valid,
        message: if valid { "Valid code".to_string() } else { "Invalid code".to_string() },
    };

    let json = serde_json::to_string(&response)?;

    let status = if valid { StatusCode::OK } else { StatusCode::UNAUTHORIZED };

    Ok(Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(json)))?)
}
