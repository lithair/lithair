//! HTTP routing and request dispatching
//!
//! This module provides a flexible routing system for mapping HTTP requests
//! to handler functions with parameter extraction and middleware support.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use super::{HttpMethod, HttpRequest, HttpResponse};

/// Path parameters extracted from dynamic routes
pub type PathParams = HashMap<String, String>;

/// Error handler function type used by routers
pub type ErrorHandler = Arc<dyn Fn(&HttpRequest, &str) -> HttpResponse + Send + Sync>;

/// Middleware function type used by routes
pub type Middleware = Arc<dyn Fn(&HttpRequest) -> Option<HttpResponse> + Send + Sync>;

/// Route handler function type for read-only operations
///
/// Handlers receive the request, path parameters, and state (read-only), and return a response
pub type RouteHandler<S> = Arc<dyn Fn(&HttpRequest, &PathParams, &S) -> HttpResponse + Send + Sync>;

/// Async route handler function type
///
/// Handlers receive the request, path parameters, and state, and return a future that resolves to a response
pub type AsyncRouteHandler<S> = Arc<
    dyn Fn(
            &HttpRequest,
            &PathParams,
            &S,
        ) -> Pin<Box<dyn Future<Output = HttpResponse> + Send + 'static>>
        + Send
        + Sync,
>;

/// Command route handler function type for operations that modify state
///
/// Handlers receive the request, path parameters, and a command sender to apply events asynchronously
pub type CommandRouteHandler<A> =
    Arc<dyn Fn(&HttpRequest, &PathParams, &CommandSender<A>) -> HttpResponse + Send + Sync>;

/// Command sender for sending events to the engine worker thread
pub type CommandSender<A> =
    std::sync::Arc<std::sync::Mutex<std::sync::mpsc::Sender<CommandMessage<A>>>>;

/// Command message sent to the engine worker thread
pub struct CommandMessage<A: crate::engine::RaftstoneApplication> {
    pub event: <A as crate::engine::RaftstoneApplication>::Event,
    pub response_sender: std::sync::mpsc::Sender<Result<(), String>>,
}

/// A single route definition for read-only operations
#[derive(Clone)]
pub struct Route<S> {
    method: HttpMethod,
    pattern: String,
    handler: RouteHandler<S>,
    middleware: Vec<Middleware>,
}

/// An async route definition
pub struct AsyncRoute<S> {
    method: HttpMethod,
    pattern: String,
    handler: AsyncRouteHandler<S>,
    middleware: Vec<Middleware>,
}

/// A command route definition for operations that modify state
pub struct CommandRoute<A: crate::engine::RaftstoneApplication> {
    method: HttpMethod,
    pattern: String,
    handler: CommandRouteHandler<A>,
    middleware: Vec<Middleware>,
}

impl<S> Route<S> {
    /// Create a new route for read-only operations
    pub fn new<F>(method: HttpMethod, pattern: &str, handler: F) -> Self
    where
        F: Fn(&HttpRequest, &PathParams, &S) -> HttpResponse + Send + Sync + 'static,
    {
        Self {
            method,
            pattern: pattern.to_string(),
            handler: Arc::new(handler),
            middleware: Vec::new(),
        }
    }

    /// Add middleware to this route
    pub fn with_middleware<F>(mut self, middleware: F) -> Self
    where
        F: Fn(&HttpRequest) -> Option<HttpResponse> + Send + Sync + 'static,
    {
        self.middleware.push(Arc::new(middleware));
        self
    }

    /// Get the HTTP method for this route
    pub fn method(&self) -> &HttpMethod {
        &self.method
    }

    /// Get the pattern for this route
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// Check if this route matches the given request
    pub fn matches(&self, request: &HttpRequest) -> Option<PathParams> {
        if &self.method != request.method() {
            return None;
        }

        self.extract_params(request.path())
    }

    /// Extract path parameters from a URL path
    fn extract_params(&self, path: &str) -> Option<PathParams> {
        let pattern_parts: Vec<&str> = self.pattern.split('/').collect();
        let path_parts: Vec<&str> = path.split('/').collect();

        if pattern_parts.len() != path_parts.len() {
            return None;
        }

        let mut params = HashMap::new();

        for (pattern_part, path_part) in pattern_parts.iter().zip(path_parts.iter()) {
            if let Some(param_name) = pattern_part.strip_prefix(':') {
                // Dynamic parameter
                params.insert(param_name.to_string(), path_part.to_string());
            } else if pattern_part != path_part {
                // Static part doesn't match
                return None;
            }
        }

        Some(params)
    }

    /// Execute this route with the given request and state
    pub fn execute(&self, request: &HttpRequest, params: &PathParams, state: &S) -> HttpResponse {
        // Run middleware first
        for middleware in &self.middleware {
            if let Some(response) = middleware(request) {
                return response; // Middleware intercepted the request
            }
        }

        // Execute the main handler
        (self.handler)(request, params, state)
    }
}

impl<S> AsyncRoute<S> {
    /// Create a new async route
    pub fn new<F, Fut>(method: HttpMethod, pattern: &str, handler: F) -> Self
    where
        F: Fn(&HttpRequest, &PathParams, &S) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HttpResponse> + Send + 'static,
    {
        Self {
            method,
            pattern: pattern.to_string(),
            handler: Arc::new(move |req, params, state| Box::pin(handler(req, params, state))),
            middleware: Vec::new(),
        }
    }

    /// Add middleware to this route
    pub fn with_middleware<F>(mut self, middleware: F) -> Self
    where
        F: Fn(&HttpRequest) -> Option<HttpResponse> + Send + Sync + 'static,
    {
        self.middleware.push(Arc::new(middleware));
        self
    }

    /// Get the HTTP method for this route
    pub fn method(&self) -> &HttpMethod {
        &self.method
    }

    /// Get the pattern for this route
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// Check if this route matches the given request
    pub fn matches(&self, request: &HttpRequest) -> Option<PathParams> {
        if &self.method != request.method() {
            return None;
        }

        self.extract_params(request.path())
    }

    /// Extract path parameters from a URL path
    fn extract_params(&self, path: &str) -> Option<PathParams> {
        let pattern_parts: Vec<&str> = self.pattern.split('/').collect();
        let path_parts: Vec<&str> = path.split('/').collect();

        if pattern_parts.len() != path_parts.len() {
            return None;
        }

        let mut params = HashMap::new();

        for (pattern_part, path_part) in pattern_parts.iter().zip(path_parts.iter()) {
            if let Some(param_name) = pattern_part.strip_prefix(':') {
                // Dynamic parameter
                params.insert(param_name.to_string(), path_part.to_string());
            } else if pattern_part != path_part {
                // Static part doesn't match
                return None;
            }
        }

        Some(params)
    }

    /// Execute this async route with the given request and state
    pub async fn execute_async(
        &self,
        request: &HttpRequest,
        params: &PathParams,
        state: &S,
    ) -> HttpResponse {
        // Run middleware first
        for middleware in &self.middleware {
            if let Some(response) = middleware(request) {
                return response; // Middleware intercepted the request
            }
        }

        // Execute the main async handler
        (self.handler)(request, params, state).await
    }
}

impl<S> std::fmt::Debug for Route<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Route")
            .field("method", &self.method)
            .field("pattern", &self.pattern)
            .field("middleware_count", &self.middleware.len())
            .finish()
    }
}

impl<A: crate::engine::RaftstoneApplication> CommandRoute<A> {
    /// Create a new command route for operations that modify state
    pub fn new<F>(method: HttpMethod, pattern: &str, handler: F) -> Self
    where
        F: Fn(&HttpRequest, &PathParams, &CommandSender<A>) -> HttpResponse + Send + Sync + 'static,
    {
        Self {
            method,
            pattern: pattern.to_string(),
            handler: Arc::new(handler),
            middleware: Vec::new(),
        }
    }

    /// Check if this route matches the given request and return path parameters
    pub fn matches(&self, request: &HttpRequest) -> Option<PathParams> {
        if request.method() != &self.method {
            return None;
        }

        self.extract_params(request.path())
    }

    /// Get path parameters if this route matches
    pub fn get_params(&self, request: &HttpRequest) -> Option<PathParams> {
        self.matches(request)
    }

    /// Extract path parameters from a URL path
    fn extract_params(&self, path: &str) -> Option<PathParams> {
        let pattern_parts: Vec<&str> = self.pattern.split('/').collect();
        let path_parts: Vec<&str> = path.split('/').collect();

        if pattern_parts.len() != path_parts.len() {
            return None;
        }

        let mut params = HashMap::new();

        for (pattern_part, path_part) in pattern_parts.iter().zip(path_parts.iter()) {
            if let Some(param_name) = pattern_part.strip_prefix(':') {
                // Dynamic parameter
                params.insert(param_name.to_string(), path_part.to_string());
            } else if pattern_part != path_part {
                // Static part doesn't match
                return None;
            }
        }

        Some(params)
    }

    /// Execute this command route with the given request and command sender
    pub fn execute(
        &self,
        request: &HttpRequest,
        params: &PathParams,
        command_sender: &CommandSender<A>,
    ) -> HttpResponse {
        // Run middleware first
        for middleware in &self.middleware {
            if let Some(response) = middleware(request) {
                return response; // Middleware intercepted the request
            }
        }

        // Execute the main handler with command sender for async event processing
        (self.handler)(request, params, command_sender)
    }
}

/// HTTP router for dispatching requests to handlers
pub struct Router<S = ()> {
    routes: Vec<Route<S>>,
    async_routes: Vec<AsyncRoute<S>>,
    not_found_handler: Option<RouteHandler<S>>,
    _error_handler: Option<ErrorHandler>,
}

/// Enhanced Router that supports both read-only routes and command routes
pub struct EnhancedRouter<A: crate::engine::RaftstoneApplication> {
    routes: Vec<Route<A::State>>,
    command_routes: Vec<CommandRoute<A>>,
    not_found_handler: Option<RouteHandler<A::State>>,
    _error_handler: Option<ErrorHandler>,
}

impl<S> Router<S> {
    /// Create a new router
    pub fn new() -> Self {
        Self {
            routes: Vec::new(),
            async_routes: Vec::new(),
            not_found_handler: None,
            _error_handler: None,
        }
    }

    /// Add a route to the router
    pub fn route(mut self, route: Route<S>) -> Self {
        self.routes.push(route);
        self
    }

    /// Add an async route to the router
    pub fn async_route(mut self, route: AsyncRoute<S>) -> Self {
        self.async_routes.push(route);
        self
    }

    /// Add a GET route
    pub fn get<F>(self, pattern: &str, handler: F) -> Self
    where
        F: Fn(&HttpRequest, &PathParams, &S) -> HttpResponse + Send + Sync + 'static,
    {
        self.route(Route::new(HttpMethod::GET, pattern, handler))
    }

    /// Add a POST route
    pub fn post<F>(self, pattern: &str, handler: F) -> Self
    where
        F: Fn(&HttpRequest, &PathParams, &S) -> HttpResponse + Send + Sync + 'static,
    {
        self.route(Route::new(HttpMethod::POST, pattern, handler))
    }

    /// Add a PUT route
    pub fn put<F>(self, pattern: &str, handler: F) -> Self
    where
        F: Fn(&HttpRequest, &PathParams, &S) -> HttpResponse + Send + Sync + 'static,
    {
        self.route(Route::new(HttpMethod::PUT, pattern, handler))
    }

    /// Add a DELETE route
    pub fn delete<F>(self, pattern: &str, handler: F) -> Self
    where
        F: Fn(&HttpRequest, &PathParams, &S) -> HttpResponse + Send + Sync + 'static,
    {
        self.route(Route::new(HttpMethod::DELETE, pattern, handler))
    }

    /// Add a PATCH route
    pub fn patch<F>(self, pattern: &str, handler: F) -> Self
    where
        F: Fn(&HttpRequest, &PathParams, &S) -> HttpResponse + Send + Sync + 'static,
    {
        self.route(Route::new(HttpMethod::PATCH, pattern, handler))
    }

    /// Add an async GET route
    pub fn get_async<F, Fut>(self, pattern: &str, handler: F) -> Self
    where
        F: Fn(&HttpRequest, &PathParams, &S) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HttpResponse> + Send + 'static,
    {
        self.async_route(AsyncRoute::new(HttpMethod::GET, pattern, handler))
    }

    /// Add an async POST route
    pub fn post_async<F, Fut>(self, pattern: &str, handler: F) -> Self
    where
        F: Fn(&HttpRequest, &PathParams, &S) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HttpResponse> + Send + 'static,
    {
        self.async_route(AsyncRoute::new(HttpMethod::POST, pattern, handler))
    }

    /// Add an async PUT route
    pub fn put_async<F, Fut>(self, pattern: &str, handler: F) -> Self
    where
        F: Fn(&HttpRequest, &PathParams, &S) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HttpResponse> + Send + 'static,
    {
        self.async_route(AsyncRoute::new(HttpMethod::PUT, pattern, handler))
    }

    /// Add an async DELETE route
    pub fn delete_async<F, Fut>(self, pattern: &str, handler: F) -> Self
    where
        F: Fn(&HttpRequest, &PathParams, &S) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HttpResponse> + Send + 'static,
    {
        self.async_route(AsyncRoute::new(HttpMethod::DELETE, pattern, handler))
    }

    /// Set a custom 404 handler
    pub fn not_found<F>(mut self, handler: F) -> Self
    where
        F: Fn(&HttpRequest, &PathParams, &S) -> HttpResponse + Send + Sync + 'static,
    {
        self.not_found_handler = Some(Arc::new(handler));
        self
    }

    /// Set a custom error handler
    pub fn error<F>(mut self, handler: F) -> Self
    where
        F: Fn(&HttpRequest, &str) -> HttpResponse + Send + Sync + 'static,
    {
        self._error_handler = Some(Arc::new(handler));
        self
    }

    /// Handle a request and return a response
    pub fn handle_request(&self, request: &HttpRequest, state: &S) -> HttpResponse {
        // Try to find a matching route
        for route in &self.routes {
            if let Some(params) = route.matches(request) {
                return route.execute(request, &params, state);
            }
        }

        // No route matched - use 404 handler or default
        if let Some(handler) = &self.not_found_handler {
            handler(request, &HashMap::new(), state)
        } else {
            HttpResponse::not_found().text(&format!(
                "Not found: {} {}",
                request.method(),
                request.path()
            ))
        }
    }

    /// Handle a request without state (for compatibility)
    pub fn handle_request_stateless(&self, request: &HttpRequest) -> HttpResponse
    where
        S: Default,
    {
        // This is used when the router doesn't need application state
        // We create a dummy state for compatibility
        let dummy_state = S::default();
        self.handle_request(request, &dummy_state)
    }

    /// Handle a request asynchronously and return a response
    pub async fn handle_request_async(&self, request: &HttpRequest, state: &S) -> HttpResponse {
        // First try to find a matching async route
        for async_route in &self.async_routes {
            if let Some(params) = async_route.matches(request) {
                return async_route.execute_async(request, &params, state).await;
            }
        }

        // Then fall back to sync routes
        for route in &self.routes {
            if let Some(params) = route.matches(request) {
                return route.execute(request, &params, state);
            }
        }

        // No route matched - use 404 handler or default
        if let Some(handler) = &self.not_found_handler {
            handler(request, &HashMap::new(), state)
        } else {
            HttpResponse::not_found().text(&format!(
                "Not found: {} {}",
                request.method(),
                request.path()
            ))
        }
    }

    /// Get all routes (for debugging/introspection)
    pub fn routes(&self) -> &[Route<S>] {
        &self.routes
    }

    /// Get all async routes (for debugging/introspection)
    pub fn async_routes(&self) -> &[AsyncRoute<S>] {
        &self.async_routes
    }

    /// Get the number of registered routes
    pub fn route_count(&self) -> usize {
        self.routes.len()
    }
}

impl<S> Default for Router<S> {
    fn default() -> Self {
        Self::new()
    }
}

// Implement Router for () (stateless router)
impl Router<()> {
    /// Handle a request without application state
    pub fn handle(&self, request: &HttpRequest) -> HttpResponse {
        self.handle_request_stateless(request)
    }
}

/// Builder for creating routers with common patterns
pub struct RouterBuilder<S> {
    router: Router<S>,
}

impl<S> RouterBuilder<S> {
    pub fn new() -> Self {
        Self { router: Router::new() }
    }

    /// Add a REST resource with standard CRUD operations
    pub fn resource<F1, F2, F3, F4, F5>(
        mut self,
        path: &str,
        index: F1,
        show: F2,
        create: F3,
        update: F4,
        delete: F5,
    ) -> Self
    where
        F1: Fn(&HttpRequest, &PathParams, &S) -> HttpResponse + Send + Sync + 'static,
        F2: Fn(&HttpRequest, &PathParams, &S) -> HttpResponse + Send + Sync + 'static,
        F3: Fn(&HttpRequest, &PathParams, &S) -> HttpResponse + Send + Sync + 'static,
        F4: Fn(&HttpRequest, &PathParams, &S) -> HttpResponse + Send + Sync + 'static,
        F5: Fn(&HttpRequest, &PathParams, &S) -> HttpResponse + Send + Sync + 'static,
    {
        let id_path = &format!("{}/:id", path);

        self.router = self.router
            .get(path, index)           // GET /users
            .get(id_path, show)         // GET /users/:id
            .post(path, create)         // POST /users
            .put(id_path, update)       // PUT /users/:id
            .delete(id_path, delete); // DELETE /users/:id

        self
    }

    /// Add API versioning prefix
    pub fn api_version(self, version: &str) -> Self {
        // This would be used to prefix all routes with /api/v1, etc.
        // For now, just a placeholder
        println!("API version: {}", version);
        self
    }

    /// Build the final router
    pub fn build(self) -> Router<S> {
        self.router
    }
}

impl<S> Default for RouterBuilder<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: crate::engine::RaftstoneApplication> EnhancedRouter<A> {
    /// Create a new enhanced router
    pub fn new() -> Self {
        Self {
            routes: Vec::new(),
            command_routes: Vec::new(),
            not_found_handler: None,
            _error_handler: None,
        }
    }

    /// Add a regular route (read-only)
    pub fn route(mut self, route: Route<A::State>) -> Self {
        self.routes.push(route);
        self
    }

    /// Add a command route (can modify state)
    pub fn command_route(mut self, route: CommandRoute<A>) -> Self {
        self.command_routes.push(route);
        self
    }

    /// Check if any command route matches this request
    pub fn has_matching_command_route(&self, request: &HttpRequest) -> bool {
        for route in &self.command_routes {
            if route.matches(request).is_some() {
                return true;
            }
        }
        false
    }

    /// Handle a request with access to both state and command sender
    pub fn handle_request(
        &self,
        request: &HttpRequest,
        state: &A::State,
        command_sender: &CommandSender<A>,
    ) -> HttpResponse {
        // First try command routes (they use async command sender)
        for route in &self.command_routes {
            if let Some(params) = route.matches(request) {
                return route.execute(request, &params, command_sender);
            }
        }

        // Then try regular routes (read-only state access)
        for route in &self.routes {
            if let Some(params) = route.matches(request) {
                return route.execute(request, &params, state);
            }
        }

        // No route matched - use 404 handler or default
        if let Some(handler) = &self.not_found_handler {
            handler(request, &std::collections::HashMap::new(), state)
        } else {
            HttpResponse::not_found().text(&format!(
                "Not found: {} {}",
                request.method(),
                request.path()
            ))
        }
    }

    /// Get the total number of registered routes (both regular and command)
    pub fn route_count(&self) -> usize {
        self.routes.len() + self.command_routes.len()
    }
}

impl<A: crate::engine::RaftstoneApplication> Default for EnhancedRouter<A> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_route_pattern_matching() {
        let route = Route::new(HttpMethod::GET, "/users/:id", |_req, _params, _state: &()| {
            HttpResponse::ok().text("user")
        });

        // Create a minimal request for testing
        let request = HttpRequest::new(
            HttpMethod::GET,
            "/users/123".to_string(),
            super::super::HttpVersion::Http1_1,
            HashMap::new(),
            Vec::new(),
        );

        let params = route.matches(&request).unwrap();
        assert_eq!(params.get("id"), Some(&"123".to_string()));
    }

    #[test]
    fn test_route_no_match() {
        let route = Route::new(HttpMethod::GET, "/users/:id", |_req, _params, _state: &()| {
            HttpResponse::ok().text("user")
        });

        let request = HttpRequest::new(
            HttpMethod::POST, // Wrong method
            "/users/123".to_string(),
            super::super::HttpVersion::Http1_1,
            HashMap::new(),
            Vec::new(),
        );

        assert!(route.matches(&request).is_none());
    }

    #[test]
    fn test_router_building() {
        let router = Router::<()>::new()
            .get("/", |_req, _params, _state| HttpResponse::ok().text("home"))
            .get("/users/:id", |_req, params, _state| {
                let id = params.get("id").unwrap();
                HttpResponse::ok().text(&format!("User {}", id))
            });

        assert_eq!(router.route_count(), 2);
    }

    #[test]
    fn test_path_parameter_extraction() {
        let route = Route::new(
            HttpMethod::GET,
            "/api/v1/users/:user_id/posts/:post_id",
            |_req, _params, _state: &()| HttpResponse::ok().text("post"),
        );

        let params = route.extract_params("/api/v1/users/123/posts/456").unwrap();
        assert_eq!(params.get("user_id"), Some(&"123".to_string()));
        assert_eq!(params.get("post_id"), Some(&"456".to_string()));
    }

    #[test]
    fn test_static_route_matching() {
        let route = Route::new(HttpMethod::GET, "/api/health", |_req, _params, _state: &()| {
            HttpResponse::ok().text("healthy")
        });

        let params = route.extract_params("/api/health").unwrap();
        assert!(params.is_empty());

        assert!(route.extract_params("/api/status").is_none());
    }
}
