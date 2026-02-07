//! Helper types and utilities for procedural macros
//!
//! This module contains types and functions that support the procedural macros
//! defined in the lithair-macros crate.

pub mod api;
pub mod model;

#[allow(unused_imports)]
pub use api::ApiGenerator;
#[allow(unused_imports)]
pub use model::ModelGenerator;

/// Helper trait for macro-generated models
#[allow(dead_code)]
pub trait GeneratedModel {
    /// Get the model name
    fn model_name() -> &'static str;

    /// Get the field names
    fn field_names() -> &'static [&'static str];
}

/// Helper trait for API generation
#[allow(dead_code)]
pub trait GeneratedApi<S> {
    /// Get the generated routes
    fn routes() -> Vec<crate::http::Route<S>>;
}
