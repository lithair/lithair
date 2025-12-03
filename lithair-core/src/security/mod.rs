//! Lithair Security Module
//!
//! Core RBAC (Role-Based Access Control) implementation for Lithair.
//! This module is non-optional and provides enterprise-grade security
//! built into every Lithair application.

mod core;
mod middleware;
pub mod anti_ddos;

// Re-export core security types
pub use core::{
    AuthContext, Permission, Role, RoleId, SecurityError, SecurityEvent, SecurityState, Session,
    SessionId, User, UserId,
};

// Re-export middleware types
pub use middleware::{JwtClaims, RBACMiddleware};
