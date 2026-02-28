//! HTTP utility functions for Hyper-based servers
//!
//! Common HTTP operations that are reusable across different Lithair applications.

use bytes::Bytes;
use http_body_util::{combinators::BoxBody, BodyExt, Full};
use hyper::{Request, Response};
use std::convert::Infallible;

/// Common HTTP type aliases for consistent usage across Lithair applications
pub type RespBody = BoxBody<Bytes, Infallible>;
pub type Req = Request<hyper::body::Incoming>;
pub type Resp = Response<RespBody>;

/// Create a response body from any data that can be converted to Bytes
pub fn body_from<T: Into<Bytes>>(data: T) -> RespBody {
    Full::new(data.into()).boxed()
}

/// Extract client IP from proxy headers (`X-Forwarded-For`, `X-Real-IP`).
///
/// Returns `Some(ip)` only if a proxy header is present. Returns `None` when
/// no proxy header is found, so callers can fall back to `remote_addr`.
///
/// **Security note:** These headers are trivially spoofed by clients. Only
/// trust them when `remote_addr` is a known trusted proxy (e.g. loopback,
/// private network). See [`resolve_client_ip`] for a safe wrapper.
pub fn extract_client_ip<T>(req: &Request<T>) -> Option<String> {
    // Check X-Forwarded-For header first (for proxies)
    if let Some(forwarded) = req.headers().get("x-forwarded-for") {
        if let Ok(forwarded_str) = forwarded.to_str() {
            if let Some(first_ip) = forwarded_str.split(',').next() {
                let trimmed = first_ip.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
    }

    // Check X-Real-IP header
    if let Some(real_ip) = req.headers().get("x-real-ip") {
        if let Ok(ip_str) = real_ip.to_str() {
            let trimmed = ip_str.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }

    None
}

/// Resolve the real client IP safely.
///
/// Only trusts proxy headers (`X-Forwarded-For`, `X-Real-IP`) when
/// `remote_addr` is a loopback or private IP (i.e. a trusted reverse proxy).
/// For direct connections from the internet, returns the TCP socket IP.
pub fn resolve_client_ip<T>(req: &Request<T>, remote_addr: std::net::SocketAddr) -> String {
    let ip = remote_addr.ip();
    let is_trusted_proxy = ip.is_loopback()
        || match ip {
            std::net::IpAddr::V4(v4) => v4.is_private(),
            std::net::IpAddr::V6(_) => false,
        };

    if is_trusted_proxy {
        extract_client_ip(req).unwrap_or_else(|| ip.to_string())
    } else {
        ip.to_string()
    }
}

/// Extract path from request URI
///
/// Simple helper to get the path portion of the URI.
pub fn extract_path<T>(req: &Request<T>) -> &str {
    req.uri().path()
}

/// Check if path matches a prefix pattern
///
/// Utility for path-based routing decisions.
pub fn path_matches_prefix(path: &str, prefix: &str) -> bool {
    path.starts_with(prefix)
}

/// Extract method as string from request
///
/// Convert Hyper method to string for logging/routing.
pub fn extract_method_str<T>(req: &Request<T>) -> &str {
    req.method().as_str()
}

/// Create a JSON error response with given status code
pub fn json_error_response(
    status: hyper::StatusCode,
    error: &str,
    message: &str,
) -> Response<RespBody> {
    use serde_json::json;

    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(body_from(
            json!({
                "error": error,
                "message": message
            })
            .to_string(),
        ))
        .expect("valid HTTP response")
}

/// Create a standard 404 Not Found JSON response
pub fn not_found_response(resource: &str) -> Response<RespBody> {
    json_error_response(
        hyper::StatusCode::NOT_FOUND,
        "not_found",
        &format!("{} not found", resource),
    )
}

/// Create a standard 405 Method Not Allowed JSON response
pub fn method_not_allowed_response() -> Response<RespBody> {
    json_error_response(
        hyper::StatusCode::METHOD_NOT_ALLOWED,
        "method_not_allowed",
        "HTTP method not allowed for this endpoint",
    )
}

/// Create a standard 500 Internal Server Error JSON response
pub fn internal_server_error_response(context: &str) -> Response<RespBody> {
    json_error_response(
        hyper::StatusCode::INTERNAL_SERVER_ERROR,
        "internal_server_error",
        &format!("Internal server error: {}", context),
    )
}

/// Parse API path segments after a prefix
///
/// Example: `/api/articles/123/comments` with prefix `/api/articles`
/// returns `vec!["123", "comments"]`
pub fn parse_api_path_segments<'a>(path: &'a str, prefix: &str) -> Vec<&'a str> {
    path.strip_prefix(prefix)
        .unwrap_or("")
        .trim_start_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect()
}

/// Serve static assets from disk for development mode (hot-reload)
///
/// This function provides disk-based asset serving for development mode,
/// where assets need to be served fresh on each request for hot-reload functionality.
///
/// # Arguments
/// * `path` - The request path (e.g., "/", "/style.css")
/// * `public_dir` - Directory containing static assets
/// * `default_file` - Default file to serve for "/" requests (usually "index.html")
///
/// # Returns
/// HTTP response with the requested asset or 404 if not found
pub async fn serve_dev_asset(path: &str, public_dir: &str, default_file: &str) -> Resp {
    // Clean path and handle root
    let clean_path = if path == "/" { format!("/{}", default_file) } else { path.to_string() };

    let file_path = format!("{}{}", public_dir, clean_path);

    match tokio::fs::read(&file_path).await {
        Ok(content) => {
            let mime_type = mime_guess::from_path(&file_path).first_or_octet_stream().to_string();

            log::debug!("[DEV] Serving {} from disk ({} bytes)", path, content.len());

            hyper::Response::builder()
                .status(hyper::StatusCode::OK)
                .header("Content-Type", mime_type)
                .header("X-Served-From", "Disk-Dev-Mode")
                .header("Cache-Control", "no-cache") // No cache in dev
                .body(body_from(content))
                .expect("valid HTTP response")
        }
        Err(_) => not_found_response("asset"),
    }
}

/// Generic asset loading helper for Lithair applications
///
/// This function provides a reusable pattern for loading assets into memory
/// using the core virtual host system. It's designed to be called during
/// server initialization for production and hybrid modes.
///
/// # Arguments
/// * `frontend_state` - The frontend state to load assets into
/// * `virtual_host_id` - Identifier for the virtual host (e.g., "blog", "api")
/// * `base_path` - Base path for serving assets (usually "/")
/// * `public_dir` - Directory containing assets to load
/// * `context_name` - Human-readable context name for logging
///
/// # Returns
/// Number of assets loaded or error details
///
/// # Example Usage
/// ```rust
/// use lithair_core::http::load_assets_with_logging;
/// use lithair_core::frontend::FrontendState;
/// use tokio::sync::RwLock;
/// use std::sync::Arc;
///
/// let frontend_state = Arc::new(RwLock::new(FrontendState::default()));
/// let result = load_assets_with_logging(
///     frontend_state,
///     "blog",
///     "/",
///     "public",
///     "Blog Assets"
/// ).await;
/// ```
pub async fn load_assets_with_logging(
    frontend_state: std::sync::Arc<tokio::sync::RwLock<crate::frontend::FrontendState>>,
    virtual_host_id: &str,
    base_path: &str,
    public_dir: &str,
    context_name: &str,
) -> Result<usize, String> {
    log::info!("Loading {} from {}...", context_name, public_dir);

    // Use core load function for memory-first serving
    match crate::frontend::load_static_directory_to_memory(
        frontend_state,
        virtual_host_id,
        base_path,
        public_dir,
    )
    .await
    {
        Ok(count) => {
            log::info!(
                "[{}] {} assets loaded from {} directory",
                virtual_host_id,
                count,
                public_dir
            );
            Ok(count)
        }
        Err(e) => {
            let error_msg = format!("Could not load assets from {}: {}", public_dir, e);
            log::warn!("{}", error_msg);
            Err(error_msg)
        }
    }
}

/// Escape a string for safe inclusion in a JSON value.
///
/// Handles `"`, `\`, and control characters (U+0000–U+001F).
fn escape_json_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                use std::fmt::Write;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

/// Log an HTTP access entry in structured JSON format.
///
/// Generic over the response body type — only reads status and headers.
/// Used by both `LithairServer` and `DeclarativeServer`.
///
/// The `remote` parameter is the TCP socket address. When behind a reverse
/// proxy, use [`log_access_ip`] instead to pass the real client IP extracted
/// from `X-Forwarded-For` / `X-Real-IP` headers.
pub fn log_access<B>(
    remote: Option<std::net::SocketAddr>,
    method: &str,
    path: &str,
    resp: &Response<B>,
    start: std::time::Instant,
) {
    let remote_ip = remote.map(|r| r.ip().to_string()).unwrap_or_else(|| "-".into());
    log_access_ip(&remote_ip, method, path, resp, start);
}

/// Log an HTTP access entry using a pre-resolved client IP string.
///
/// Prefer this over [`log_access`] when the real client IP has already been
/// extracted from proxy headers (`X-Forwarded-For`, `X-Real-IP`).
pub fn log_access_ip<B>(
    remote_ip: &str,
    method: &str,
    path: &str,
    resp: &Response<B>,
    start: std::time::Instant,
) {
    let status = resp.status().as_u16();
    let headers = resp.headers();
    let len = headers.get("content-length").and_then(|v| v.to_str().ok()).unwrap_or("-");
    let enc = headers.get("content-encoding").and_then(|v| v.to_str().ok()).unwrap_or("-");
    let dur_ms = start.elapsed().as_millis();
    log::info!(
        "{{\"remote\":\"{}\",\"method\":\"{}\",\"path\":\"{}\",\"status\":{},\"len\":\"{}\",\"enc\":\"{}\",\"dur_ms\":{}}}",
        escape_json_value(remote_ip),
        escape_json_value(method),
        escape_json_value(path),
        status,
        escape_json_value(len),
        escape_json_value(enc),
        dur_ms
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_client_ip_from_x_forwarded_for() {
        let req = Request::builder()
            .header("x-forwarded-for", "192.168.1.100, 10.0.0.1")
            .body(())
            .unwrap();

        assert_eq!(extract_client_ip(&req), Some("192.168.1.100".to_string()));
    }

    #[test]
    fn test_extract_client_ip_from_x_real_ip() {
        let req = Request::builder().header("x-real-ip", "203.0.113.42").body(()).unwrap();

        assert_eq!(extract_client_ip(&req), Some("203.0.113.42".to_string()));
    }

    #[test]
    fn test_extract_client_ip_no_headers() {
        let req = Request::builder().body(()).unwrap();

        assert_eq!(extract_client_ip(&req), None);
    }

    #[test]
    fn test_resolve_client_ip_trusted_proxy() {
        let req = Request::builder().header("x-forwarded-for", "203.0.113.42").body(()).unwrap();
        let loopback: std::net::SocketAddr = "127.0.0.1:12345".parse().unwrap();

        // Loopback is trusted → use proxy header
        assert_eq!(resolve_client_ip(&req, loopback), "203.0.113.42");
    }

    #[test]
    fn test_resolve_client_ip_untrusted_direct() {
        let req = Request::builder().header("x-forwarded-for", "spoofed.ip").body(()).unwrap();
        let public: std::net::SocketAddr = "82.67.19.159:54321".parse().unwrap();

        // Public IP is NOT trusted → ignore proxy header, use socket IP
        assert_eq!(resolve_client_ip(&req, public), "82.67.19.159");
    }

    #[test]
    fn test_resolve_client_ip_private_network() {
        let req = Request::builder().header("x-real-ip", "198.51.100.1").body(()).unwrap();
        let private: std::net::SocketAddr = "192.168.1.50:8080".parse().unwrap();

        // Private IP is trusted → use proxy header
        assert_eq!(resolve_client_ip(&req, private), "198.51.100.1");
    }

    #[test]
    fn test_path_matches_prefix() {
        assert!(path_matches_prefix("/admin/sites/status", "/admin"));
        assert!(path_matches_prefix("/api/articles", "/api"));
        assert!(!path_matches_prefix("/about", "/admin"));
    }

    #[test]
    fn test_escape_json_value_clean() {
        assert_eq!(escape_json_value("/api/products"), "/api/products");
        assert_eq!(escape_json_value("GET"), "GET");
    }

    #[test]
    fn test_escape_json_value_quotes_and_backslash() {
        assert_eq!(escape_json_value(r#"path/"with"quotes"#), r#"path/\"with\"quotes"#);
        assert_eq!(escape_json_value(r"back\slash"), r"back\\slash");
    }

    #[test]
    fn test_escape_json_value_control_chars() {
        assert_eq!(escape_json_value("line\nnew"), r"line\nnew");
        assert_eq!(escape_json_value("tab\there"), r"tab\there");
        assert_eq!(escape_json_value("cr\rhere"), r"cr\rhere");
        // Null byte
        assert_eq!(escape_json_value("null\0byte"), r"null\u0000byte");
    }
}
