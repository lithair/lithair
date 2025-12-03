//! Generic backend proxy patterns for Lithair applications
//!
//! Provides reusable patterns for proxying requests to backend handlers,
//! particularly useful for DeclarativeHttpHandler routing and API management.

#[allow(unused_imports)]
use http_body_util::BodyExt;
use crate::http::{internal_server_error_response, not_found_response, parse_api_path_segments, Req, Resp};
use std::future::Future;
use std::pin::Pin;
#[cfg(test)]
use hyper::StatusCode;

/// Future type for backend handler functions
pub type BackendHandlerFuture = Pin<Box<dyn Future<Output = Result<Resp, Box<dyn std::error::Error + Send + Sync>>> + Send>>;

/// Generic backend API router
pub struct BackendRouter {
    /// API prefix (e.g., "/api/")
    pub api_prefix: String,
}

/// Trait for backend API handlers
pub trait BackendHandler: Send + Sync {
    /// Handle the backend request with parsed path segments
    fn handle_backend_request(&self, req: Req, segments: &[&str]) -> BackendHandlerFuture;
}

/// Route configuration for backend endpoints
#[derive(Clone)]
pub struct BackendRoute {
    /// Path prefix (e.g., "/api/articles")
    pub prefix: String,
    /// Human-readable name for logging
    pub name: String,
}

impl BackendRouter {
    /// Create a new backend router
    pub fn new(api_prefix: impl Into<String>) -> Self {
        Self {
            api_prefix: api_prefix.into(),
        }
    }

    /// Route a request to the appropriate backend handler
    pub async fn route_to_handler<H>(
        &self,
        req: Req,
        route: &BackendRoute,
        handler: &H,
    ) -> Resp
    where
        H: BackendHandler,
    {
        let path = req.uri().path().to_string();

        if !path.starts_with(&route.prefix) {
            return not_found_response("API endpoint");
        }

        // Parse path segments after the route prefix
        let segments = parse_api_path_segments(&path, &route.prefix);

        log::debug!("🔌 {} API: {} → segments: {:?}", route.name, path, segments);

        // Call the backend handler
        match handler.handle_backend_request(req, &segments).await {
            Ok(response) => response,
            Err(error) => {
                log::error!("Backend handler error: {}", error);
                internal_server_error_response(&format!("{} handler failed", route.name))
            }
        }
    }

    /// Route with multiple backend handlers based on path prefixes
    pub async fn route_multi<H>(
        &self,
        req: Req,
        routes: &[(BackendRoute, H)],
    ) -> Resp
    where
        H: BackendHandler,
    {
        let path = req.uri().path();

        // Find the matching route
        for (route, handler) in routes {
            if path.starts_with(&route.prefix) {
                return self.route_to_handler(req, route, handler).await;
            }
        }

        // No route matched
        not_found_response("API endpoint")
    }
}

/// Generic backend handler helper - takes a handler function and applies it
pub async fn handle_with_segments<F, Fut>(
    req: Req,
    path: &str,
    prefix: &str,
    handler_name: &str,
    handler_fn: F,
) -> Resp
where
    F: FnOnce(Req, Vec<&str>) -> Fut,
    Fut: std::future::Future<Output = Result<Resp, Box<dyn std::error::Error + Send + Sync>>>,
{
    if !path.starts_with(prefix) {
        return not_found_response("API endpoint");
    }

    let segments = parse_api_path_segments(path, prefix);
    log::debug!("🔌 {} API: {} → segments: {:?}", handler_name, path, segments);

    match handler_fn(req, segments).await {
        Ok(response) => response,
        Err(error) => {
            log::error!("{} handler error: {}", handler_name, error);
            internal_server_error_response(&format!("{} handler failed", handler_name))
        }
    }
}

/// Simple backend proxy pattern for DeclarativeHttpHandler
///
/// This function provides a reusable pattern for proxying API requests
/// to DeclarativeHttpHandler instances.
///
/// # Arguments
/// * `req` - The incoming HTTP request
/// * `path` - The request path
/// * `prefix` - The API prefix to match (e.g., "/api/articles")
/// * `handler_name` - Name for logging purposes
/// * `handler` - The DeclarativeHttpHandler to proxy to
///
/// # Returns
/// HTTP response from handler or 404 if path doesn't match prefix
pub async fn proxy_to_declarative_handler<T>(
    req: Req,
    path: &str,
    prefix: &str,
    handler_name: &str,
    handler: &crate::http::DeclarativeHttpHandler<T>,
) -> Resp
where
    T: crate::http::HttpExposable + crate::lifecycle::LifecycleAware + crate::consensus::ReplicatedModel + Send + Sync,
{
    // Check if this request should be handled by this endpoint
    if !path.starts_with(prefix) {
        return not_found_response("API endpoint");
    }

    // Parse path segments for the handler
    let segments = parse_api_path_segments(path, prefix);
    log::debug!("🔌 {} API: {} → segments: {:?}", handler_name, path, segments);

    // Call the declarative handler
    match handler.handle_request(req, &segments).await {
        Ok(response) => response,
        Err(_) => internal_server_error_response(&format!("{} handler failed", handler_name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::body_from;
    use hyper::Response;

    #[allow(dead_code)]
    struct TestBackendHandler;

    impl BackendHandler for TestBackendHandler {
        fn handle_backend_request(&self, _req: Req, segments: &[&str]) -> BackendHandlerFuture {
            let response_text = if segments.is_empty() {
                "list".to_string()
            } else {
                format!("item: {}", segments.join("/"))
            };

            Box::pin(async move {
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(body_from(response_text))
                    .unwrap())
            })
        }
    }

    /* #[tokio::test]
    async fn test_backend_router() {
        let router = BackendRouter::new("/api/");
        let route = BackendRoute {
            prefix: "/api/articles".to_string(),
            name: "Articles".to_string(),
        };
        let handler = TestBackendHandler;

        // Test list endpoint
        let req = Request::builder()
            .uri("/api/articles")
            .body(http_body_util::Empty::<bytes::Bytes>::new().map_err(|e| match e {}))
            .unwrap();
        let resp = router.route_to_handler(req, &route, &handler).await;
        assert_eq!(resp.status(), StatusCode::OK);

        // Test item endpoint
        let req = Request::builder()
            .uri("/api/articles/123")
            .body(http_body_util::Empty::<bytes::Bytes>::new().map_err(|e| match e {}))
            .unwrap();
        let resp = router.route_to_handler(req, &route, &handler).await;
        assert_eq!(resp.status(), StatusCode::OK);
    } */
}
