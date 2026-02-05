//! Custom serialization with zero external dependencies
//!
//! This module provides JSON and binary serialization built from scratch
//! using only the Rust standard library.
//!
//! # Dual-Mode Serialization (NEW)
//!
//! Lithair now supports two serialization modes:
//! - **Json**: Human-readable, uses simd-json for ~3GB/s parsing
//! - **Binary**: Zero-copy, uses rkyv for ~10GB/s throughput

pub mod binary;
pub mod bincode_optimized;
pub mod json;
pub mod rkyv_mode;

// Re-export main types
pub use binary::{BinaryEnvelope, BinaryError, BinarySerializable, BinaryStats};
pub use bincode_optimized::{
    BincodeSerializable, SerializationBenchmark, SerializationEnvelope, SerializationFormat,
    SmartSerializer,
};
pub use json::{parse_json, stringify_json, JsonError, JsonValue};

// Re-export dual-mode serialization (NEW)
pub use rkyv_mode::{
    benchmark_json, binary_mode, json_mode, DualModeError, DualModeResult, DualModeSerializer,
    SerializationBenchmarkResult, SerializationMode,
};

/// Result type for serialization operations
pub type SerializationResult<T> = std::result::Result<T, SerializationError>;

/// Serialization error types
#[derive(Debug, Clone)]
pub enum SerializationError {
    JsonError(JsonError),
    BinaryError(BinaryError),
    InvalidFormat(String),
    InvalidData(String),
}

impl std::fmt::Display for SerializationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SerializationError::JsonError(e) => write!(f, "JSON error: {}", e),
            SerializationError::BinaryError(e) => write!(f, "Binary error: {}", e),
            SerializationError::InvalidFormat(msg) => write!(f, "Invalid format: {}", msg),
            SerializationError::InvalidData(msg) => write!(f, "Invalid data: {}", msg),
        }
    }
}

impl std::error::Error for SerializationError {}

impl From<JsonError> for SerializationError {
    fn from(err: JsonError) -> Self {
        SerializationError::JsonError(err)
    }
}

impl From<BinaryError> for SerializationError {
    fn from(err: BinaryError) -> Self {
        SerializationError::BinaryError(err)
    }
}

// Convert to main framework error
impl From<SerializationError> for crate::Error {
    fn from(err: SerializationError) -> Self {
        crate::Error::SerializationError(err.to_string())
    }
}
