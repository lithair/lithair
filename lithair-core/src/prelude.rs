//! Prelude module for convenient imports.
//!
//! Import everything you need with a single line:
//!
//! ```rust,ignore
//! use lithair_core::prelude::*;
//! ```

// Derive macros (re-exported from lithair-macros)
#[cfg(feature = "macros")]
pub use crate::{
    DeclarativeModel, LifecycleAware, Page, RbacRole, RaftstoneModel, SchemaEvolution,
    lithair_api, lithair_model,
};

// Server builder
pub use crate::app::LithairServer;
pub use crate::app::LithairServerBuilder;
pub use crate::app::ModelHandler;

// Engine and application traits
pub use crate::engine::RaftstoneApplication;
pub use crate::engine::StateEngine;

// HTTP types
pub use crate::http::HttpMethod;
pub use crate::http::HttpResponse;
pub use crate::http::HttpServer;
pub use crate::http::Route;

// Security
pub use crate::security::AuthContext;
pub use crate::security::Permission;
pub use crate::security::Role;

// Model inspection
pub use crate::model_inspect::Inspectable;
