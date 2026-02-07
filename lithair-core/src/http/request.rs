//! HTTP request parsing and representation
//!
//! This module provides comprehensive HTTP request parsing functionality
//! built from scratch without external dependencies.

use std::collections::HashMap;
use std::str::FromStr;

use super::{HttpError, HttpResult};

/// HTTP methods supported by the server
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HttpMethod {
    GET,
    POST,
    PUT,
    DELETE,
    PATCH,
    HEAD,
    OPTIONS,
}

impl HttpMethod {
    /// Convert method to string
    pub fn as_str(&self) -> &'static str {
        match self {
            HttpMethod::GET => "GET",
            HttpMethod::POST => "POST",
            HttpMethod::PUT => "PUT",
            HttpMethod::DELETE => "DELETE",
            HttpMethod::PATCH => "PATCH",
            HttpMethod::HEAD => "HEAD",
            HttpMethod::OPTIONS => "OPTIONS",
        }
    }
}

impl FromStr for HttpMethod {
    type Err = HttpError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "GET" => Ok(HttpMethod::GET),
            "POST" => Ok(HttpMethod::POST),
            "PUT" => Ok(HttpMethod::PUT),
            "DELETE" => Ok(HttpMethod::DELETE),
            "PATCH" => Ok(HttpMethod::PATCH),
            "HEAD" => Ok(HttpMethod::HEAD),
            "OPTIONS" => Ok(HttpMethod::OPTIONS),
            _ => Err(HttpError::UnsupportedMethod(s.to_string())),
        }
    }
}

impl std::fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// HTTP version information
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpVersion {
    Http1_0,
    Http1_1,
}

impl HttpVersion {
    pub fn as_str(&self) -> &'static str {
        match self {
            HttpVersion::Http1_0 => "HTTP/1.0",
            HttpVersion::Http1_1 => "HTTP/1.1",
        }
    }
}

impl FromStr for HttpVersion {
    type Err = HttpError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "HTTP/1.0" => Ok(HttpVersion::Http1_0),
            "HTTP/1.1" => Ok(HttpVersion::Http1_1),
            _ => Err(HttpError::InvalidRequest(format!("Unsupported HTTP version: {}", s))),
        }
    }
}

impl std::fmt::Display for HttpVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Parsed query parameters from URL
pub type QueryParams = HashMap<String, String>;

/// HTTP headers collection
pub type Headers = HashMap<String, String>;

/// Represents a complete HTTP request
///
/// # Example
///
/// ```rust
/// use lithair_core::http::{HttpRequest, HttpMethod};
///
/// // Parse a simple GET request
/// let raw_request = b"GET /users?page=1 HTTP/1.1\r\nHost: localhost\r\n\r\n";
/// // let request = HttpRequest::parse(raw_request)?;
/// ```
#[derive(Debug, Clone)]
pub struct HttpRequest {
    method: HttpMethod,
    path: String,
    query_params: QueryParams,
    version: HttpVersion,
    headers: Headers,
    body: Vec<u8>,
    remote_addr: Option<std::net::SocketAddr>,
}

impl HttpRequest {
    /// Create a new HTTP request
    pub fn new(
        method: HttpMethod,
        path: String,
        version: HttpVersion,
        headers: Headers,
        body: Vec<u8>,
    ) -> Self {
        let (path, query_params) = Self::parse_path_and_query(&path);

        Self { method, path, query_params, version, headers, body, remote_addr: None }
    }

    /// Parse an HTTP request from raw bytes
    ///
    /// # Arguments
    ///
    /// * `raw_request` - The raw HTTP request bytes
    ///
    /// # Errors
    ///
    /// Returns an error if the request is malformed or unsupported
    ///
    /// # Example
    ///
    /// ```rust
    /// use lithair_core::http::HttpRequest;
    ///
    /// let raw = b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n";
    /// // let request = HttpRequest::parse(raw)?;
    /// ```
    pub fn parse(raw_request: &[u8]) -> HttpResult<Self> {
        let request_str = std::str::from_utf8(raw_request)
            .map_err(|e| HttpError::InvalidRequest(format!("Invalid UTF-8: {}", e)))?;

        // Split headers and body
        let parts: Vec<&str> = request_str.splitn(2, "\r\n\r\n").collect();
        if parts.is_empty() {
            return Err(HttpError::InvalidRequest("Empty request".to_string()));
        }

        let headers_section = parts[0];
        let body = parts.get(1).unwrap_or(&"").as_bytes().to_vec();

        // Parse headers section
        let lines: Vec<&str> = headers_section.split("\r\n").collect();
        if lines.is_empty() {
            return Err(HttpError::InvalidRequest("No request line".to_string()));
        }

        // Parse request line (first line)
        let request_line = lines[0];
        let (method, path, version) = Self::parse_request_line(request_line)?;

        // Parse headers (remaining lines)
        let headers = Self::parse_headers(&lines[1..])?;

        Ok(Self::new(method, path, version, headers, body))
    }

    /// Parse the HTTP request line (e.g., "GET /path HTTP/1.1")
    fn parse_request_line(line: &str) -> HttpResult<(HttpMethod, String, HttpVersion)> {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() != 3 {
            return Err(HttpError::InvalidRequest(format!("Invalid request line: {}", line)));
        }

        let method = parts[0].parse()?;
        let path = parts[1].to_string();
        let version = parts[2].parse()?;

        Ok((method, path, version))
    }

    /// Parse HTTP headers from lines
    fn parse_headers(lines: &[&str]) -> HttpResult<Headers> {
        let mut headers = HashMap::new();

        for line in lines {
            if line.is_empty() {
                continue;
            }

            let parts: Vec<&str> = line.splitn(2, ':').collect();
            if parts.len() != 2 {
                return Err(HttpError::InvalidHeaders(format!("Invalid header line: {}", line)));
            }

            let name = parts[0].trim().to_lowercase();
            let value = parts[1].trim().to_string();
            headers.insert(name, value);
        }

        Ok(headers)
    }

    /// Parse path and query parameters
    fn parse_path_and_query(full_path: &str) -> (String, QueryParams) {
        let parts: Vec<&str> = full_path.splitn(2, '?').collect();
        let path = parts[0].to_string();

        if parts.len() == 1 {
            return (path, HashMap::new());
        }

        let query_string = parts[1];
        let mut params = HashMap::new();

        for pair in query_string.split('&') {
            let kv: Vec<&str> = pair.splitn(2, '=').collect();
            if kv.len() == 2 {
                let key = urlcode_decode(kv[0]);
                let value = urlcode_decode(kv[1]);
                params.insert(key, value);
            } else if kv.len() == 1 && !kv[0].is_empty() {
                let key = urlcode_decode(kv[0]);
                params.insert(key, String::new());
            }
        }

        (path, params)
    }

    // Accessors

    /// Get the HTTP method
    pub fn method(&self) -> &HttpMethod {
        &self.method
    }

    /// Get the request path (without query parameters)
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Get the query parameters
    pub fn query_params(&self) -> &QueryParams {
        &self.query_params
    }

    /// Get a specific query parameter
    pub fn query_param(&self, key: &str) -> Option<&str> {
        self.query_params.get(key).map(|s| s.as_str())
    }

    /// Get the HTTP version
    pub fn version(&self) -> &HttpVersion {
        &self.version
    }

    /// Get all headers
    pub fn headers(&self) -> &Headers {
        &self.headers
    }

    /// Get a specific header value
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(&name.to_lowercase()).map(|s| s.as_str())
    }

    /// Get the request body
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// Get the request body as a string (if valid UTF-8)
    pub fn body_string(&self) -> HttpResult<&str> {
        std::str::from_utf8(&self.body)
            .map_err(|e| HttpError::InvalidRequest(format!("Body is not valid UTF-8: {}", e)))
    }

    /// Get the remote address (if available)
    pub fn remote_addr(&self) -> Option<std::net::SocketAddr> {
        self.remote_addr
    }

    /// Set the remote address
    pub fn set_remote_addr(&mut self, addr: std::net::SocketAddr) {
        self.remote_addr = Some(addr);
    }

    /// Check if this is a JSON request
    pub fn is_json(&self) -> bool {
        self.header("content-type")
            .map(|ct| ct.starts_with("application/json"))
            .unwrap_or(false)
    }

    /// Parse JSON body
    ///
    /// This will use our custom JSON parser once implemented
    pub fn json_value(&self) -> HttpResult<crate::serialization::JsonValue> {
        let body_str = self.body_string()?;
        crate::serialization::parse_json(body_str)
            .map_err(|e| HttpError::InvalidRequest(format!("Invalid JSON: {}", e)))
    }
}

/// URL decoding for query parameters (RFC 3986)
fn urlcode_decode(s: &str) -> String {
    let mut result = Vec::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                result.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                if let (Some(hi), Some(lo)) = (
                    hex_digit(bytes[i + 1]),
                    hex_digit(bytes[i + 2]),
                ) {
                    result.push((hi << 4) | lo);
                    i += 3;
                } else {
                    result.push(b'%');
                    i += 1;
                }
            }
            b => {
                result.push(b);
                i += 1;
            }
        }
    }

    String::from_utf8(result).unwrap_or_else(|_| s.to_string())
}

/// Convert a hex character to its numeric value
fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_method_parsing() {
        assert_eq!("GET".parse::<HttpMethod>().unwrap(), HttpMethod::GET);
        assert_eq!("POST".parse::<HttpMethod>().unwrap(), HttpMethod::POST);
        assert_eq!("get".parse::<HttpMethod>().unwrap(), HttpMethod::GET);
        assert!("INVALID".parse::<HttpMethod>().is_err());
    }

    #[test]
    fn test_http_version_parsing() {
        assert_eq!("HTTP/1.1".parse::<HttpVersion>().unwrap(), HttpVersion::Http1_1);
        assert_eq!("HTTP/1.0".parse::<HttpVersion>().unwrap(), HttpVersion::Http1_0);
        assert!("HTTP/2.0".parse::<HttpVersion>().is_err());
    }

    #[test]
    fn test_parse_path_and_query() {
        let (path, params) = HttpRequest::parse_path_and_query("/users?page=1&size=10");
        assert_eq!(path, "/users");
        assert_eq!(params.get("page"), Some(&"1".to_string()));
        assert_eq!(params.get("size"), Some(&"10".to_string()));
    }

    #[test]
    fn test_parse_path_without_query() {
        let (path, params) = HttpRequest::parse_path_and_query("/users");
        assert_eq!(path, "/users");
        assert!(params.is_empty());
    }

    #[test]
    fn test_request_line_parsing() {
        let (method, path, version) =
            HttpRequest::parse_request_line("GET /users HTTP/1.1").unwrap();
        assert_eq!(method, HttpMethod::GET);
        assert_eq!(path, "/users");
        assert_eq!(version, HttpVersion::Http1_1);
    }

    #[test]
    fn test_headers_parsing() {
        let lines = ["Host: localhost", "Content-Type: application/json", ""];
        let headers = HttpRequest::parse_headers(&lines).unwrap();
        assert_eq!(headers.get("host"), Some(&"localhost".to_string()));
        assert_eq!(headers.get("content-type"), Some(&"application/json".to_string()));
    }
}
