//! HTTP server implementation using std::net::TcpListener
//!
//! This module implements a high-performance HTTP server from scratch using only
//! the Rust standard library. It's designed to be the foundation for Lithair's
//! zero-dependency architecture.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use super::{HttpError, HttpRequest, HttpResponse, HttpResult, Router};

/// Configuration for the HTTP server
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Maximum number of concurrent connections
    pub max_connections: usize,
    /// Read timeout for client connections
    pub read_timeout: Option<Duration>,
    /// Write timeout for client connections
    pub write_timeout: Option<Duration>,
    /// Maximum request body size
    pub max_body_size: usize,
    /// Whether to enable keep-alive connections
    pub keep_alive: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            max_connections: 1000,
            read_timeout: Some(Duration::from_secs(30)),
            write_timeout: Some(Duration::from_secs(30)),
            max_body_size: super::DEFAULT_MAX_BODY_SIZE,
            keep_alive: true,
        }
    }
}

/// High-performance HTTP server with zero external dependencies
///
/// # Example
///
/// ```rust,no_run
/// use lithair_core::http::{HttpServer, HttpRequest, HttpResponse};
///
/// let server = HttpServer::new();
/// // server.bind("127.0.0.1:8080")?.serve()?;
/// ```
pub struct HttpServer {
    config: ServerConfig,
    router: Option<Arc<Router>>,
}

impl HttpServer {
    /// Create a new HTTP server with default configuration
    pub fn new() -> Self {
        Self { config: ServerConfig::default(), router: None }
    }

    /// Create a new HTTP server with custom configuration
    pub fn with_config(config: ServerConfig) -> Self {
        Self { config, router: None }
    }

    /// Set the router for handling requests
    pub fn with_router(mut self, router: Router) -> Self {
        self.router = Some(Arc::new(router));
        self
    }

    /// Bind to an address and start serving requests
    ///
    /// This method will block the current thread and handle incoming connections.
    ///
    /// # Arguments
    ///
    /// * `addr` - The address to bind to (e.g., "127.0.0.1:8080")
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The address is invalid or already in use
    /// - The server fails to accept connections
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use lithair_core::http::HttpServer;
    ///
    /// let server = HttpServer::new();
    /// server.serve("127.0.0.1:8080")?;
    /// # Ok::<(), lithair_core::http::HttpError>(())
    /// ```
    pub fn serve(&self, addr: &str) -> HttpResult<()> {
        let listener = TcpListener::bind(addr)
            .map_err(|e| HttpError::ServerError(format!("Failed to bind to {}: {}", addr, e)))?;

        println!("🌐 HTTP server listening on {}", addr);
        println!("📊 Max connections: {}", self.config.max_connections);
        println!("🔧 Keep-alive: {}", self.config.keep_alive);

        // Clone configuration and router for sharing between threads
        let config = self.config.clone();
        let router = self.router.clone();

        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let config = config.clone();
                    let router = router.clone();

                    // Handle each connection in a separate thread
                    // TODO: Consider using a thread pool for better performance
                    thread::spawn(move || {
                        if let Err(e) = handle_connection(stream, &config, router.as_deref()) {
                            eprintln!("❌ Error handling connection: {}", e);
                        }
                    });
                }
                Err(e) => {
                    eprintln!("❌ Failed to accept connection: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Get the server configuration
    pub fn config(&self) -> &ServerConfig {
        &self.config
    }
}

impl Default for HttpServer {
    fn default() -> Self {
        Self::new()
    }
}

/// Handle a single client connection
fn handle_connection(
    mut stream: TcpStream,
    config: &ServerConfig,
    router: Option<&Router>,
) -> HttpResult<()> {
    // Set timeouts
    stream
        .set_read_timeout(config.read_timeout)
        .map_err(|e| HttpError::ConnectionError(format!("Failed to set read timeout: {}", e)))?;
    stream
        .set_write_timeout(config.write_timeout)
        .map_err(|e| HttpError::ConnectionError(format!("Failed to set write timeout: {}", e)))?;

    let peer_addr = stream
        .peer_addr()
        .map_err(|e| HttpError::ConnectionError(format!("Failed to get peer address: {}", e)))?;

    // println!("🔗 New connection from {}", peer_addr); // Disabled for performance

    // Handle keep-alive connections
    loop {
        match handle_request(&mut stream, config, router) {
            Ok(should_close) => {
                if should_close || !config.keep_alive {
                    break;
                }
            }
            Err(e) => {
                // Downgrade noisy logs: ignore empty keep-alive reads or spurious empty requests
                match e {
                    HttpError::InvalidRequest(ref msg) if msg == "Empty request" => {
                        // Silently close without logging an error or sending a 500
                        break;
                    }
                    _ => {
                        eprintln!("❌ Error handling request from {}: {}", peer_addr, e);
                        // Try to send an error response
                        let error_response =
                            HttpResponse::internal_server_error().text("Internal Server Error");
                        let _ = send_response(&mut stream, &error_response);
                        break;
                    }
                }
            }
        }
    }

    // println!("👋 Connection closed: {}", peer_addr); // Disabled for performance
    Ok(())
}

/// Handle a single HTTP request
///
/// Returns `Ok(true)` if the connection should be closed, `Ok(false)` if it should be kept alive
fn handle_request(
    stream: &mut TcpStream,
    config: &ServerConfig,
    router: Option<&Router>,
) -> HttpResult<bool> {
    // Parse the HTTP request
    let request = parse_request(stream, config)?;

    // println!(
    //     "📨 {} {} from {}",
    //     request.method(),
    //     request.path(),
    //     stream
    //         .peer_addr()
    //         .unwrap_or_else(|_| "unknown".parse().unwrap())
    // );

    // Route the request and generate response
    let response = if let Some(router) = router {
        // Use the async router to handle the request (supports both sync and async routes)
        // We use block_in_place to call async from sync context
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                // Create dummy state for stateless routing
                let dummy_state = ();
                router.handle_request_async(&request, &dummy_state).await
            })
        })
    } else {
        // Default response when no router is configured
        HttpResponse::not_found().text("No router configured")
    };

    // Send the response
    send_response(stream, &response)?;

    // Check if connection should be closed
    let should_close = request
        .headers()
        .get("connection")
        .map(|v| v.to_lowercase() == "close")
        .unwrap_or(false);

    Ok(should_close)
}

/// Parse an HTTP request from a TCP stream
fn parse_request(stream: &mut TcpStream, config: &ServerConfig) -> HttpResult<HttpRequest> {
    // Get the remote address before creating BufReader
    let remote_addr = stream.peer_addr().ok();

    let mut buffer = Vec::new();
    let mut reader = BufReader::new(&mut *stream);

    // Read the request line first
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .map_err(|e| HttpError::ConnectionError(format!("Failed to read request line: {}", e)))?;

    if request_line.is_empty() {
        return Err(HttpError::InvalidRequest("Empty request".to_string()));
    }

    buffer.extend_from_slice(request_line.as_bytes());

    // Read headers until we find an empty line
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).map_err(|e| {
            HttpError::ConnectionError(format!("Failed to read header line: {}", e))
        })?;

        buffer.extend_from_slice(line.as_bytes());

        // Empty line indicates end of headers
        if line.trim().is_empty() {
            break;
        }
    }

    // Check if there's a body by looking for Content-Length header
    let headers_str = std::str::from_utf8(&buffer)
        .map_err(|e| HttpError::InvalidRequest(format!("Invalid UTF-8: {}", e)))?;

    let content_length = extract_content_length(headers_str)?;

    // Read the body if Content-Length is specified
    if content_length > 0 {
        if content_length > config.max_body_size {
            return Err(HttpError::BodyTooLarge(content_length));
        }

        let mut body_buffer = vec![0u8; content_length];
        reader
            .read_exact(&mut body_buffer)
            .map_err(|e| HttpError::ConnectionError(format!("Failed to read body: {}", e)))?;

        buffer.extend_from_slice(&body_buffer);
    }

    // Parse the complete request using HttpRequest::parse
    let mut request = HttpRequest::parse(&buffer)?;

    // Set the remote address
    if let Some(addr) = remote_addr {
        request.set_remote_addr(addr);
    }

    // println!(
    //     "📝 Parsed request: {} {} from {}",
    //     request.method(),
    //     request.path(),
    //     request
    //         .remote_addr()
    //         .map(|a| a.to_string())
    //         .unwrap_or_else(|| "unknown".to_string())
    // );

    Ok(request)
}

/// Extract Content-Length from headers
fn extract_content_length(headers_str: &str) -> HttpResult<usize> {
    for line in headers_str.lines() {
        if line.to_lowercase().starts_with("content-length:") {
            let parts: Vec<&str> = line.splitn(2, ':').collect();
            if parts.len() == 2 {
                let length_str = parts[1].trim();
                return length_str.parse::<usize>().map_err(|e| {
                    HttpError::InvalidRequest(format!("Invalid Content-Length: {}", e))
                });
            }
        }
    }
    Ok(0)
}

/// Send an HTTP response to a TCP stream
fn send_response(stream: &mut TcpStream, response: &HttpResponse) -> HttpResult<()> {
    let response_bytes = response.to_bytes();

    stream
        .write_all(&response_bytes)
        .map_err(|e| HttpError::ConnectionError(format!("Failed to write response: {}", e)))?;

    stream
        .flush()
        .map_err(|e| HttpError::ConnectionError(format!("Failed to flush response: {}", e)))?;

    // println!("📤 Response sent: {} bytes", response_bytes.len()); // Disabled for performance
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_creation() {
        let server = HttpServer::new();
        assert_eq!(server.config().max_connections, 1000);
        assert!(server.config().keep_alive);
    }

    #[test]
    fn test_server_config() {
        let config = ServerConfig { max_connections: 500, keep_alive: false, ..Default::default() };
        let server = HttpServer::with_config(config);
        assert_eq!(server.config().max_connections, 500);
        assert!(!server.config().keep_alive);
    }
}
