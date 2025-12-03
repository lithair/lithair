//! Proxy module for Lithair
//! 
//! Provides generic proxy functionality for forward, reverse, and transparent proxies.
//! This module is designed to be reusable across different proxy implementations.

pub mod traits;
pub mod forward;
pub mod reverse;
pub mod utils;

pub use traits::{ProxyHandler, ProxyRequest, ProxyResponse};
pub use forward::ForwardProxyHandler;
pub use reverse::ReverseProxyHandler;
pub use utils::{ProxyError, ProxyResult};

// Advanced filtering with metadata
pub mod filtering;
pub mod metadata;
pub mod tls;

pub use filtering::FilterListManager;
pub use metadata::{EntryMetadata, FilterEntry, BlockResult, BlockInfo};
pub use tls::{CertificateFingerprint, TlsFingerprinter};
