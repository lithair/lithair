//! Response helpers for custom route handlers.
//!
//! These functions simplify building common HTTP responses
//! so you don't need to assemble `Response::builder()` chains by hand.
//!
//! # Example
//!
//! ```rust,ignore
//! use lithair_core::app::response;
//! use http::StatusCode;
//!
//! // In a route handler:
//! Ok(response::json(StatusCode::OK, r#"{"status":"ok"}"#))
//! ```

use bytes::Bytes;
use http::StatusCode;
use http_body_util::Full;
use hyper::Response;

/// JSON response with the given status code.
pub fn json(status: StatusCode, body: impl Into<String>) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(body.into())))
        .expect("valid HTTP response")
}

/// Plain-text response with the given status code.
pub fn text(status: StatusCode, body: impl Into<String>) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "text/plain; charset=utf-8")
        .body(Full::new(Bytes::from(body.into())))
        .expect("valid HTTP response")
}

/// HTML response with the given status code.
pub fn html(status: StatusCode, body: impl Into<String>) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "text/html; charset=utf-8")
        .body(Full::new(Bytes::from(body.into())))
        .expect("valid HTTP response")
}

/// 302 redirect to the given location.
pub fn redirect(location: &str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(StatusCode::FOUND)
        .header("Location", location)
        .body(Full::new(Bytes::new()))
        .expect("valid HTTP response")
}

/// Empty-body response with the given status code.
pub fn empty(status: StatusCode) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .body(Full::new(Bytes::new()))
        .expect("valid HTTP response")
}
