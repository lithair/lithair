//! Lithair Framework - Core
//!
//! A declarative framework for building APIs and websites in Rust with a coherent,
//! memory-first runtime.
//!
//! # Overview
//!
//! Lithair starts from a simple idea: many projects need to store data, expose it
//! over HTTP, and serve a frontend without assembling a large stack of separate
//! services. You define your data models with annotations and enable the pieces
//! you need: REST endpoints, event sourcing, sessions, RBAC, frontend serving,
//! and replication.
//!
//! The result is a modular framework with a simple default deployment model. You
//! can start with one coherent binary and add complexity only when your use case
//! truly requires it.
//!
//! # Quick Start
//!
//! Add `lithair-core` to your `Cargo.toml` (derive macros are re-exported, so
//! you do not need a separate `lithair-macros` dependency):
//!
//! ```toml,ignore
//! [dependencies]
//! lithair-core = "1.0"
//! serde = { version = "1.0", features = ["derive"] }
//! tokio = { version = "1", features = ["full"] }
//! ```
//!
//! Then define your model and start the server:
//!
//! ```rust,ignore
//! use lithair_core::app::LithairServer;
//! use lithair_core::DeclarativeModel;
//! use serde::{Serialize, Deserialize};
//!
//! #[derive(DeclarativeModel, Serialize, Deserialize, Clone, Debug)]
//! struct Product {
//!     id: String,
//!     name: String,
//!     price: f64,
//! }
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     LithairServer::new()
//!         .with_port(3000)
//!         .with_model::<Product>("./data/products", "/api/products")
//!         .serve()
//!         .await
//! }
//! ```
//!
//! # Architecture
//!
//! Lithair is built on these core modules:
//!
//! - [`app`] - Declarative server builder (`LithairServer`)
//! - [`http`] - HTTP server built on hyper
//! - [`engine`] - Event sourcing and state management
//! - [`serialization`] - JSON (simd-json) and binary (rkyv) serialization
//! - [`rbac`] - Role-based access control
//! - [`session`] - Session management with event sourcing
//!
//! # Getting Started with the CLI
//!
//! The fastest way to start is with the `lithair` CLI:
//!
//! ```bash
//! cargo install lithair-cli
//! lithair new my-app
//! cd my-app
//! cargo run
//! ```
//!
//! This generates a ready-to-run project with a `DeclarativeModel`, health
//! endpoint, frontend serving, and standard directory layout.
//!
//! # Features
//!
//! - **Declarative**: Define models, get full REST APIs automatically
//! - **Performance-focused**: Native Rust runtime with memory-first serving
//! - **Event Sourcing**: Built-in immutable event log with CQRS
//! - **Type Safety**: Rust's type system prevents common errors
//! - **Modular runtime**: Start with one binary, expand when needed

// Public modules - Core framework only
pub mod cluster;
pub mod config; // Configuration system with TOML support
#[allow(deprecated)]
pub mod consensus; // Distributed replication for DeclarativeModels
pub mod engine;
pub mod frontend; // Memory-first static file serving
pub mod http;
pub mod lifecycle;
#[cfg(feature = "mfa")]
pub mod mfa; // Multi-Factor Authentication (TOTP)
pub mod model; // Declarative model specifications
pub mod model_inspect; // Internal field inspection and optimization
pub mod rbac; // Role-Based Access Control system
pub mod schema;
pub mod security; // Core RBAC security - non-optional
pub mod serialization; // JSON and binary serialization (simd-json, rkyv, bincode)
pub mod session; // Session management with event sourcing
pub mod system; // System metrics (CPU, RAM, load, RSS, request stats)

// Application server (unified multi-model server)
pub mod app;

// Admin UI (optional, feature-gated)
#[cfg(feature = "admin-ui")]
pub mod admin_ui;

// No internal examples - keep framework API clean

pub mod testing;

// The recommended one-line application import (stable v1.0 surface).
pub mod prelude;

// Re-export derive macros from lithair-macros so users only need one crate
#[cfg(feature = "macros")]
pub use lithair_macros::{lithair_model, DeclarativeModel, LifecycleAware, RbacRole};

/// Private re-exports used by derive macros.
///
/// These are implementation details that allow `lithair-macros` derives
/// (e.g. `DeclarativeModel`) to reference external crates without forcing
/// consumers to declare those crates in their own `Cargo.toml`.
///
/// `lithair-macros` is a `proc-macro` crate and therefore cannot expose
/// non-macro items itself. The canonical workaround (used by `serde`,
/// `tokio`, etc.) is to host the `__private` namespace on the companion
/// regular crate — here, `lithair-core`. Macro-emitted code references
/// `::lithair_core::__private::<crate>::…` instead of `::<crate>::…`.
///
/// Do not use these paths from application code — they are not part of the
/// stable public API and may move or disappear between versions.
#[doc(hidden)]
pub mod __private {
    pub use anyhow;
    pub use clap;
    pub use serde_json;
    pub use tokio;
}

// Re-exports of main types and traits
pub use app::LithairServer;
pub use engine::{RaftstoneApplication, StateEngine};
pub use http::{HttpServer, Route};
pub use model_inspect::Inspectable;
pub use security::{
    AuthContext, Permission, RBACMiddleware, Role, SecurityError, SecurityEvent, SecurityState,
    User,
};

// Main result type for the framework
pub type Result<T> = std::result::Result<T, Error>;

/// Main error type for Lithair framework
#[derive(Debug)]
pub enum Error {
    /// HTTP-related errors (parsing, server issues, etc.)
    HttpError(String),
    /// Serialization/deserialization errors
    SerializationError(String),
    /// File I/O and persistence errors
    PersistenceError(String),
    /// State management and engine errors
    EngineError(String),
    /// Generic framework errors
    FrameworkError(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::HttpError(msg) => write!(f, "HTTP Error: {}", msg),
            Error::SerializationError(msg) => write!(f, "Serialization Error: {}", msg),
            Error::PersistenceError(msg) => write!(f, "Persistence Error: {}", msg),
            Error::EngineError(msg) => write!(f, "Engine Error: {}", msg),
            Error::FrameworkError(msg) => write!(f, "Framework Error: {}", msg),
        }
    }
}

impl std::error::Error for Error {}
