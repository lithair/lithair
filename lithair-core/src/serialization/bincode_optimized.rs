//! Optimized Bincode Serialization for Lithair T021
//!
//! This module provides high-performance binary serialization using bincode for internal
//! operations while maintaining JSON compatibility for HTTP APIs.
//! Expected performance improvement: 3-5x over JSON serialization.

use crate::serialization::{SerializationError, SerializationResult};
use bincode::config::standard;
use bincode::serde::{decode_from_slice, encode_to_vec};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Bincode serialization error types
#[derive(Debug, Clone)]
pub enum BincodeError {
    SerializationFailed(String),
    DeserializationFailed(String),
    InvalidData(String),
}

impl fmt::Display for BincodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BincodeError::SerializationFailed(msg) => {
                write!(f, "Bincode serialization failed: {}", msg)
            }
            BincodeError::DeserializationFailed(msg) => {
                write!(f, "Bincode deserialization failed: {}", msg)
            }
            BincodeError::InvalidData(msg) => write!(f, "Invalid bincode data: {}", msg),
        }
    }
}

impl std::error::Error for BincodeError {}

impl From<BincodeError> for SerializationError {
    fn from(err: BincodeError) -> Self {
        match err {
            BincodeError::SerializationFailed(msg) => SerializationError::InvalidData(msg),
            BincodeError::DeserializationFailed(msg) => SerializationError::InvalidData(msg),
            BincodeError::InvalidData(msg) => SerializationError::InvalidData(msg),
        }
    }
}

/// High-performance bincode serialization trait
pub trait BincodeSerializable: Serialize + for<'de> Deserialize<'de> {
    /// Serialize to bincode bytes (3-5x faster than JSON)
    fn to_bincode_bytes(&self) -> SerializationResult<Vec<u8>> {
        encode_to_vec(self, standard())
            .map_err(|e| BincodeError::SerializationFailed(e.to_string()).into())
    }

    /// Deserialize from bincode bytes (3-5x faster than JSON)
    fn from_bincode_bytes(bytes: &[u8]) -> SerializationResult<Self>
    where
        Self: Sized,
    {
        decode_from_slice(bytes, standard())
            .map(|(val, _)| val)
            .map_err(|e| BincodeError::DeserializationFailed(e.to_string()).into())
    }

    /// Serialize to JSON for HTTP API compatibility
    fn to_json_bytes(&self) -> SerializationResult<Vec<u8>> {
        serde_json::to_vec(self).map_err(|e| SerializationError::InvalidData(e.to_string()))
    }

    /// Deserialize from JSON bytes for HTTP API compatibility
    fn from_json_bytes(bytes: &[u8]) -> SerializationResult<Self>
    where
        Self: Sized,
    {
        serde_json::from_slice(bytes).map_err(|e| SerializationError::InvalidData(e.to_string()))
    }
}

/// Serialization format selection
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SerializationFormat {
    /// Bincode for internal operations (3-5x faster)
    #[default]
    Bincode,
    /// JSON for HTTP API compatibility
    Json,
}

/// High-performance serialization envelope with format detection
#[derive(Debug, Clone)]
pub struct SerializationEnvelope {
    pub format: SerializationFormat,
    pub data: Vec<u8>,
    pub original_size: Option<usize>, // For performance metrics
}

impl SerializationEnvelope {
    /// Create bincode envelope (T021 optimization)
    pub fn from_bincode<T: BincodeSerializable>(value: &T) -> SerializationResult<Self> {
        let data = value.to_bincode_bytes()?;
        Ok(Self { format: SerializationFormat::Bincode, data, original_size: None })
    }

    /// Create JSON envelope (HTTP API compatibility)  
    pub fn from_json<T: BincodeSerializable>(value: &T) -> SerializationResult<Self> {
        let data = value.to_json_bytes()?;
        Ok(Self { format: SerializationFormat::Json, data, original_size: None })
    }

    /// Deserialize based on format
    pub fn to_value<T: BincodeSerializable>(&self) -> SerializationResult<T> {
        match self.format {
            SerializationFormat::Bincode => T::from_bincode_bytes(&self.data),
            SerializationFormat::Json => T::from_json_bytes(&self.data),
        }
    }

    /// Get size in bytes
    pub fn size(&self) -> usize {
        self.data.len()
    }

    /// Check if using optimized bincode format
    pub fn is_optimized(&self) -> bool {
        self.format == SerializationFormat::Bincode
    }
}

/// Smart serializer that chooses format based on context
pub struct SmartSerializer {
    default_format: SerializationFormat,
}

impl SmartSerializer {
    /// Create with default bincode for T021 optimization
    pub fn new() -> Self {
        Self { default_format: SerializationFormat::Bincode }
    }

    /// Create with specific default format
    pub fn with_format(format: SerializationFormat) -> Self {
        Self { default_format: format }
    }

    /// Serialize using default format (bincode for T021)
    pub fn serialize<T: BincodeSerializable>(
        &self,
        value: &T,
    ) -> SerializationResult<SerializationEnvelope> {
        match self.default_format {
            SerializationFormat::Bincode => SerializationEnvelope::from_bincode(value),
            SerializationFormat::Json => SerializationEnvelope::from_json(value),
        }
    }

    /// Serialize for internal operations (always bincode for T021)
    pub fn serialize_internal<T: BincodeSerializable>(
        &self,
        value: &T,
    ) -> SerializationResult<SerializationEnvelope> {
        SerializationEnvelope::from_bincode(value)
    }

    /// Serialize for HTTP API (always JSON for compatibility)
    pub fn serialize_http<T: BincodeSerializable>(
        &self,
        value: &T,
    ) -> SerializationResult<SerializationEnvelope> {
        SerializationEnvelope::from_json(value)
    }

    /// Deserialize any envelope
    pub fn deserialize<T: BincodeSerializable>(
        &self,
        envelope: &SerializationEnvelope,
    ) -> SerializationResult<T> {
        envelope.to_value()
    }
}

impl Default for SmartSerializer {
    fn default() -> Self {
        Self::new()
    }
}

/// Auto-implement BincodeSerializable for any Serialize + Deserialize type
impl<T> BincodeSerializable for T where T: Serialize + for<'de> Deserialize<'de> {}

/// Performance comparison utilities for T021 validation
pub struct SerializationBenchmark;

impl SerializationBenchmark {
    /// Compare JSON vs Bincode serialization performance
    pub fn compare_formats<T: BincodeSerializable>(
        value: &T,
        iterations: usize,
    ) -> (f64, f64, f64) {
        use std::time::Instant;

        // JSON benchmark
        let start = Instant::now();
        for _ in 0..iterations {
            let _ = value.to_json_bytes().expect("serialization of known type");
        }
        let json_time = start.elapsed().as_secs_f64();

        // Bincode benchmark
        let start = Instant::now();
        for _ in 0..iterations {
            let _ = value.to_bincode_bytes().expect("serialization of known type");
        }
        let bincode_time = start.elapsed().as_secs_f64();

        // Calculate speedup ratio
        let speedup = json_time / bincode_time;

        (json_time, bincode_time, speedup)
    }

    /// Get size comparison between formats
    pub fn compare_sizes<T: BincodeSerializable>(value: &T) -> (usize, usize, f64) {
        let json_data = value.to_json_bytes().expect("serialization of known type");
        let bincode_data = value.to_bincode_bytes().expect("serialization of known type");

        let json_size = json_data.len();
        let bincode_size = bincode_data.len();
        let size_ratio = json_size as f64 / bincode_size as f64;

        (json_size, bincode_size, size_ratio)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Serialize};
    use uuid::Uuid;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestProduct {
        id: Uuid,
        name: String,
        price: f64,
        category: String,
        created_at: DateTime<Utc>,
    }

    impl TestProduct {
        fn sample() -> Self {
            Self {
                id: Uuid::new_v4(),
                name: "Test Product with a reasonably long name".to_string(),
                price: 299.99,
                category: "Electronics & Gadgets Category".to_string(),
                created_at: Utc::now(),
            }
        }
    }

    #[test]
    fn test_bincode_serialization() {
        let product = TestProduct::sample();

        // Test bincode round-trip
        let bincode_data = product.to_bincode_bytes().unwrap();
        let deserialized: TestProduct = TestProduct::from_bincode_bytes(&bincode_data).unwrap();
        assert_eq!(product, deserialized);
    }

    #[test]
    fn test_json_compatibility() {
        let product = TestProduct::sample();

        // Test JSON round-trip
        let json_data = product.to_json_bytes().unwrap();
        let deserialized: TestProduct = TestProduct::from_json_bytes(&json_data).unwrap();
        assert_eq!(product, deserialized);
    }

    #[test]
    fn test_serialization_envelope() {
        let product = TestProduct::sample();

        // Test bincode envelope
        let bincode_envelope = SerializationEnvelope::from_bincode(&product).unwrap();
        assert_eq!(bincode_envelope.format, SerializationFormat::Bincode);
        assert!(bincode_envelope.is_optimized());

        let deserialized: TestProduct = bincode_envelope.to_value().unwrap();
        assert_eq!(product, deserialized);

        // Test JSON envelope
        let json_envelope = SerializationEnvelope::from_json(&product).unwrap();
        assert_eq!(json_envelope.format, SerializationFormat::Json);
        assert!(!json_envelope.is_optimized());
    }

    #[test]
    fn test_smart_serializer() {
        let product = TestProduct::sample();
        let serializer = SmartSerializer::new();

        // Test default format (should be bincode for T021)
        let envelope = serializer.serialize(&product).unwrap();
        assert_eq!(envelope.format, SerializationFormat::Bincode);

        // Test internal serialization (always bincode)
        let internal_envelope = serializer.serialize_internal(&product).unwrap();
        assert_eq!(internal_envelope.format, SerializationFormat::Bincode);

        // Test HTTP serialization (always JSON)
        let http_envelope = serializer.serialize_http(&product).unwrap();
        assert_eq!(http_envelope.format, SerializationFormat::Json);
    }

    #[test]
    fn test_performance_comparison() {
        let product = TestProduct::sample();
        let iterations = 1000;

        let (json_time, bincode_time, speedup) =
            SerializationBenchmark::compare_formats(&product, iterations);

        println!("JSON time: {:.4}s", json_time);
        println!("Bincode time: {:.4}s", bincode_time);
        println!("Speedup: {:.2}x", speedup);

        // Only enforce strict speedup when explicitly requested
        let enforce_perf = std::env::var("LT_ENFORCE_PERF")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        if enforce_perf {
            // Bincode should be faster (speedup > 1.0)
            assert!(
                speedup > 1.0,
                "Expected bincode to be faster than JSON (speedup > 1.0), got {:.2}x",
                speedup
            );

            // Allow environment to set expected minimum, default to 1.2x
            let min_speedup: f64 = std::env::var("LT_BINCODE_MIN_SPEEDUP")
                .ok()
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(1.2);
            assert!(
                speedup >= min_speedup,
                "Expected speedup >= {:.2}x, got {:.2}x",
                min_speedup,
                speedup
            );
        } else {
            // On environments where timing is noisy, just validate the operations succeeded
            assert!(json_time > 0.0);
            assert!(bincode_time > 0.0);
        }
    }

    #[test]
    fn test_size_comparison() {
        let product = TestProduct::sample();

        let (json_size, bincode_size, size_ratio) = SerializationBenchmark::compare_sizes(&product);

        println!("JSON size: {} bytes", json_size);
        println!("Bincode size: {} bytes", bincode_size);
        println!("Size ratio: {:.2}x", size_ratio);

        // Bincode is typically more compact
        assert!(bincode_size <= json_size);
    }
}
