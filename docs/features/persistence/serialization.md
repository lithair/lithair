# Dual-Mode Serialization (JSON + rkyv)

High-performance serialization system supporting both human-readable JSON and zero-copy binary formats.

## Architecture

```
                    ┌─────────────────────────────────────┐
                    │         SerializationMode           │
                    │  ┌─────────────┬─────────────────┐  │
                    │  │    Json     │     Binary      │  │
                    │  │  (default)  │    (rkyv)       │  │
                    │  └─────────────┴─────────────────┘  │
                    └─────────────────────────────────────┘
                                    │
           ┌────────────────────────┴────────────────────────┐
           ▼                                                 ▼
    ┌─────────────────┐                           ┌─────────────────┐
    │    json_mode    │                           │   binary_mode   │
    │                 │                           │                 │
    │  • simd-json    │                           │  • rkyv 0.8     │
    │  • ~3 GB/s      │                           │  • ~10 GB/s     │
    │  • Human-read   │                           │  • Zero-copy    │
    └─────────────────┘                           └─────────────────┘
```

## Content-Type Negotiation

The serialization mode is selected via HTTP headers:

| Accept Header | Mode | Content-Type Response |
|--------------|------|----------------------|
| `application/json` | Json | `application/json` |
| `application/octet-stream` | Binary | `application/octet-stream` |
| `application/x-rkyv` | Binary | `application/octet-stream` |
| `text/html` | Json | `application/json` |
| `*/*` | Json | `application/json` |

```rust
use lithair_core::serialization::SerializationMode;

let mode = SerializationMode::from_accept("application/octet-stream");
assert_eq!(mode, SerializationMode::Binary);
assert_eq!(mode.content_type(), "application/octet-stream");
```

## JSON Mode (simd-json)

SIMD-accelerated JSON parsing using CPU vector instructions (AVX2/SSE4.2).

### Performance
- **Serialize**: ~50-100 MB/s (serde_json compatible)
- **Deserialize**: ~3 GB/s with SIMD acceleration

### Usage

```rust
use lithair_core::serialization::json_mode;

// Serialize to JSON string
let json = json_mode::serialize(&my_struct)?;

// Serialize to bytes
let bytes = json_mode::serialize_bytes(&my_struct)?;

// Deserialize (immutable, copies data)
let obj: MyStruct = json_mode::deserialize_immutable(&bytes)?;

// Deserialize (mutable, in-place, faster)
let mut bytes = json_data.into_bytes();
let obj: MyStruct = json_mode::deserialize_mutable(&mut bytes)?;
```

### When to Use
- HTTP API responses (human-readable)
- Debugging and logging
- Interoperability with external systems
- Configuration files

## Binary Mode (rkyv)

Zero-copy deserialization - access data directly from bytes without allocation.

### Performance
- **Serialize**: ~500+ MB/s
- **Deserialize**: ~10 GB/s (zero-copy access)
- **Storage**: Often smaller than JSON

### Usage

```rust
use lithair_core::serialization::binary_mode;

// Your type must derive rkyv traits
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(derive(Debug))]
struct MyStruct {
    id: String,
    value: i64,
}

// Serialize using the macro
let bytes = binary_mode::serialize!(MyStruct, &my_struct)?;

// Deserialize using the macro
let obj: MyStruct = binary_mode::deserialize!(MyStruct, &bytes)?;

// Zero-copy access (no allocation!)
let archived = binary_mode::access!(MyStruct, &bytes)?;
println!("ID: {}", archived.id); // Direct access, no copy
```

### When to Use
- Internal storage (event log, snapshots)
- High-throughput scenarios
- Memory-constrained environments
- Raft log replication

## Dual-Mode Types

Types that support both serialization modes:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]           // JSON support
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)] // rkyv support
#[rkyv(derive(Debug))]
pub struct Event {
    pub id: String,
    pub timestamp: i64,
    pub payload: Vec<u8>,
}
```

## DualModeSerializer Trait

For types that need custom serialization logic:

```rust
use lithair_core::serialization::{
    DualModeSerializer, DualModeResult, SerializationMode
};

impl DualModeSerializer for MyType {
    fn serialize_dual(&self, mode: SerializationMode) -> DualModeResult<Vec<u8>> {
        match mode {
            SerializationMode::Json => {
                json_mode::serialize_bytes(self)
            }
            SerializationMode::Binary => {
                binary_mode::serialize!(MyType, self)
            }
        }
    }

    fn deserialize_dual(data: &[u8], mode: SerializationMode) -> DualModeResult<Self> {
        match mode {
            SerializationMode::Json => {
                json_mode::deserialize_immutable(data)
            }
            SerializationMode::Binary => {
                binary_mode::deserialize!(MyType, data)
            }
        }
    }
}
```

## Error Handling

```rust
use lithair_core::serialization::DualModeError;

match result {
    Err(DualModeError::JsonSerializeError(e)) => {
        // JSON serialization failed
    }
    Err(DualModeError::JsonDeserializeError(e)) => {
        // JSON parsing failed (includes position info)
    }
    Err(DualModeError::RkyvSerializeError(e)) => {
        // rkyv serialization failed
    }
    Err(DualModeError::RkyvDeserializeError(e)) => {
        // rkyv deserialization failed
    }
    Err(DualModeError::RkyvValidationError(e)) => {
        // rkyv data validation failed (corrupted data)
    }
    Ok(data) => { /* success */ }
}
```

## Benchmarks

Tested with 10,000 small objects (3 fields each):

| Operation | JSON (simd-json) | Binary (rkyv) | Speedup |
|-----------|-----------------|---------------|---------|
| Serialize | ~50 MB/s | ~500 MB/s | ~10x |
| Deserialize | ~200 MB/s | ~2000 MB/s | ~10x |
| Size | 100% (baseline) | ~60-80% | ~25% smaller |

*Note: Actual performance depends on data structure and CPU capabilities.*

## Configuration

Environment variables:

```bash
# Force JSON mode even for internal storage (debugging)
OMNILITH_FORCE_JSON=1

# Disable SIMD acceleration (fallback to standard serde_json)
OMNILITH_NO_SIMD=1
```

## Migration from JSON-only

1. Add rkyv derives to your types:
```rust
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(derive(Debug))]
```

2. Use `DualModeSerializer` trait for automatic mode selection

3. Existing JSON data remains readable (backward compatible)

## See Also

- [Event Store](./event-store.md) - How events are persisted
- [Snapshots](./snapshots.md) - Fast recovery with binary snapshots
- [Optimized Storage](./optimized-storage.md) - Storage format details
