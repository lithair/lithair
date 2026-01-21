//! Utility functions and types for proxy operations

use std::fmt;

/// Error types for proxy operations
#[derive(Debug)]
pub enum ProxyError {
    /// Request was blocked by filter
    Blocked { reason: String },

    /// Upstream connection failed
    UpstreamError { message: String },

    /// Invalid request
    InvalidRequest { message: String },

    /// Authentication required
    AuthRequired,

    /// Rate limit exceeded
    RateLimitExceeded,

    /// Generic error
    Other(Box<dyn std::error::Error + Send + Sync>),
}

impl fmt::Display for ProxyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProxyError::Blocked { reason } => write!(f, "Request blocked: {}", reason),
            ProxyError::UpstreamError { message } => write!(f, "Upstream error: {}", message),
            ProxyError::InvalidRequest { message } => write!(f, "Invalid request: {}", message),
            ProxyError::AuthRequired => write!(f, "Authentication required"),
            ProxyError::RateLimitExceeded => write!(f, "Rate limit exceeded"),
            ProxyError::Other(e) => write!(f, "Proxy error: {}", e),
        }
    }
}

impl std::error::Error for ProxyError {}

/// Result type for proxy operations
pub type ProxyResult<T> = Result<T, ProxyError>;

/// Helper to extract client IP from request
pub fn extract_client_ip<B>(req: &hyper::Request<B>) -> Option<String> {
    // Try X-Forwarded-For first
    if let Some(forwarded) = req.headers().get("x-forwarded-for") {
        if let Ok(value) = forwarded.to_str() {
            return Some(value.split(',').next()?.trim().to_string());
        }
    }

    // Try X-Real-IP
    if let Some(real_ip) = req.headers().get("x-real-ip") {
        if let Ok(value) = real_ip.to_str() {
            return Some(value.to_string());
        }
    }

    None
}

/// Helper to check if a URI matches a pattern
pub fn matches_pattern(uri: &str, pattern: &str) -> bool {
    // Simple wildcard matching for now
    // TODO: Support more complex patterns (regex, CIDR for IPs, etc.)

    if pattern == "*" {
        return true;
    }

    if pattern.contains('*') {
        let parts: Vec<&str> = pattern.split('*').collect();
        if parts.len() == 2 {
            let prefix = parts[0];
            let suffix = parts[1];
            return uri.starts_with(prefix) && uri.ends_with(suffix);
        }
    }

    uri == pattern
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matches_pattern() {
        assert!(matches_pattern("anything", "*"));
        assert!(matches_pattern("example.com", "example.com"));
        assert!(matches_pattern("api.example.com", "*.example.com"));
        assert!(matches_pattern("/api/users", "/api/*"));
        assert!(!matches_pattern("/admin/users", "/api/*"));
    }
}
