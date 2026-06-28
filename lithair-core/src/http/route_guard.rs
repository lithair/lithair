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
use std::sync::Arc;

type Req = Request<hyper::body::Incoming>;
type Resp = Response<Full<Bytes>>;

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

        let token = super::declarative::extract_session_token(req);

        // Validate token: a live (present, non-expired) session in a recognized
        // store. A configured-but-unrecognized store shape fails CLOSED (deny) —
        // this is a security guard, so an unverifiable session is not allowed.
        let is_valid = match (&recognized, &token) {
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
        req: &Req,
        session_store: Option<Arc<dyn std::any::Any + Send + Sync>>,
        roles: &[String],
        redirect_to: &Option<String>,
    ) -> Result<GuardResult, anyhow::Error> {
        // Authorization guard: resolve the live session (same recognizer path
        // RequireAuth uses, issue #143), then check its role against the
        // allowed set. The role lives in `session.data["role"]` — the
        // convention the login examples establish via `session.set("role", …)`.
        //
        // Fail CLOSED: unlike RequireAuth (which allows when no session store is
        // configured, i.e. no auth at all), an authz guard with no way to read a
        // role denies — an unverifiable role must not pass.
        let Some(store_any) = session_store else {
            return Ok(deny_role(redirect_to));
        };
        let Some(recognized) = crate::session::RecognizedSessionStore::recognize(&store_any) else {
            return Ok(deny_role(redirect_to));
        };
        let Some(token) = super::declarative::extract_session_token(req) else {
            return Ok(deny_role(redirect_to));
        };
        let Some(session) = recognized.get_live_session(&token).await else {
            return Ok(deny_role(redirect_to));
        };

        // `data["role"]` is a JSON string (see `Session::set`). Match it against
        // the allowed roles by exact string equality — the application chooses
        // the role strings it writes at login.
        let role = session.data.get("role").and_then(|v| v.as_str());
        let allowed = role.is_some_and(|r| roles.iter().any(|allowed| allowed == r));

        if allowed {
            Ok(GuardResult::Allow)
        } else {
            Ok(deny_role(redirect_to))
        }
    }
}

/// Build the denial response for a failed role check: a redirect when one is
/// configured (browser flows), otherwise a `403` JSON (API flows).
fn deny_role(redirect_to: &Option<String>) -> GuardResult {
    if let Some(redirect_url) = redirect_to {
        GuardResult::Deny(
            Response::builder()
                .status(StatusCode::FOUND)
                .header("Location", redirect_url)
                .header("Content-Type", "text/html")
                .body(Full::new(Bytes::from(format!(
                    r#"<!DOCTYPE html>
<html><head><meta http-equiv="refresh" content="0;url={redirect_url}"></head>
<body><p>Redirecting...</p></body></html>"#
                ))))
                .unwrap(),
        )
    } else {
        GuardResult::Deny(
            Response::builder()
                .status(StatusCode::FORBIDDEN)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(r#"{"error":"Forbidden: insufficient role"}"#)))
                .unwrap(),
        )
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
