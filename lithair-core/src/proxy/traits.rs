//! Core traits for proxy functionality

use hyper::{Request, Response};
use http_body_util::combinators::BoxBody;
use bytes::Bytes;
use std::future::Future;
use std::pin::Pin;

// Type alias for body
type Body = BoxBody<Bytes, hyper::Error>;

/// Result type for proxy operations
pub type ProxyResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Trait for handling proxy requests
/// 
/// This trait provides the core interface for all proxy types (forward, reverse, transparent).
/// Implementations should handle request validation, filtering, and forwarding.
pub trait ProxyHandler: Send + Sync {
    /// Handle an incoming proxy request
    /// 
    /// # Arguments
    /// * `req` - The incoming HTTP request
    /// 
    /// # Returns
    /// A future that resolves to the proxy response
    fn handle_request(
        &self,
        req: Request<Body>,
    ) -> Pin<Box<dyn Future<Output = ProxyResult<Response<Body>>> + Send + 'static>>;

    /// Check if a request should be blocked
    /// 
    /// # Arguments
    /// * `req` - The request to check
    /// 
    /// # Returns
    /// `true` if the request should be blocked, `false` otherwise
    fn should_block(
        &self,
        req: &Request<Body>,
    ) -> Pin<Box<dyn Future<Output = ProxyResult<bool>> + Send + 'static>>;

    /// Get the proxy type name (for logging/metrics)
    fn proxy_type(&self) -> &'static str;
}

/// Metadata about a proxy request
#[derive(Debug, Clone)]
pub struct ProxyRequest {
    pub source_ip: String,
    pub method: String,
    pub uri: String,
    pub headers: Vec<(String, String)>,
}

/// Metadata about a proxy response
#[derive(Debug, Clone)]
pub struct ProxyResponse {
    pub status_code: u16,
    pub duration_ms: u128,
    pub bytes_transferred: u64,
}
