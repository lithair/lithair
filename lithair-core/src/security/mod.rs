//! Lithair Security Module
//!
//! Core RBAC (Role-Based Access Control) implementation for Lithair.
//! This module is non-optional and provides enterprise-grade security
//! built into every Lithair application.
//!
//! ## Security Features
//! - **Password Hashing**: Argon2id (OWASP recommended)
//! - **JWT Tokens**: HMAC-SHA256 signatures
//! - **Session IDs**: Cryptographically secure UUIDs
//! - **Anti-DDoS**: Rate limiting and circuit breakers

pub mod anti_ddos;
mod core;
mod middleware;
pub mod password;

// Re-export core security types
pub use core::{
    AuthContext, Permission, Role, RoleId, SecurityError, SecurityEvent, SecurityState, Session,
    SessionId, User, UserId,
};

// Re-export middleware types
pub use middleware::{JwtClaims, RBACMiddleware};

// Re-export password utilities
pub use password::{hash_password, verify_password, PasswordError, PasswordHasherService};
