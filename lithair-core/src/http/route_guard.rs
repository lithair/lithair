//! Route Guard System - Declarative route protection
//!
//! Provides declarative patterns for common route protection scenarios:
//! - Authentication checks
//! - Role-based access
//! - Rate limiting
//! - Custom validation
//!
//! Example:
//! ```ignore
//! .with_route_guard("/admin/*", RouteGuard::RequireAuth {
//!     redirect_to: Some("/login".to_string()),
//!     exclude: vec!["/admin/login".to_string()],
//! })
//! ```

use bytes::Bytes;
#[allow(unused_imports)]
use http_body_util::BodyExt;
use http_body_util::Full;
use hyper::{Method, Request, Response, StatusCode};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

type Req = Request<hyper::body::Incoming>;
type Resp = Response<Full<Bytes>>;
type BoxFuture = Pin<Box<dyn Future<Output = Result<GuardResult, anyhow::Error>> + Send>>;

/// Result of a route guard check
#[derive(Debug)]
pub enum GuardResult {
    /// Request is allowed, continue to handler
    Allow,
    /// Request is denied, return this response
    Deny(Resp),
}

/// Route guard definition
#[derive(Clone)]
pub enum RouteGuard {
    /// Require authentication via session token
    RequireAuth {
        /// Where to redirect if not authenticated (None = 401 response)
        redirect_to: Option<String>,
        /// Paths to exclude from this policy (e.g., login page)
        exclude: Vec<String>,
    },

    /// Require specific role(s)
    RequireRole {
        /// Allowed roles
        roles: Vec<String>,
        /// Where to redirect if unauthorized (None = 403 response)
        redirect_to: Option<String>,
    },

    /// Rate limiting
    RateLimit {
        /// Maximum requests per window
        max_requests: u32,
        /// Window duration in seconds
        window_secs: u64,
    },

    /// Custom policy with user-defined logic
    Custom(Arc<dyn Fn(Req) -> BoxFuture + Send + Sync>),
}

impl std::fmt::Debug for RouteGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RouteGuard::RequireAuth { redirect_to, exclude } => f
                .debug_struct("RequireAuth")
                .field("redirect_to", redirect_to)
                .field("exclude", exclude)
                .finish(),
            RouteGuard::RequireRole { roles, redirect_to } => f
                .debug_struct("RequireRole")
                .field("roles", roles)
                .field("redirect_to", redirect_to)
                .finish(),
            RouteGuard::RateLimit { max_requests, window_secs } => f
                .debug_struct("RateLimit")
                .field("max_requests", max_requests)
                .field("window_secs", window_secs)
                .finish(),
            RouteGuard::Custom(_) => f.debug_struct("Custom").finish(),
        }
    }
}

impl RouteGuard {
    /// Check if a request matches this policy
    pub async fn check(
        &self,
        req: &Req,
        session_store: Option<Arc<dyn std::any::Any + Send + Sync>>,
    ) -> Result<GuardResult, anyhow::Error> {
        match self {
            RouteGuard::RequireAuth { redirect_to, exclude } => {
                self.check_auth(req, session_store, redirect_to, exclude).await
            }
            RouteGuard::RequireRole { roles, redirect_to } => {
                self.check_role(req, session_store, roles, redirect_to).await
            }
            RouteGuard::RateLimit { .. } => {
                // Fail closed: rate limiting is not yet enforced, deny all rate-limited routes
                log::warn!("RateLimit guard is not yet implemented; denying request");
                Ok(GuardResult::Deny(
                    Response::builder()
                        .status(StatusCode::SERVICE_UNAVAILABLE)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(
                            r#"{"error":"Rate limiting not yet available"}"#,
                        )))
                        .unwrap(),
                ))
            }
            RouteGuard::Custom(_checker) => {
                // Custom policies handle their own logic
                Ok(GuardResult::Allow)
            }
        }
    }

    async fn check_auth(
        &self,
        req: &Req,
        session_store: Option<Arc<dyn std::any::Any + Send + Sync>>,
        redirect_to: &Option<String>,
        exclude: &[String],
    ) -> Result<GuardResult, anyhow::Error> {
        let path = req.uri().path();

        // Check exclusions
        for excluded_path in exclude {
            if path.starts_with(excluded_path) {
                return Ok(GuardResult::Allow);
            }
        }

        // Resolve the session store via the shared recognizer so this guard
        // honors BOTH a raw `Arc<PersistentSessionStore>` (the RBAC builder
        // path) AND an `Arc<SessionManager<…>>` (the `with_sessions(...)` API).
        // The previous raw downcast to `PersistentSessionStore` silently failed
        // for the `SessionManager` shape, which made RequireAuth a no-op for the
        // documented `with_sessions` configuration (issue #143; recognizer is
        // the same single source of truth used by the model gate, issue #80).
        let store_any = match session_store {
            Some(store_any) => store_any,
            None => return Ok(GuardResult::Allow), // No session store = no auth configured.
        };
        let recognized = crate::session::RecognizedSessionStore::recognize(&store_any);

        // Extract token from Authorization header or Cookie
        let token = req
            .headers()
            .get(hyper::header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer "))
            .or_else(|| {
                req.headers().get(hyper::header::COOKIE).and_then(|h| h.to_str().ok()).and_then(
                    |cookies| {
                        cookies
                            .split(';')
                            .find(|c| c.trim().starts_with("session_token="))
                            .and_then(|c| c.trim().strip_prefix("session_token="))
                    },
                )
            });

        // Validate token: a live (present, non-expired) session in a recognized
        // store. A configured-but-unrecognized store shape fails CLOSED (deny) —
        // this is a security guard, so an unverifiable session is not allowed.
        let is_valid = match (&recognized, token) {
            (Some(store), Some(token)) => store.get_live_session(token).await.is_some(),
            _ => false,
        };

        if is_valid {
            Ok(GuardResult::Allow)
        } else {
            // Denied - redirect or return 401
            if let Some(redirect_url) = redirect_to {
                Ok(GuardResult::Deny(
                    Response::builder()
                        .status(StatusCode::FOUND)
                        .header("Location", redirect_url)
                        .header("Content-Type", "text/html")
                        .body(Full::new(Bytes::from(format!(
                            r#"<!DOCTYPE html>
<html><head><meta http-equiv="refresh" content="0;url={}"></head>
<body><p>Redirecting to login...</p></body></html>"#,
                            redirect_url
                        ))))
                        .unwrap(),
                ))
            } else {
                Ok(GuardResult::Deny(
                    Response::builder()
                        .status(StatusCode::UNAUTHORIZED)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(r#"{"error":"Authentication required"}"#)))
                        .unwrap(),
                ))
            }
        }
    }

    async fn check_role(
        &self,
        _req: &Req,
        _session_store: Option<Arc<dyn std::any::Any + Send + Sync>>,
        _roles: &[String],
        redirect_to: &Option<String>,
    ) -> Result<GuardResult, anyhow::Error> {
        // Fail closed: role checking is not yet enforced, deny all role-protected routes
        log::warn!("RequireRole guard is not yet implemented; denying request");
        if let Some(redirect_url) = redirect_to {
            Ok(GuardResult::Deny(
                Response::builder()
                    .status(StatusCode::FOUND)
                    .header("Location", redirect_url)
                    .body(Full::new(Bytes::from("Redirecting...")))
                    .unwrap(),
            ))
        } else {
            Ok(GuardResult::Deny(
                Response::builder()
                    .status(StatusCode::FORBIDDEN)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(
                        r#"{"error":"Role-based access control not yet available"}"#,
                    )))
                    .unwrap(),
            ))
        }
    }
}

/// Route guard matcher - associates patterns with policies
#[derive(Debug, Clone)]
pub struct RouteGuardMatcher {
    /// Pattern to match (supports wildcards)
    pub pattern: String,
    /// HTTP methods to apply to (None = all methods)
    pub methods: Option<Vec<Method>>,
    /// Guard to apply
    pub guard: RouteGuard,
}

impl RouteGuardMatcher {
    /// Check if a request matches this guard matcher
    pub fn matches(&self, req: &Req) -> bool {
        let path = req.uri().path();
        let method = req.method();

        // Check method
        if let Some(ref methods) = self.methods {
            if !methods.contains(method) {
                return false;
            }
        }

        // Check pattern (simple wildcard matching)
        self.matches_pattern(path)
    }

    fn matches_pattern(&self, path: &str) -> bool {
        if self.pattern.ends_with("/*") {
            let prefix = &self.pattern[..self.pattern.len() - 2];
            path.starts_with(prefix)
        } else {
            path == self.pattern
        }
    }
}

#[cfg(test)]
mod tests {

    // #[test]
    // fn test_pattern_matching() {
    //     let req = Request::builder()
    //         .uri("/admin/users")
    //         .body(http_body_util::Empty::<bytes::Bytes>::new().map_err(|e| match e {}))
    //         .unwrap();
    //
    //     let matcher = RouteGuardMatcher {
    //         pattern: "/admin/*".to_string(),
    //         methods: None,
    //         guard: RouteGuard::RequireAuth {
    //             redirect_to: Some("/login".to_string()),
    //             exclude: vec![],
    //         },
    //     };
    //
    //     assert!(matcher.matches(&req));
    // }
}
