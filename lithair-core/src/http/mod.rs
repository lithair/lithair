//! Custom HTTP server implementation with zero external dependencies
//!
//! This module provides a complete HTTP/1.1 server implementation built from scratch using
//! only the Rust standard library. It's designed for high performance and minimal resource usage.
//!
//! # Architecture
//!
//! - [`server`] - Main HTTP server with TCP listener
//! - [`request`] - HTTP request parsing and representation
//! - [`response`] - HTTP response building and serialization
//! - [`router`] - URL routing and method dispatch
//!
//! # Example
//!
//! ```rust,no_run
//! use lithair_core::http::{HttpServer, HttpRequest, HttpResponse, HttpMethod};
//!
//! // Create and start a basic HTTP server
//! let server = HttpServer::new();
//! // server.bind("127.0.0.1:8080")?.serve()?;
//! ```
//!
//! # Performance Goals
//!
//! - **Latency**: Sub-millisecond request processing
//! - **Throughput**: 10,000+ requests/second on commodity hardware
//! - **Memory**: Minimal allocations per request
//! - **Concurrency**: Efficient handling of thousands of connections

pub mod admin;
pub mod api_router; // Generic API routing for DeclarativeHttpHandler
pub mod async_server; // Async HTTP server with Hyper
pub mod backend;
pub mod declarative;
pub mod declarative_handlers; // Revolutionary Data-First routing system
pub mod declarative_server;
pub mod error;
pub mod firewall;
pub mod optimized_declarative; // T021 bincode optimization
pub mod request;
pub mod response;
pub mod route_guard; // Declarative route protection
pub mod router;
pub mod server;
pub mod three_tier;
pub mod ultra_performance;
pub mod url_handlers; // Direct URL-to-Function mapping system
pub mod utils;

// Re-export main types for convenience
pub use admin::{
    build_health_response, build_info_response, build_standard_status, build_status_response,
    check_admin_firewall, forbidden_admin_response, format_uptime, handle_admin_with_custom,
    handle_auto_admin_endpoints, handle_auto_admin_endpoints_with_reload,
    handle_auto_reload_endpoint, handle_complete_admin_management, AutoAdminConfig,
    ReloadableServer, ServerMetrics,
};
pub use api_router::{ApiRouter, ApiRouterStats};
pub use async_server::AsyncHttpServer;
pub use backend::{
    handle_with_segments, proxy_to_declarative_handler, BackendHandler, BackendRoute, BackendRouter,
};
pub use declarative::{DeclarativeHttpHandler, HttpExposable};
pub use declarative_handlers::{
    AdminHandlerConfig, ApiProxyConfig, CustomHandlerCallback, CustomHandlerConfig,
    CustomHandlerRegistry, DeclarativeHandlerConfig, DeclarativeHandlerSystem,
    FrontendHandlerConfig, GlobalHandlerConfig, HandlerDeclaration, HandlerType, ProxyTarget,
};
pub use declarative_server::{
    DeclarativeServe, DeclarativeServer, GzipConfig, ObserveConfig, PerfEndpointsConfig,
    ReadinessConfig, RoutePolicy,
};
pub use error as http_error;
pub use firewall::{Firewall, FirewallConfig};
pub use optimized_declarative::{OptimizedDeclarativeHttpHandler, OptimizedHttpExposable};
pub use request::{HttpMethod, HttpRequest, HttpVersion};
pub use response::{HttpResponse, StatusCode};
pub use route_guard::{GuardResult, RouteGuard, RouteGuardMatcher};
pub use router::{
    AsyncRoute, AsyncRouteHandler, CommandMessage, CommandRoute, CommandRouteHandler,
    CommandSender, EnhancedRouter, PathParams, Route, RouteHandler, Router,
};
pub use server::HttpServer;
pub use three_tier::{ThreeTierHandler, ThreeTierResult, ThreeTierRouter, ThreeTierRouterBuilder};
pub use url_handlers::{UrlHandler, UrlHandlerRegistry, UrlHandlerStats};
pub use utils::{
    body_from, extract_client_ip, extract_method_str, extract_path, internal_server_error_response,
    json_error_response, load_assets_with_logging, log_access, log_access_ip,
    method_not_allowed_response, not_found_response, parse_api_path_segments, path_matches_prefix,
    serve_dev_asset, Req, Resp, RespBody,
};

/// Result type for HTTP operations
pub type HttpResult<T> = std::result::Result<T, HttpError>;

/// HTTP-specific error types
#[derive(Debug, Clone)]
pub enum HttpError {
    /// Invalid HTTP request format
    InvalidRequest(String),
    /// Unsupported HTTP method
    UnsupportedMethod(String),
    /// Malformed URL or path
    InvalidUrl(String),
    /// Invalid HTTP headers
    InvalidHeaders(String),
    /// Request body too large
    BodyTooLarge(usize),
    /// Connection-related errors
    ConnectionError(String),
    /// Server binding or startup errors
    ServerError(String),
    /// Generic I/O errors
    IoError(String),
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpError::InvalidRequest(msg) => write!(f, "Invalid HTTP request: {}", msg),
            HttpError::UnsupportedMethod(method) => {
                write!(f, "Unsupported HTTP method: {}", method)
            }
            HttpError::InvalidUrl(url) => write!(f, "Invalid URL: {}", url),
            HttpError::InvalidHeaders(msg) => write!(f, "Invalid headers: {}", msg),
            HttpError::BodyTooLarge(size) => write!(f, "Request body too large: {} bytes", size),
            HttpError::ConnectionError(msg) => write!(f, "Connection error: {}", msg),
            HttpError::ServerError(msg) => write!(f, "Server error: {}", msg),
            HttpError::IoError(msg) => write!(f, "I/O error: {}", msg),
        }
    }
}

impl std::error::Error for HttpError {}

impl From<std::io::Error> for HttpError {
    fn from(err: std::io::Error) -> Self {
        HttpError::IoError(err.to_string())
    }
}

// Convert HTTP errors to main framework errors
impl From<HttpError> for crate::Error {
    fn from(err: HttpError) -> Self {
        crate::Error::HttpError(err.to_string())
    }
}

/// Maximum request body size (default: 16MB)
pub const DEFAULT_MAX_BODY_SIZE: usize = 16 * 1024 * 1024;

/// Default HTTP port
pub const DEFAULT_PORT: u16 = 8080;

/// HTTP/1.1 protocol constants
pub mod constants {
    /// HTTP/1.1 version string
    pub const HTTP_1_1: &str = "HTTP/1.1";

    /// Common HTTP headers
    pub mod headers {
        pub const CONTENT_TYPE: &str = "Content-Type";
        pub const CONTENT_LENGTH: &str = "Content-Length";
        pub const CONNECTION: &str = "Connection";
        pub const HOST: &str = "Host";
        pub const USER_AGENT: &str = "User-Agent";
        pub const ACCEPT: &str = "Accept";
        pub const AUTHORIZATION: &str = "Authorization";
    }

    /// Common content types
    pub mod content_types {
        pub const JSON: &str = "application/json";
        pub const HTML: &str = "text/html; charset=utf-8";
        pub const TEXT: &str = "text/plain; charset=utf-8";
        pub const BINARY: &str = "application/octet-stream";
    }

    /// HTTP line ending
    pub const CRLF: &str = "\r\n";
    pub const CRLF_BYTES: &[u8] = b"\r\n";
    pub const DOUBLE_CRLF: &str = "\r\n\r\n";
    pub const DOUBLE_CRLF_BYTES: &[u8] = b"\r\n\r\n";
}
