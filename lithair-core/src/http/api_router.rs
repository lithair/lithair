//! Generic API Router for DeclarativeHttpHandler
//!
//! Routes `/api/` requests to the appropriate DeclarativeHttpHandler instances based on path.
//! This allows separation of concerns: Frontend assets, SSR pages, and API backend routes.

use crate::consensus::ReplicatedModel;
use crate::http::declarative::HttpExposable;
use crate::http::{not_found_response, DeclarativeHttpHandler, Req, Resp};
use crate::lifecycle::LifecycleAware;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Generic API Router that maps URL paths to DeclarativeHttpHandler instances
///
/// This router enables clean separation between:
/// - Frontend asset serving (/, /style.css, etc.)
/// - Server-Side Rendering (/blog, /blog/articles, etc.)
/// - API backend operations (/api/articles, /api/authors, etc.)
pub struct ApiRouter {
    /// Map of path prefixes to their corresponding handlers
    /// e.g., "/api/articles" -> articles_handler
    handlers: HashMap<String, Box<dyn ApiHandlerWrapper>>,
}

/// Trait wrapper to enable storing different DeclarativeHttpHandler types in the same collection
trait ApiHandlerWrapper: Send + Sync {
    fn handle_request<'a>(
        &'a self,
        req: Req,
        path_segments: Vec<String>,
    ) -> Pin<Box<dyn Future<Output = Result<Resp, std::convert::Infallible>> + Send + 'a>>;
}

/// Implementation for any DeclarativeHttpHandler type wrapped in Arc
impl<T> ApiHandlerWrapper for Arc<DeclarativeHttpHandler<T>>
where
    T: Clone + Send + Sync + HttpExposable + LifecycleAware + ReplicatedModel + 'static,
{
    fn handle_request<'a>(
        &'a self,
        req: Req,
        path_segments: Vec<String>,
    ) -> Pin<Box<dyn Future<Output = Result<Resp, std::convert::Infallible>> + Send + 'a>> {
        Box::pin(async move {
            // Convert Vec<String> to Vec<&str>
            let path_refs: Vec<&str> = path_segments.iter().map(|s| s.as_str()).collect();
            DeclarativeHttpHandler::handle_request(self, req, &path_refs).await
        })
    }
}

impl ApiRouter {
    /// Create a new empty API router
    pub fn new() -> Self {
        Self { handlers: HashMap::new() }
    }

    /// Register a DeclarativeHttpHandler for a specific API path prefix
    ///
    /// # Example
    /// ```rust
    /// let mut router = ApiRouter::new();
    /// router.register("/api/articles", articles_handler);
    /// router.register("/api/authors", authors_handler);
    /// ```
    pub fn register<T>(&mut self, path_prefix: &str, handler: Arc<DeclarativeHttpHandler<T>>)
    where
        T: Clone + Send + Sync + HttpExposable + LifecycleAware + ReplicatedModel + 'static,
    {
        log::info!("Registering API handler: {} -> DeclarativeHttpHandler", path_prefix);
        self.handlers.insert(path_prefix.to_string(), Box::new(handler));
    }

    /// Route an API request to the appropriate handler
    ///
    /// Returns None if no handler matches the path
    pub async fn route_request(&self, req: Req) -> Option<Resp> {
        let path = req.uri().path().to_string(); // Clone the path to avoid borrow checker issues

        // Find the longest matching prefix
        let mut best_match: Option<(&String, &Box<dyn ApiHandlerWrapper>)> = None;
        for (prefix, handler) in &self.handlers {
            if path.starts_with(prefix) {
                match best_match {
                    None => best_match = Some((prefix, handler)),
                    Some((best_prefix, _)) => {
                        if prefix.len() > best_prefix.len() {
                            best_match = Some((prefix, handler));
                        }
                    }
                }
            }
        }

        if let Some((matched_prefix, handler)) = best_match {
            log::debug!("API route match: {} -> {}", path, matched_prefix);

            // Extract path segments for the handler (remove the matched prefix)
            let remaining_path = path.strip_prefix(matched_prefix).unwrap_or("");
            let path_segments: Vec<String> = remaining_path
                .split('/')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();

            match handler.handle_request(req, path_segments).await {
                Ok(response) => Some(response),
                Err(_) => {
                    // Since DeclarativeHttpHandler returns Result<Resp, Infallible>,
                    // this should never happen, but we handle it for completeness
                    log::error!("API handler error for {}: Infallible error occurred", path);
                    Some(not_found_response("API handler error"))
                }
            }
        } else {
            log::warn!("No API handler found for path: {}", path);
            None
        }
    }

    /// Get statistics about registered handlers
    pub fn stats(&self) -> ApiRouterStats {
        ApiRouterStats {
            registered_handlers: self.handlers.len(),
            handler_prefixes: self.handlers.keys().cloned().collect(),
        }
    }
}

impl Default for ApiRouter {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about the API router
#[derive(Debug, Clone)]
pub struct ApiRouterStats {
    pub registered_handlers: usize,
    pub handler_prefixes: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_router_stats() {
        let router = ApiRouter::new();
        let stats = router.stats();

        assert_eq!(stats.registered_handlers, 0);
        assert!(stats.handler_prefixes.is_empty());
    }

    // Integration tests with hyper Request<Incoming> are omitted here because
    // constructing a valid Incoming body requires a live HTTP connection.
}
