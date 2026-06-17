//! The Lithair prelude — the recommended one-line import for applications.
//!
//! ```rust,ignore
//! use lithair_core::prelude::*;
//! ```
//!
//! This brings the common user-facing surface into scope: the server builder,
//! the derive/attribute macros, the HTTP method/status/response helpers and the
//! custom-route type aliases, plus the core RBAC types most handlers reference.
//!
//! Everything re-exported here is part of the **stable** v1.0 public API surface
//! (see [`docs/reference/api-stability.md`](https://github.com/lithair/lithair/blob/main/docs/reference/api-stability.md)).
//! Items reachable only through their defining modules (engine internals, etc.)
//! are deliberately *not* in the prelude.

// Server entry point and its builder methods.
pub use crate::app::LithairServer;

// HTTP essentials for custom routes: `with_route(Method::GET, …)` handlers take
// a `RouteRequest` and return a `RouteResponse`; `response::*` and `StatusCode`
// build those responses.
pub use crate::app::{response, Method, RouteRequest, RouteResponse, StatusCode};

// Derive and attribute macros (re-exported from `lithair-macros` under the
// default `macros` feature so applications depend on `lithair-core` only).
#[cfg(feature = "macros")]
pub use lithair_macros::{lithair_model, DeclarativeModel, LifecycleAware, RbacRole};

// Core RBAC / security types commonly named in handlers and model annotations.
pub use crate::security::{AuthContext, Permission, Role, User};
