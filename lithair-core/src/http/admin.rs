//! Automatic admin endpoint system for Lithair applications
//!
//! This module provides a complete automatic admin system:
//! - **Automatic endpoints**: `/status`, `/health`, `/info` generated automatically
//! - **Zero boilerplate**: Just implement `ServerMetrics` trait and configure `AutoAdminConfig`
//! - **Firewall integration**: Built-in protection and logging
//! - **Extensible**: Custom endpoints coexist with automatic ones
//!
//! # Usage
//! ```rust
//! // 1. Implement ServerMetrics for your server
//! impl ServerMetrics for MyServer {
//!     fn get_uptime_seconds(&self) -> i64 { ... }
//!     fn get_server_mode(&self) -> &str { ... }
//!     // ...
//! }
//!
//! // 2. Configure automatic endpoints
//! let config = AutoAdminConfig {
//!     enable_status: true,
//!     enable_health: true,
//!     enable_info: true,
//!     admin_prefix: "/admin".to_string(),
//! };
//!
//! // 3. Use in your routing
//! if let Some(response) = handle_auto_admin_endpoints(&method, &path, &req, &server, &config, firewall).await {
//!     return response; // Automatic endpoint handled
//! }
//! // Handle custom endpoints here...
//! ```

#[allow(unused_imports)]
use http_body_util::BodyExt;
use crate::http::firewall::Firewall;
use crate::http::{body_from, extract_client_ip, json_error_response, not_found_response, Req, Resp};
use hyper::{Method, StatusCode};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;

/// Legacy admin handler trait - DEPRECATED
///
/// **⚠️ DEPRECATED**: Use the automatic admin system instead.
/// - Implement `ServerMetrics` trait
/// - Configure `AutoAdminConfig`
/// - Use `handle_auto_admin_endpoints` for automatic routing
///
/// This trait is kept for backward compatibility but should not be used in new code.
#[deprecated(since = "0.2.0", note = "Use automatic admin system with ServerMetrics trait instead")]
pub trait AdminHandler: Send + Sync {
    /// Handle admin GET requests (status, info, etc.)
    fn handle_admin_get(&self, path: &str, req: &Req) -> impl std::future::Future<Output = Resp> + Send;

    /// Handle admin POST requests (actions, configuration, etc.)
    fn handle_admin_post(&self, path: &str, req: Req) -> impl std::future::Future<Output = Resp> + Send;
}

/// Generic admin firewall protection with logging
pub async fn check_admin_firewall(
    firewall: Option<&Firewall>,
    method: &Method,
    path: &str,
    req: &Req,
    fake_addr: Option<SocketAddr>,
) -> Result<(), Resp> {
    if let Some(firewall) = firewall {
        if firewall.check(fake_addr, method, path).is_err() {
            if let Some(client_ip) = extract_client_ip(req) {
                log::warn!("🚫 Admin access denied from IP: {} for path: {}", client_ip, path);
            } else {
                log::warn!("🚫 Admin access denied: Could not determine client IP for path: {}", path);
            }

            return Err(forbidden_admin_response());
        }

        if let Some(client_ip) = extract_client_ip(req) {
            log::debug!("✅ Admin access allowed from IP: {} for path: {}", client_ip, path);
        }
    }
    Ok(())
}

/// Create a standardized 403 Forbidden response for admin endpoints
pub fn forbidden_admin_response() -> Resp {
    use chrono::Utc;
    use serde_json::json;

    hyper::Response::builder()
        .status(StatusCode::FORBIDDEN)
        .header("Content-Type", "application/json")
        .body(body_from(json!({
            "error": "access_denied",
            "message": "Access to admin endpoints is restricted",
            "timestamp": Utc::now().to_rfc3339()
        }).to_string()))
        .unwrap()
}

/// Format uptime duration in human-readable format
pub fn format_uptime(uptime_seconds: i64) -> String {
    let hours = uptime_seconds / 3600;
    let minutes = (uptime_seconds % 3600) / 60;
    let seconds = uptime_seconds % 60;

    if hours > 0 {
        format!("{}h{}m{}s", hours, minutes, seconds)
    } else if minutes > 0 {
        format!("{}m{}s", minutes, seconds)
    } else {
        format!("{}s", seconds)
    }
}

/// Generic status response builder
pub fn build_status_response(status_data: serde_json::Value) -> Resp {
    hyper::Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(body_from(status_data.to_string()))
        .unwrap()
}

/// Legacy admin route dispatcher - DEPRECATED
///
/// **⚠️ DEPRECATED**: Use `handle_auto_admin_endpoints` instead.
/// The automatic admin system provides better routing with zero boilerplate.
///
/// This function is kept for backward compatibility but should not be used in new code.
#[deprecated(since = "0.2.0", note = "Use handle_auto_admin_endpoints instead")]
#[allow(deprecated)]
pub async fn dispatch_admin_route<H>(
    method: &Method,
    path: &str,
    req: Req,
    handler: &Arc<H>,
    firewall: Option<&Firewall>,
) -> Resp
where
    H: AdminHandler,
{
    // Check firewall protection first
    let fake_addr: Option<SocketAddr> = "127.0.0.1:0".parse().ok();
    if let Err(forbidden_response) = check_admin_firewall(firewall, method, path, &req, fake_addr).await {
        return forbidden_response;
    }

    // Route based on HTTP method
    match method {
        &Method::GET => handler.handle_admin_get(path, &req).await,
        &Method::POST => handler.handle_admin_post(path, req).await,
        _ => json_error_response(
            StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
            "Only GET and POST methods are supported for admin endpoints"
        ),
    }
}

//  ╔═══════════════════════════════════════════════════════════╗
//  ║                    NEW AUTOMATIC ADMIN API               ║
//  ╠═══════════════════════════════════════════════════════════╣
//  ║  🚀 CORE TRAIT: ServerMetrics - implement this only      ║
//  ║  🎛️  CONFIGURATION: AutoAdminConfig - configure once      ║
//  ║  🔄 ROUTER: handle_auto_admin_endpoints - call in routes ║
//  ║  ✨ RESULT: Automatic /status, /health, /info endpoints   ║
//  ╚═══════════════════════════════════════════════════════════╝

/// **🎯 CORE TRAIT** - Server metrics for automatic admin endpoints
///
/// This is the ONLY trait you need to implement for the automatic admin system.
/// Once implemented, you get `/status`, `/health`, and `/info` endpoints for free.
///
/// # Example Implementation
/// ```rust
/// impl ServerMetrics for MyServer {
///     fn get_uptime_seconds(&self) -> i64 {
///         chrono::Utc::now().signed_duration_since(self.start_time).num_seconds()
///     }
///
///     fn get_server_mode(&self) -> &str {
///         if self.dev_mode { "dev" } else { "prod" }
///     }
///
///     fn get_last_reload_at(&self) -> Option<String> {
///         self.last_reload.as_ref().map(|dt| dt.to_rfc3339())
///     }
///
///     fn get_server_start_time(&self) -> String {
///         self.start_time.to_rfc3339()
///     }
/// }
/// ```
pub trait ServerMetrics {
    /// Get server uptime in seconds
    fn get_uptime_seconds(&self) -> i64;

    /// Get server mode (dev, hybrid, prod, etc.)
    fn get_server_mode(&self) -> &str;

    /// Get optional last reload timestamp (RFC3339 format)
    fn get_last_reload_at(&self) -> Option<String>;

    /// Get server start time (RFC3339 format)
    fn get_server_start_time(&self) -> String;

    /// Get additional metrics as JSON value
    fn get_additional_metrics(&self) -> serde_json::Value {
        serde_json::json!({})
    }
}

/// **⚡ RELOADABLE TRAIT** - Optional trait for servers with asset reloading capability
///
/// Implement this trait alongside ServerMetrics to get automatic `/reload` endpoint
/// with validation, error handling, and async processing.
pub trait ReloadableServer: ServerMetrics {
    /// Get the public directory path for assets
    fn get_public_dir(&self) -> Option<&str>;

    /// Get the frontend state for asset loading
    fn get_frontend_state(&self) -> std::sync::Arc<tokio::sync::RwLock<crate::frontend::FrontendState>>;

    /// Get the virtual host ID for assets (e.g., "blog", "app")
    fn get_virtual_host_id(&self) -> &str { "app" }

    /// Get the base path for assets (usually "/")
    fn get_base_path(&self) -> &str { "/" }

    /// Update the last reload timestamp
    fn update_reload_timestamp(&self);

    /// Get the development reload token (if configured)
    ///
    /// This enables simplified hot-reload in development mode via X-Reload-Token header.
    /// Returns None in production mode (default).
    ///
    /// ⚠️ **WARNING**: This is for DEVELOPMENT ONLY! Do not use in production!
    fn get_dev_reload_token(&self) -> Option<&str> { None }
}

/// Build a standardized status response from server metrics
pub fn build_standard_status<T: ServerMetrics>(server: &T) -> Resp {
    use chrono::Utc;
    use serde_json::json;

    let uptime_seconds = server.get_uptime_seconds();
    let uptime_formatted = format_uptime(uptime_seconds);

    let mut status_data = json!({
        "status": "running",
        "mode": server.get_server_mode(),
        "last_reload_at": server.get_last_reload_at(),
        "uptime": uptime_formatted,
        "uptime_seconds": uptime_seconds,
        "server_start_time": server.get_server_start_time(),
        "timestamp": Utc::now().to_rfc3339()
    });

    // Merge additional metrics
    let additional = server.get_additional_metrics();
    if let serde_json::Value::Object(additional_map) = additional {
        if let serde_json::Value::Object(ref mut status_map) = status_data {
            for (key, value) in additional_map {
                status_map.insert(key, value);
            }
        }
    }

    build_status_response(status_data)
}

/// Configuration for automatic admin endpoints
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoAdminConfig {
    /// Enable automatic /admin/status endpoint
    pub enable_status: bool,
    /// Enable automatic /admin/health endpoint
    pub enable_health: bool,
    /// Enable automatic /admin/info endpoint
    pub enable_info: bool,
    /// Enable automatic /admin/reload endpoint (requires ReloadableServer trait)
    pub enable_reload: bool,
    /// Custom admin path prefix (default: "/admin")
    pub admin_prefix: String,
}

impl Default for AutoAdminConfig {
    fn default() -> Self {
        Self {
            enable_status: true,
            enable_health: true,
            enable_info: true,
            enable_reload: false,  // Disabled by default - requires ReloadableServer trait
            admin_prefix: "/admin".to_string(),
        }
    }
}

/// Automatic admin endpoint handler - handles standard admin endpoints automatically
pub async fn handle_auto_admin_endpoints<T>(
    method: &hyper::Method,
    path: &str,
    req: &Req,
    server: &T,
    config: &AutoAdminConfig,
    firewall: Option<&Firewall>,
) -> Option<Resp>
where
    T: ServerMetrics,
{
    // Check if this is an admin path we should handle
    if !path.starts_with(&config.admin_prefix) {
        return None;
    }

    // Check firewall protection
    let fake_addr: Option<SocketAddr> = "127.0.0.1:0".parse().ok();
    if let Err(forbidden_response) = check_admin_firewall(firewall, method, path, req, fake_addr).await {
        return Some(forbidden_response);
    }

    // Handle specific auto endpoints
    match (method, path) {
        // GET endpoints
        (&hyper::Method::GET, p) if config.enable_status && p.ends_with("/status") => {
            Some(build_standard_status(server))
        }
        (&hyper::Method::GET, p) if config.enable_health && p.ends_with("/health") => {
            Some(build_health_response(server))
        }
        (&hyper::Method::GET, p) if config.enable_info && p.ends_with("/info") => {
            Some(build_info_response(server))
        }
        // POST endpoints - reload endpoint handled separately due to trait bounds
        (&hyper::Method::POST, p) if config.enable_reload && p.ends_with("/reload") => {
            // This requires ReloadableServer trait - return None to indicate not handled here
            // The caller should use handle_auto_reload_endpoint directly if needed
            None
        }
        _ => None, // Not an auto-handled endpoint
    }
}

/// Build a health check response (always 200 OK if server is running)
pub fn build_health_response<T: ServerMetrics>(server: &T) -> Resp {
    use chrono::Utc;
    use serde_json::json;

    let health_data = json!({
        "health": "ok",
        "status": "running",
        "mode": server.get_server_mode(),
        "uptime_seconds": server.get_uptime_seconds(),
        "timestamp": Utc::now().to_rfc3339(),
        "checks": {
            "server": "pass",
            "uptime": "pass"
        }
    });

    build_status_response(health_data)
}

/// Build an info response with basic server information
pub fn build_info_response<T: ServerMetrics>(server: &T) -> Resp {
    use chrono::Utc;
    use serde_json::json;

    let info_data = json!({
        "name": "Lithair Server",
        "version": env!("CARGO_PKG_VERSION"),
        "mode": server.get_server_mode(),
        "server_start_time": server.get_server_start_time(),
        "uptime": format_uptime(server.get_uptime_seconds()),
        "framework": "lithair-core",
        "timestamp": Utc::now().to_rfc3339()
    });

    build_status_response(info_data)
}

/// Handle automatic reload endpoint for servers implementing ReloadableServer trait
pub async fn handle_auto_reload_endpoint<T>(server: &T) -> Resp
where
    T: ReloadableServer,
{
    use chrono::Utc;
    use serde_json::json;
    use crate::http::utils::load_assets_with_logging;

    log::info!("🔄 Admin auto reload request");

    // Pre-flight validation
    match server.get_public_dir() {
        None => {
            log::warn!("⚠️ Reload attempted but no public directory configured");
            hyper::Response::builder()
                .status(hyper::StatusCode::BAD_REQUEST)
                .header("Content-Type", "application/json")
                .body(body_from(json!({
                    "error": "reload_failed",
                    "message": "No public directory configured for asset serving",
                    "details": "Server was started without a valid public directory",
                    "timestamp": Utc::now().to_rfc3339()
                }).to_string()))
                .unwrap()
        }
        Some(public_dir) => {
            // Additional validation: check if directory exists
            if !std::path::Path::new(public_dir).exists() {
                log::error!("❌ Public directory does not exist: {}", public_dir);
                hyper::Response::builder()
                    .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                    .header("Content-Type", "application/json")
                    .body(body_from(json!({
                        "error": "directory_not_found",
                        "message": format!("Public directory '{}' does not exist", public_dir),
                        "details": "The configured public directory was removed or is inaccessible",
                        "timestamp": Utc::now().to_rfc3339()
                    }).to_string()))
                    .unwrap()
            } else if !std::path::Path::new(public_dir).is_dir() {
                log::error!("❌ Public path is not a directory: {}", public_dir);
                hyper::Response::builder()
                    .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                    .header("Content-Type", "application/json")
                    .body(body_from(json!({
                        "error": "invalid_directory",
                        "message": format!("Public path '{}' is not a directory", public_dir),
                        "details": "The configured public path exists but is not a directory",
                        "timestamp": Utc::now().to_rfc3339()
                    }).to_string()))
                    .unwrap()
            } else {
                // Directory is valid, proceed with reload
                let frontend_state = server.get_frontend_state();
                let virtual_host_id = server.get_virtual_host_id().to_string();
                let base_path = server.get_base_path().to_string();
                let public_dir_owned = public_dir.to_string();

                // Update last reload timestamp before spawning
                server.update_reload_timestamp();

                tokio::spawn(async move {
                    let reload_start = Utc::now();
                    log::info!("🔄 Starting assets reload from {}", public_dir_owned);

                    match load_assets_with_logging(
                        frontend_state,
                        &virtual_host_id,
                        &base_path,
                        &public_dir_owned,
                        "Assets (Auto Reload)"
                    ).await {
                        Ok(count) => {
                            let duration = Utc::now().signed_duration_since(reload_start);
                            log::info!("⚡ Auto reload completed: {} assets in {}ms",
                                count, duration.num_milliseconds());
                        }
                        Err(e) => {
                            log::error!("❌ Auto reload failed: {}", e);
                        }
                    }
                });

                hyper::Response::builder()
                    .status(hyper::StatusCode::OK)
                    .header("Content-Type", "application/json")
                    .body(body_from(json!({
                        "status": "initiated",
                        "message": "Assets reload initiated successfully",
                        "public_dir": public_dir,
                        "note": "Check server logs for actual reload results",
                        "timestamp": Utc::now().to_rfc3339()
                    }).to_string()))
                    .unwrap()
            }
        }
    }
}

/// Handle auto admin endpoints with reload support for ReloadableServer implementations
pub async fn handle_auto_admin_endpoints_with_reload<T>(
    method: &hyper::Method,
    path: &str,
    req: &Req,
    server: &T,
    config: &AutoAdminConfig,
    firewall: Option<&Firewall>,
) -> Option<Resp>
where
    T: ReloadableServer,
{
    // Check if this is an admin path we should handle
    if !path.starts_with(&config.admin_prefix) {
        return None;
    }

    // Check firewall protection
    let fake_addr: Option<std::net::SocketAddr> = "127.0.0.1:0".parse().ok();
    if let Err(forbidden_response) = check_admin_firewall(firewall, method, path, req, fake_addr).await {
        return Some(forbidden_response);
    }

    // Handle specific auto endpoints
    match (method, path) {
        // GET endpoints
        (&hyper::Method::GET, p) if config.enable_status && p.ends_with("/status") => {
            Some(build_standard_status(server))
        }
        (&hyper::Method::GET, p) if config.enable_health && p.ends_with("/health") => {
            Some(build_health_response(server))
        }
        (&hyper::Method::GET, p) if config.enable_info && p.ends_with("/info") => {
            Some(build_info_response(server))
        }
        // POST endpoints
        (&hyper::Method::POST, p) if config.enable_reload && p.ends_with("/reload") => {
            // Check for dev reload token if configured
            if let Some(dev_token) = server.get_dev_reload_token() {
                // Dev mode: check X-Reload-Token header
                if let Some(provided_token) = req.headers().get("X-Reload-Token") {
                    if let Ok(provided_str) = provided_token.to_str() {
                        if provided_str == dev_token {
                            log::info!("🔧 Dev reload token validated");
                            Some(handle_auto_reload_endpoint(server).await)
                        } else {
                            log::warn!("⚠️ Invalid dev reload token provided");
                            Some(hyper::Response::builder()
                                .status(hyper::StatusCode::UNAUTHORIZED)
                                .header("Content-Type", "application/json")
                                .body(body_from(serde_json::json!({
                                    "error": "invalid_token",
                                    "message": "Invalid reload token"
                                }).to_string()))
                                .unwrap())
                        }
                    } else {
                        Some(hyper::Response::builder()
                            .status(hyper::StatusCode::BAD_REQUEST)
                            .header("Content-Type", "application/json")
                            .body(body_from(serde_json::json!({
                                "error": "invalid_header",
                                "message": "X-Reload-Token header contains invalid characters"
                            }).to_string()))
                            .unwrap())
                    }
                } else {
                    log::warn!("⚠️ Dev reload token configured but X-Reload-Token header missing");
                    Some(hyper::Response::builder()
                        .status(hyper::StatusCode::UNAUTHORIZED)
                        .header("Content-Type", "application/json")
                        .body(body_from(serde_json::json!({
                            "error": "missing_token",
                            "message": "X-Reload-Token header required (dev mode enabled)"
                        }).to_string()))
                        .unwrap())
                }
            } else {
                // Production mode: requires proper authentication (handled by route guard)
                Some(handle_auto_reload_endpoint(server).await)
            }
        }
        _ => None, // Not an auto-handled endpoint
    }
}

/// Generic admin handler that combines automatic and custom endpoints with firewall protection
///
/// This function provides a complete pattern for admin endpoint handling:
/// 1. Try automatic admin endpoints first (status, health, info)
/// 2. If not handled automatically, check firewall protection for custom endpoints
/// 3. Call custom handler function if provided
///
/// # Arguments
/// * `method` - HTTP method
/// * `path` - Request path
/// * `req` - HTTP request
/// * `server` - Server implementing ServerMetrics
/// * `config` - Automatic admin configuration
/// * `firewall` - Optional firewall protection
/// * `custom_handler` - Optional function to handle custom admin endpoints
///
/// # Returns
/// HTTP response or None if no endpoint matched
pub async fn handle_admin_with_custom<T, F, Fut>(
    method: &hyper::Method,
    path: &str,
    req: &Req,
    server: &T,
    config: &AutoAdminConfig,
    firewall: Option<&Firewall>,
    custom_handler: Option<F>,
) -> Option<Resp>
where
    T: ServerMetrics,
    F: FnOnce(&hyper::Method, &str, &Req) -> Fut,
    Fut: std::future::Future<Output = Option<Resp>>,
{
    // Check if this is an admin path we should handle
    if !path.starts_with(&config.admin_prefix) {
        return None;
    }

    // First, try automatic admin endpoints
    if let Some(auto_response) = handle_auto_admin_endpoints(method, path, req, server, config, firewall).await {
        return Some(auto_response);
    }

    // If not handled by auto endpoints and we have a custom handler
    if let Some(handler_fn) = custom_handler {
        // Check firewall protection for custom endpoints
        let fake_addr: Option<std::net::SocketAddr> = "127.0.0.1:0".parse().ok();
        if let Err(forbidden_response) = check_admin_firewall(firewall, method, path, req, fake_addr).await {
            return Some(forbidden_response);
        }

        // Call custom handler
        return handler_fn(method, path, req).await;
    }

    None
}

/// Complete admin management handler with automatic endpoints and custom handler fallback
///
/// This function provides the complete Lithair admin management pattern:
/// 1. Try automatic admin endpoints first (status, health, info, reload if ReloadableServer)
/// 2. If not handled automatically and path matches admin prefix, check firewall for custom endpoints
/// 3. Call custom handler function if provided
/// 4. Return 404 if path doesn't match admin prefix
///
/// # Arguments
/// * `method` - HTTP method
/// * `path` - Request path
/// * `req` - HTTP request
/// * `server` - Server implementing ReloadableServer (for full automatic support)
/// * `config` - Automatic admin configuration
/// * `firewall` - Optional firewall protection
/// * `custom_handler` - Optional function to handle custom admin endpoints
///
/// # Returns
/// HTTP response for the admin request
pub async fn handle_complete_admin_management<T, F, Fut>(
    method: &hyper::Method,
    path: &str,
    req: &Req,
    server: &T,
    config: &AutoAdminConfig,
    firewall: Option<&Firewall>,
    custom_handler: Option<F>,
) -> Resp
where
    T: ReloadableServer,
    F: FnOnce(&hyper::Method, &str, &Req) -> Fut,
    Fut: std::future::Future<Output = Option<Resp>>,
{
    // First try automatic admin endpoints with reload support
    if let Some(auto_response) = handle_auto_admin_endpoints_with_reload(
        method, path, req, server, config, firewall
    ).await {
        return auto_response;
    }

    // Then try custom admin endpoints with firewall protection
    if path.starts_with(&config.admin_prefix) {
        // Check firewall protection for custom endpoints
        let fake_addr: Option<std::net::SocketAddr> = "127.0.0.1:0".parse().ok();
        if let Err(forbidden_response) = check_admin_firewall(
            firewall, method, path, req, fake_addr
        ).await {
            return forbidden_response;
        }

        // Handle custom endpoints
        if let Some(handler) = custom_handler {
            if let Some(custom_response) = handler(method, path, req).await {
                return custom_response;
            }
        }
    }

    // If path doesn't match admin prefix, return not found
    not_found_response("Admin endpoint")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_uptime() {
        assert_eq!(format_uptime(45), "45s");
        assert_eq!(format_uptime(90), "1m30s");
        assert_eq!(format_uptime(3661), "1h1m1s");
        assert_eq!(format_uptime(7200), "2h0m0s");
    }

    struct MockServer {
        uptime: i64,
        mode: &'static str,
    }

    impl ServerMetrics for MockServer {
        fn get_uptime_seconds(&self) -> i64 { self.uptime }
        fn get_server_mode(&self) -> &str { self.mode }
        fn get_last_reload_at(&self) -> Option<String> { None }
        fn get_server_start_time(&self) -> String { "2025-01-01T00:00:00Z".to_string() }
    }

    #[test]
    fn test_build_standard_status() {
        let mock_server = MockServer {
            uptime: 3661,
            mode: "test",
        };

        let response = build_standard_status(&mock_server);
        assert_eq!(response.status(), StatusCode::OK);
    }

    /* #[tokio::test]
    async fn test_check_admin_firewall_no_firewall() {
        let req = Request::builder()
            .uri("/admin/test")
            .body(http_body_util::Empty::<bytes::Bytes>::new().map_err(|e| match e {}))
            .unwrap();

        let result = check_admin_firewall(None, &Method::GET, "/admin/test", &req, None).await;
        assert!(result.is_ok());
    } */
}
