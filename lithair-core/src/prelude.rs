//! Prelude module for convenient imports.
//!
//! Import everything you need with a single line:
//!
//! ```rust,ignore
//! use lithair_core::prelude::*;
//! ```
//!
//! This re-exports the most commonly used types, traits, and functions
//! so you can get started quickly without hunting for import paths.
//!
//! ## What's included
//!
//! - **Server builder**: [`LithairServer`], [`LithairServerBuilder`], [`ModelHandler`]
//! - **Configuration**: [`LithairConfig`], [`LoggingConfig`]
//! - **HTTP primitives** (from the `http` crate): [`Method`], [`Request`], [`Response`], [`StatusCode`]
//! - **Response body helpers**: [`Full`] (from `http-body-util`)
//! - **Engine traits**: [`RaftstoneApplication`], [`StateEngine`]
//! - **HTTP server types**: [`HttpMethod`], [`HttpResponse`], [`HttpServer`], [`Route`], [`FirewallConfig`], [`GzipConfig`]
//! - **Frontend**: [`FrontendEngine`], [`FrontendServer`]
//! - **Session management**: [`Session`], [`SessionConfig`], [`SessionStore`], [`SessionMiddleware`], [`MemorySessionStore`]
//! - **Security & RBAC**: [`AuthContext`], [`Permission`], [`Role`]
//! - **Derive macros** (with `macros` feature): `DeclarativeModel`, `RbacRole`, `SchemaEvolution`, etc.
//! - **Clustering**: [`ClusterArgs`]
//! - **Schema**: [`SchemaMigrationMode`]

// === Derive macros (from lithair-macros) ===
#[cfg(feature = "macros")]
pub use crate::{
    lithair_api, lithair_model, DeclarativeModel, LifecycleAware, Page, RaftstoneModel, RbacRole,
    SchemaEvolution,
};

// === Server builder ===
pub use crate::app::LithairServer;
pub use crate::app::LithairServerBuilder;
pub use crate::app::ModelHandler;

// === Configuration ===
pub use crate::config::LithairConfig;
pub use crate::logging::LoggingConfig;

// === Engine and application traits ===
pub use crate::engine::RaftstoneApplication;
pub use crate::engine::StateEngine;

// === HTTP types ===
pub use crate::http::declarative_server::GzipConfig;
pub use crate::http::DeclarativeHttpHandler;
pub use crate::http::FirewallConfig;
pub use crate::http::HttpMethod;
pub use crate::http::HttpResponse;
pub use crate::http::HttpServer;
pub use crate::http::Route;

// === HTTP essentials (re-exported from the `http` crate) ===
pub use http::Method;
pub use http::Request;
pub use http::Response;
pub use http::StatusCode;

// === Response body helpers (re-exported from `http-body-util`) ===
pub use http_body_util::Full;

// === Frontend ===
pub use crate::frontend::FrontendEngine;
pub use crate::frontend::FrontendServer;

// === Session management ===
pub use crate::session::MemorySessionStore;
pub use crate::session::Session;
pub use crate::session::SessionConfig;
pub use crate::session::SessionMiddleware;
pub use crate::session::SessionStore;

// === Security ===
pub use crate::security::AuthContext;
pub use crate::security::Permission;
pub use crate::security::Role;

// === Clustering ===
pub use crate::cluster::ClusterArgs;

// === Schema ===
pub use crate::config::SchemaMigrationMode;

// === Model inspection ===
pub use crate::model_inspect::Inspectable;
