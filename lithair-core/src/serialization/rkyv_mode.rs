//! Dual-Mode Serialization: JSON (simd-json) + Binary (rkyv)
//!
//! This module provides ultra-high-performance serialization with two modes:
//! - **Json**: Human-readable, debuggable, uses simd-json (~3GB/s parsing)
//! - **Binary**: Zero-copy, internal use, uses rkyv (~10GB/s)
//!
//! # Performance Comparison
//!
//! | Mode   | Serialize | Deserialize | Zero-Copy | Human Readable |
//! |--------|-----------|-------------|-----------|----------------|
//! | Json   | ~300MB/s  | ~3GB/s      | No        | Yes            |
//! | Binary | ~5GB/s    | ~10GB/s     | Yes       | No             |

use serde::{de::DeserializeOwned, Serialize as SerdeSerialize};
use std::fmt;

/// Serialization mode for Lithair
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SerializationMode {
    /// Human-readable JSON using simd-json for fast parsing
    /// Best for: HTTP APIs, debugging, external clients
    #[default]
    Json,

    /// Zero-copy binary using rkyv
    /// Best for: Storage, Raft replication, internal communication
    Binary,
}

impl fmt::Display for SerializationMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SerializationMode::Json => write!(f, "json"),
            SerializationMode::Binary => write!(f, "binary"),
        }
    }
}

impl SerializationMode {
    /// Get content-type header for HTTP responses
    pub fn content_type(&self) -> &'static str {
        match self {
            SerializationMode::Json => "application/json",
            SerializationMode::Binary => "application/octet-stream",
        }
    }

    /// Parse from Accept header or query parameter
    pub fn from_accept(accept: &str) -> Self {
        if accept.contains("octet-stream") || accept.contains("application/x-rkyv") {
            SerializationMode::Binary
        } else {
            SerializationMode::Json
        }
    }
}

/// Error types for dual-mode serialization
#[derive(Debug, Clone)]
pub enum DualModeError {
    JsonSerializeError(String),
    JsonDeserializeError(String),
    RkyvSerializeError(String),
    RkyvDeserializeError(String),
    RkyvValidationError(String),
    InvalidData(String),
}

impl fmt::Display for DualModeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DualModeError::JsonSerializeError(msg) => write!(f, "JSON serialize error: {}", msg),
            DualModeError::JsonDeserializeError(msg) => {
                write!(f, "JSON deserialize error: {}", msg)
            }
            DualModeError::RkyvSerializeError(msg) => write!(f, "rkyv serialize error: {}", msg),
            DualModeError::RkyvDeserializeError(msg) => {
                write!(f, "rkyv deserialize error: {}", msg)
            }
            DualModeError::RkyvValidationError(msg) => write!(f, "rkyv validation error: {}", msg),
            DualModeError::InvalidData(msg) => write!(f, "Invalid data: {}", msg),
        }
    }
}

impl std::error::Error for DualModeError {}

pub type DualModeResult<T> = Result<T, DualModeError>;

// ============================================================================
// JSON Mode (simd-json)
// ============================================================================

/// Fast JSON serialization using simd-json
pub mod json_mode {
    use super::*;

    /// Serialize to JSON string using serde_json (simd-json for parsing only)
    pub fn serialize<T: SerdeSerialize>(value: &T) -> DualModeResult<String> {
        serde_json::to_string(value).map_err(|e| DualModeError::JsonSerializeError(e.to_string()))
    }

    /// Serialize to JSON bytes
    pub fn serialize_bytes<T: SerdeSerialize>(value: &T) -> DualModeResult<Vec<u8>> {
        serde_json::to_vec(value).map_err(|e| DualModeError::JsonSerializeError(e.to_string()))
    }

    /// Deserialize from JSON using simd-json (SIMD-accelerated, ~3GB/s)
    pub fn deserialize<T: DeserializeOwned>(data: &mut [u8]) -> DualModeResult<T> {
        simd_json::from_slice(data).map_err(|e| DualModeError::JsonDeserializeError(e.to_string()))
    }

    /// Deserialize from JSON string (creates a copy for simd-json)
    pub fn deserialize_str<T: DeserializeOwned>(data: &str) -> DualModeResult<T> {
        let mut bytes = data.as_bytes().to_vec();
        deserialize(&mut bytes)
    }

    /// Deserialize using standard serde_json (for immutable data)
    pub fn deserialize_immutable<T: DeserializeOwned>(data: &[u8]) -> DualModeResult<T> {
        serde_json::from_slice(data).map_err(|e| DualModeError::JsonDeserializeError(e.to_string()))
    }
}

// ============================================================================
// Binary Mode (rkyv) - Zero-Copy
// ============================================================================

/// Zero-copy binary serialization using rkyv
///
/// Note: rkyv 0.8 has complex trait bounds. For concrete types, use the
/// derive macros directly: `#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]`
///
/// This module provides the infrastructure and error handling.
/// Actual serialization happens at the call site with concrete types.
pub mod binary_mode {
    use super::*;

    // Re-export rkyv types for convenience
    pub use rkyv::api::high::HighValidator;
    pub use rkyv::bytecheck::CheckBytes;
    pub use rkyv::rancor::Error as RkyvError;
    pub use rkyv::util::AlignedVec;
    pub use rkyv::{Archive, Deserialize, Serialize};

    /// Serialize a value that implements rkyv's Archive + Serialize
    ///
    /// Usage:
    /// ```ignore
    /// #[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
    /// #[rkyv(check_bytes)]
    /// struct MyType { ... }
    ///
    /// let bytes = binary_mode::serialize_value(&my_value)?;
    /// ```
    #[inline]
    pub fn serialize_to_bytes<E: rkyv::rancor::Source>(
        result: Result<AlignedVec, E>,
    ) -> DualModeResult<Vec<u8>> {
        result
            .map(|v| v.to_vec())
            .map_err(|e| DualModeError::RkyvSerializeError(e.to_string()))
    }

    /// Convert deserialization result to DualModeResult
    #[inline]
    pub fn deserialize_result<T, E: rkyv::rancor::Source>(
        result: Result<T, E>,
    ) -> DualModeResult<T> {
        result.map_err(|e| DualModeError::RkyvDeserializeError(e.to_string()))
    }

    /// Convert access/validation result to DualModeResult
    #[inline]
    pub fn access_result<'a, T, E: rkyv::rancor::Source>(
        result: Result<&'a T, E>,
    ) -> DualModeResult<&'a T> {
        result.map_err(|e| DualModeError::RkyvValidationError(e.to_string()))
    }

    /// Helper macro for serializing rkyv types
    /// Returns DualModeResult<Vec<u8>>
    #[macro_export]
    macro_rules! rkyv_serialize {
        ($value:expr) => {
            $crate::serialization::rkyv_mode::binary_mode::serialize_to_bytes(rkyv::to_bytes::<
                rkyv::rancor::Error,
            >($value))
        };
    }

    /// Helper macro for deserializing rkyv types
    /// Returns DualModeResult<T>
    #[macro_export]
    macro_rules! rkyv_deserialize {
        ($type:ty, $data:expr) => {
            $crate::serialization::rkyv_mode::binary_mode::deserialize_result(rkyv::from_bytes::<
                $type,
                rkyv::rancor::Error,
            >($data))
        };
    }

    /// Helper macro for zero-copy access to archived data
    /// Returns DualModeResult<&Archived<T>>
    #[macro_export]
    macro_rules! rkyv_access {
        ($type:ty, $data:expr) => {
            $crate::serialization::rkyv_mode::binary_mode::access_result(rkyv::access::<
                <$type as rkyv::Archive>::Archived,
                rkyv::rancor::Error,
            >($data))
        };
    }

    pub use rkyv_access;
    pub use rkyv_deserialize;
    pub use rkyv_serialize;
}

// ============================================================================
// Dual-Mode Serializer
// ============================================================================

/// Dual-mode serializer that can switch between JSON and Binary
#[derive(Debug, Clone)]
pub struct DualModeSerializer {
    mode: SerializationMode,
}

impl DualModeSerializer {
    /// Create a new serializer with the specified mode
    pub fn new(mode: SerializationMode) -> Self {
        Self { mode }
    }

    /// Create a JSON mode serializer
    pub fn json() -> Self {
        Self::new(SerializationMode::Json)
    }

    /// Create a Binary mode serializer
    pub fn binary() -> Self {
        Self::new(SerializationMode::Binary)
    }

    /// Get current mode
    pub fn mode(&self) -> SerializationMode {
        self.mode
    }

    /// Get content-type for HTTP responses
    pub fn content_type(&self) -> &'static str {
        self.mode.content_type()
    }
}

impl Default for DualModeSerializer {
    fn default() -> Self {
        Self::json()
    }
}

// ============================================================================
// Benchmark utilities
// ============================================================================

/// Benchmark results for serialization comparison
#[derive(Debug, Clone)]
pub struct SerializationBenchmarkResult {
    pub mode: SerializationMode,
    pub serialize_ns: u64,
    pub deserialize_ns: u64,
    pub size_bytes: usize,
    pub throughput_mb_s: f64,
}

impl fmt::Display for SerializationBenchmarkResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: serialize={}ns, deserialize={}ns, size={}B, throughput={:.2}MB/s",
            self.mode,
            self.serialize_ns,
            self.deserialize_ns,
            self.size_bytes,
            self.throughput_mb_s
        )
    }
}

/// Run a quick benchmark comparing JSON vs Binary serialization for serde types
pub fn benchmark_json<T>(value: &T, iterations: usize) -> SerializationBenchmarkResult
where
    T: SerdeSerialize + DeserializeOwned + Clone,
{
    use std::time::Instant;

    // JSON benchmark
    let start = Instant::now();
    let mut json_bytes = Vec::new();
    for _ in 0..iterations {
        json_bytes = json_mode::serialize_bytes(value).unwrap();
    }
    let serialize_time = start.elapsed().as_nanos() as u64 / iterations as u64;

    let deser_start = Instant::now();
    for _ in 0..iterations {
        let _: T = json_mode::deserialize_immutable(&json_bytes).unwrap();
    }
    let deserialize_time = deser_start.elapsed().as_nanos() as u64 / iterations as u64;

    let total_time = start.elapsed().as_secs_f64();
    let throughput =
        (json_bytes.len() as f64 * iterations as f64 * 2.0) / (total_time * 1_000_000.0);

    SerializationBenchmarkResult {
        mode: SerializationMode::Json,
        serialize_ns: serialize_time,
        deserialize_ns: deserialize_time,
        size_bytes: json_bytes.len(),
        throughput_mb_s: throughput,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    struct TestEvent {
        id: String,
        name: String,
        value: i64,
        tags: Vec<String>,
    }

    #[test]
    fn test_json_roundtrip() {
        let event = TestEvent {
            id: "test-123".to_string(),
            name: "Test Event".to_string(),
            value: 42,
            tags: vec!["tag1".to_string(), "tag2".to_string()],
        };

        let json = json_mode::serialize(&event).unwrap();
        let parsed: TestEvent = json_mode::deserialize_str(&json).unwrap();
        assert_eq!(event, parsed);
    }

    #[test]
    fn test_serialization_mode_from_accept() {
        assert_eq!(SerializationMode::from_accept("application/json"), SerializationMode::Json);
        assert_eq!(
            SerializationMode::from_accept("application/octet-stream"),
            SerializationMode::Binary
        );
        assert_eq!(SerializationMode::from_accept("text/html"), SerializationMode::Json);
    }

    #[test]
    fn test_json_benchmark() {
        let event = TestEvent {
            id: "bench-test".to_string(),
            name: "Benchmark Event".to_string(),
            value: 12345,
            tags: vec!["perf".to_string(), "test".to_string()],
        };

        let result = benchmark_json(&event, 1000);
        println!("JSON benchmark: {}", result);
        assert!(result.serialize_ns > 0);
        assert!(result.deserialize_ns > 0);
    }
}
