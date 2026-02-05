//! External integrations module
//!
//! Provides functionality to integrate with external data sources,
//! APIs, and services.

pub mod external_sources;

pub use external_sources::{ExternalSourceFetcher, FetchError, SourceFormat};
