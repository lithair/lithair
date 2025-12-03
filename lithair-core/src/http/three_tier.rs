//! Three-tier routing architecture for Lithair applications
//!
//! Provides a generic three-tier routing pattern:
//! - Tier 1 (Frontend): Static assets, UI resources, public content
//! - Tier 2 (Backend): API endpoints, business logic services
//! - Tier 3 (Admin): Administrative interfaces, internal management
//!
//! This pattern separates concerns cleanly and allows for different handling strategies
//! for each tier (caching, security, performance optimizations, etc.).

#[allow(unused_imports)]
use crate::http::body_from;
#[allow(unused_imports)]
use http_body_util::BodyExt;
use crate::http::{Req, Resp};
use hyper::Method;
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;

/// Result type for three-tier routing handlers
pub type ThreeTierResult = Result<Resp, Infallible>;

/// Future type for async handler functions
pub type HandlerFuture = Pin<Box<dyn Future<Output = ThreeTierResult> + Send>>;

/// Generic three-tier router configuration
pub struct ThreeTierRouter<T> {
    /// Application context/state
    pub context: T,
    /// Admin path prefix (default: "/admin/")
    pub admin_prefix: String,
    /// Backend API path prefix (default: "/api/")
    pub backend_prefix: String,
}

/// Handler trait for three-tier routing
pub trait ThreeTierHandler<T: Send + Sync>: Send + Sync {
    /// Handle frontend requests (Tier 1)
    /// These are typically static assets, UI resources, and public content
    fn handle_frontend(&self, req: Req, context: &T) -> HandlerFuture;

    /// Handle backend API requests (Tier 2)
    /// These are API endpoints, business logic services, and data operations
    fn handle_backend(&self, req: Req, context: &T) -> HandlerFuture;

    /// Handle admin requests (Tier 3)
    /// These are administrative interfaces, internal management, and monitoring
    fn handle_admin(&self, req: Req, method: &Method, path: &str, context: &T) -> HandlerFuture;
}

impl<T: Send + Sync> ThreeTierRouter<T> {
    /// Create a new three-tier router with default prefixes
    pub fn new(context: T) -> Self {
        Self {
            context,
            admin_prefix: "/admin/".to_string(),
            backend_prefix: "/api/".to_string(),
        }
    }

    /// Create a new three-tier router with custom prefixes
    pub fn with_prefixes(context: T, admin_prefix: String, backend_prefix: String) -> Self {
        Self {
            context,
            admin_prefix,
            backend_prefix,
        }
    }

    /// Route request to appropriate tier handler
    pub async fn route<H>(&self, req: Req, handler: &H) -> ThreeTierResult
    where
        H: ThreeTierHandler<T>,
    {
        let method = req.method().clone();
        let path = req.uri().path().to_string();

        log::debug!("🌐 Three-Tier Route: {} {}", method, path);

        // Three-tier routing logic
        if path.starts_with(&self.admin_prefix) {
            // Tier 3: Admin paths - Internal management
            handler.handle_admin(req, &method, &path, &self.context).await
        } else if path.starts_with(&self.backend_prefix) {
            // Tier 2: Backend paths - API services
            handler.handle_backend(req, &self.context).await
        } else {
            // Tier 1: Frontend paths - Static content
            handler.handle_frontend(req, &self.context).await
        }
    }
}

/// Builder pattern for three-tier router configuration
pub struct ThreeTierRouterBuilder<T> {
    context: Option<T>,
    admin_prefix: Option<String>,
    backend_prefix: Option<String>,
}

impl<T> ThreeTierRouterBuilder<T> {
    pub fn new() -> Self {
        Self {
            context: None,
            admin_prefix: None,
            backend_prefix: None,
        }
    }

    pub fn with_context(mut self, context: T) -> Self {
        self.context = Some(context);
        self
    }

    pub fn with_admin_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.admin_prefix = Some(prefix.into());
        self
    }

    pub fn with_backend_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.backend_prefix = Some(prefix.into());
        self
    }

    pub fn build(self) -> ThreeTierRouter<T> {
        let context = self.context.expect("Context is required");
        let admin_prefix = self.admin_prefix.unwrap_or_else(|| "/admin/".to_string());
        let backend_prefix = self.backend_prefix.unwrap_or_else(|| "/api/".to_string());

        ThreeTierRouter {
            context,
            admin_prefix,
            backend_prefix,
        }
    }
}

impl<T> Default for ThreeTierRouterBuilder<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper::{Response, StatusCode};

    struct TestContext;

    #[allow(dead_code)]
    struct TestHandler;

    impl ThreeTierHandler<TestContext> for TestHandler {
        fn handle_frontend(&self, _req: Req, _context: &TestContext) -> HandlerFuture {
            Box::pin(async move {
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(body_from("Frontend"))
                    .unwrap())
            })
        }

        fn handle_backend(&self, _req: Req, _context: &TestContext) -> HandlerFuture {
            Box::pin(async move {
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(body_from("Backend"))
                    .unwrap())
            })
        }

        fn handle_admin(&self, _req: Req, _method: &Method, _path: &str, _context: &TestContext) -> HandlerFuture {
            Box::pin(async move {
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(body_from("Admin"))
                    .unwrap())
            })
        }
    }

    /* #[tokio::test]
    async fn test_three_tier_routing() {
        let router = ThreeTierRouter::new(TestContext);
        let handler = TestHandler;

        // Test frontend routing
        let req = Request::builder()
            .uri("/")
            .body(http_body_util::Empty::<bytes::Bytes>::new().map_err(|e| match e {}))
            .unwrap();
        let resp = router.route(req, &handler).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Test backend routing
        let req = Request::builder()
            .uri("/api/test")
            .body(http_body_util::Empty::<bytes::Bytes>::new().map_err(|e| match e {}))
            .unwrap();
        let resp = router.route(req, &handler).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Test admin routing
        let req = Request::builder()
            .uri("/admin/sites")
            .body(http_body_util::Empty::<bytes::Bytes>::new().map_err(|e| match e {}))
            .unwrap();
        let resp = router.route(req, &handler).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    } */

    #[test]
    fn test_router_builder() {
        let router = ThreeTierRouterBuilder::new()
            .with_context(TestContext)
            .with_admin_prefix("/management/")
            .with_backend_prefix("/services/")
            .build();

        assert_eq!(router.admin_prefix, "/management/");
        assert_eq!(router.backend_prefix, "/services/");
    }
}
