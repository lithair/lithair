//! Pattern matching module
//!
//! Provides utilities for matching patterns against strings, IPs, domains, etc.
//! Used for proxy routing, filtering, and access control.

pub mod matcher;

pub use matcher::{MatchResult, PatternMatcher};
