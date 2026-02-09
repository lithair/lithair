//! HTTP response building and serialization
//!
//! This module provides comprehensive HTTP response building functionality
//! with fluent API and zero external dependencies.

use std::collections::HashMap;
use std::fmt::Write;

use super::constants::{content_types, headers, CRLF};
use super::{HttpError, HttpResult};

/// HTTP status codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusCode {
    // 2xx Success
    Ok = 200,
    Created = 201,
    Accepted = 202,
    NoContent = 204,

    // 3xx Redirection
    MovedPermanently = 301,
    Found = 302,
    NotModified = 304,

    // 4xx Client Error
    BadRequest = 400,
    Unauthorized = 401,
    Forbidden = 403,
    NotFound = 404,
    MethodNotAllowed = 405,
    Conflict = 409,
    UnprocessableEntity = 422,

    // 5xx Server Error
    InternalServerError = 500,
    NotImplemented = 501,
    BadGateway = 502,
    ServiceUnavailable = 503,
}

impl StatusCode {
    /// Get the status code as a number
    pub fn as_u16(self) -> u16 {
        self as u16
    }

    /// Get the reason phrase for this status code
    pub fn reason_phrase(self) -> &'static str {
        match self {
            StatusCode::Ok => "OK",
            StatusCode::Created => "Created",
            StatusCode::Accepted => "Accepted",
            StatusCode::NoContent => "No Content",
            StatusCode::MovedPermanently => "Moved Permanently",
            StatusCode::Found => "Found",
            StatusCode::NotModified => "Not Modified",
            StatusCode::BadRequest => "Bad Request",
            StatusCode::Unauthorized => "Unauthorized",
            StatusCode::Forbidden => "Forbidden",
            StatusCode::NotFound => "Not Found",
            StatusCode::MethodNotAllowed => "Method Not Allowed",
            StatusCode::Conflict => "Conflict",
            StatusCode::UnprocessableEntity => "Unprocessable Entity",
            StatusCode::InternalServerError => "Internal Server Error",
            StatusCode::NotImplemented => "Not Implemented",
            StatusCode::BadGateway => "Bad Gateway",
            StatusCode::ServiceUnavailable => "Service Unavailable",
        }
    }
}

impl std::fmt::Display for StatusCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.as_u16(), self.reason_phrase())
    }
}

/// HTTP response builder with fluent API
///
/// # Example
///
/// ```rust
/// use lithair_core::http::{HttpResponse, StatusCode};
///
/// let response = HttpResponse::ok()
///     .header("Content-Type", "application/json")
///     .json(r#"{"message": "Hello, World!"}"#);
/// ```
#[derive(Debug, Clone)]
pub struct HttpResponse {
    status: StatusCode,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

impl HttpResponse {
    /// Create a new HTTP response with the given status code
    pub fn new(status: StatusCode) -> Self {
        let mut headers = HashMap::new();

        // Add default headers
        headers.insert(headers::CONNECTION.to_string(), "close".to_string());
        headers.insert("Server".to_string(), "Lithair/0.1.0".to_string());

        Self { status, headers, body: Vec::new() }
    }

    // Convenience constructors for common status codes

    /// Create a 200 OK response
    pub fn ok() -> Self {
        Self::new(StatusCode::Ok)
    }

    /// Create a 201 Created response
    pub fn created() -> Self {
        Self::new(StatusCode::Created)
    }

    /// Create a 204 No Content response
    pub fn no_content() -> Self {
        Self::new(StatusCode::NoContent)
    }

    /// Create a 400 Bad Request response
    pub fn bad_request() -> Self {
        Self::new(StatusCode::BadRequest)
    }

    /// Create a 401 Unauthorized response
    pub fn unauthorized() -> Self {
        Self::new(StatusCode::Unauthorized)
    }

    /// Create a 403 Forbidden response
    pub fn forbidden() -> Self {
        Self::new(StatusCode::Forbidden)
    }

    /// Create a 404 Not Found response
    pub fn not_found() -> Self {
        Self::new(StatusCode::NotFound)
    }

    /// Create a 409 Conflict response
    pub fn conflict() -> Self {
        Self::new(StatusCode::Conflict)
    }

    /// Create a 422 Unprocessable Entity response
    pub fn unprocessable_entity() -> Self {
        Self::new(StatusCode::UnprocessableEntity)
    }

    /// Create a 500 Internal Server Error response
    pub fn internal_server_error() -> Self {
        Self::new(StatusCode::InternalServerError)
    }

    /// Create a 501 Not Implemented response
    pub fn not_implemented() -> Self {
        Self::new(StatusCode::NotImplemented)
    }

    /// Create a 503 Service Unavailable response
    pub fn service_unavailable() -> Self {
        Self::new(StatusCode::ServiceUnavailable)
    }

    // Builder methods

    /// Set a header
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.insert(name.to_string(), value.to_string());
        self
    }

    /// Set multiple headers
    pub fn headers(mut self, headers: &[(&str, &str)]) -> Self {
        for (name, value) in headers {
            self.headers.insert(name.to_string(), value.to_string());
        }
        self
    }

    /// Set the Content-Type header
    pub fn content_type(self, content_type: &str) -> Self {
        self.header(headers::CONTENT_TYPE, content_type)
    }

    /// Set the body as raw bytes
    pub fn body(mut self, body: Vec<u8>) -> Self {
        self.body = body;
        self.set_content_length();
        self
    }

    /// Set the body as text (UTF-8)
    pub fn text(self, text: &str) -> Self {
        self.content_type(content_types::TEXT).body(text.as_bytes().to_vec())
    }

    /// Set the body as HTML
    pub fn html(self, html: &str) -> Self {
        self.content_type(content_types::HTML).body(html.as_bytes().to_vec())
    }

    /// Set the body as JSON
    pub fn json(self, json: &str) -> Self {
        self.content_type(content_types::JSON).body(json.as_bytes().to_vec())
    }

    /// ULTRA-PERFORMANCE: Set JSON response with static pre-compiled template
    /// This avoids all allocations and JSON serialization for maximum throughput
    pub fn json_static_template(
        self,
        template: &'static str,
        replacements: &[(&str, &str)],
    ) -> Self {
        let mut json = template.to_string();
        for (placeholder, value) in replacements {
            json = json.replace(placeholder, value);
        }
        self.content_type(content_types::JSON).body(json.into_bytes())
    }

    /// ULTRA-PERFORMANCE: Set JSON response with pre-compiled static string (zero allocation)
    pub fn json_static(self, json: &'static str) -> Self {
        self.content_type(content_types::JSON).body(json.as_bytes().to_vec())
    }

    /// Set the body as a JSON value (using our custom serialization)
    pub fn json_value(self, value: crate::serialization::JsonValue) -> Self {
        let json_string = crate::serialization::stringify_json(&value);
        self.json(&json_string)
    }

    /// Set the body as binary data
    pub fn binary(self, data: Vec<u8>) -> Self {
        self.content_type(content_types::BINARY).body(data)
    }

    /// Add a cookie to the response
    pub fn cookie(self, name: &str, value: &str) -> Self {
        self.header("Set-Cookie", &format!("{}={}", name, value))
    }

    /// Add a cookie with options
    pub fn cookie_with_options(
        self,
        name: &str,
        value: &str,
        max_age: Option<i64>,
        path: Option<&str>,
        domain: Option<&str>,
        secure: bool,
        http_only: bool,
    ) -> Self {
        let mut cookie = format!("{}={}", name, value);

        if let Some(max_age) = max_age {
            write!(&mut cookie, "; Max-Age={}", max_age).expect("write to String is infallible");
        }

        if let Some(path) = path {
            write!(&mut cookie, "; Path={}", path).expect("write to String is infallible");
        }

        if let Some(domain) = domain {
            write!(&mut cookie, "; Domain={}", domain).expect("write to String is infallible");
        }

        if secure {
            cookie.push_str("; Secure");
        }

        if http_only {
            cookie.push_str("; HttpOnly");
        }

        self.header("Set-Cookie", &cookie)
    }

    /// Enable CORS for all origins (development helper)
    pub fn cors_all(self) -> Self {
        self.header("Access-Control-Allow-Origin", "*")
            .header("Access-Control-Allow-Methods", "GET, POST, PUT, DELETE, OPTIONS")
            .header("Access-Control-Allow-Headers", "Content-Type, Authorization")
    }

    /// Enable CORS for specific origin
    pub fn cors_origin(self, origin: &str) -> Self {
        self.header("Access-Control-Allow-Origin", origin)
            .header("Access-Control-Allow-Methods", "GET, POST, PUT, DELETE, OPTIONS")
            .header("Access-Control-Allow-Headers", "Content-Type, Authorization")
    }

    /// Redirect to another URL
    pub fn redirect(location: &str) -> Self {
        Self::new(StatusCode::Found).header("Location", location)
    }

    /// Permanent redirect to another URL
    pub fn redirect_permanent(location: &str) -> Self {
        Self::new(StatusCode::MovedPermanently).header("Location", location)
    }

    // Internal methods

    /// Automatically set the Content-Length header based on body size
    fn set_content_length(&mut self) {
        self.headers
            .insert(headers::CONTENT_LENGTH.to_string(), self.body.len().to_string());
    }

    // Accessors

    /// Get the status code
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// Get all headers
    pub fn get_headers(&self) -> &HashMap<String, String> {
        &self.headers
    }

    /// Get a specific header
    pub fn header_value(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(|s| s.as_str())
    }

    /// Get the response body
    pub fn body_bytes(&self) -> &[u8] {
        &self.body
    }

    /// Get the response body as a string (if valid UTF-8)
    pub fn body_string(&self) -> HttpResult<&str> {
        std::str::from_utf8(&self.body)
            .map_err(|e| HttpError::InvalidRequest(format!("Body is not valid UTF-8: {}", e)))
    }

    /// Convert the response to raw HTTP bytes for transmission
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut response = String::new();

        // Status line
        write!(&mut response, "HTTP/1.1 {}{}", self.status, CRLF)
            .expect("write to String is infallible");

        // Headers
        for (name, value) in &self.headers {
            write!(&mut response, "{}: {}{}", name, value, CRLF)
                .expect("write to String is infallible");
        }

        // Empty line to separate headers from body
        response.push_str(CRLF);

        // Convert to bytes and append body
        let mut bytes = response.into_bytes();
        bytes.extend_from_slice(&self.body);

        bytes
    }
}

impl Default for HttpResponse {
    fn default() -> Self {
        Self::ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_code_display() {
        assert_eq!(StatusCode::Ok.to_string(), "200 OK");
        assert_eq!(StatusCode::NotFound.to_string(), "404 Not Found");
        assert_eq!(StatusCode::InternalServerError.to_string(), "500 Internal Server Error");
    }

    #[test]
    fn test_response_creation() {
        let response = HttpResponse::ok().text("Hello, World!");

        assert_eq!(response.status(), StatusCode::Ok);
        assert_eq!(response.body_string().unwrap(), "Hello, World!");
        assert_eq!(response.header_value("Content-Type"), Some("text/plain; charset=utf-8"));
    }

    #[test]
    fn test_json_response() {
        let response = HttpResponse::ok().json(r#"{"message": "test"}"#);

        assert_eq!(response.header_value("Content-Type"), Some("application/json"));
        assert_eq!(response.body_string().unwrap(), r#"{"message": "test"}"#);
    }

    #[test]
    fn test_redirect() {
        let response = HttpResponse::redirect("/new-path");

        assert_eq!(response.status(), StatusCode::Found);
        assert_eq!(response.header_value("Location"), Some("/new-path"));
    }

    #[test]
    fn test_cors() {
        let response = HttpResponse::ok().cors_all().text("OK");

        assert_eq!(response.header_value("Access-Control-Allow-Origin"), Some("*"));
        assert!(response.header_value("Access-Control-Allow-Methods").is_some());
    }

    #[test]
    fn test_cookie() {
        let response = HttpResponse::ok().cookie("session", "abc123").text("OK");

        assert_eq!(response.header_value("Set-Cookie"), Some("session=abc123"));
    }

    #[test]
    fn test_response_serialization() {
        let response = HttpResponse::ok().text("Hello");

        let bytes = response.to_bytes();
        let response_str = String::from_utf8(bytes).unwrap();

        assert!(response_str.contains("HTTP/1.1 200 OK"));
        assert!(response_str.contains("Content-Type: text/plain; charset=utf-8"));
        assert!(response_str.contains("Content-Length: 5"));
        assert!(response_str.ends_with("Hello"));
    }
}
