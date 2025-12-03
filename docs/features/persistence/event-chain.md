# Event Chain & Integrity

Cryptographic event linking system ensuring data integrity and tamper detection.

## Overview

Each event in the store is linked to its predecessor through a chain of CRC32 checksums, creating an immutable audit trail similar to a blockchain.

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│  Event #0   │────▶│  Event #1   │────▶│  Event #2   │────▶│  Event #3   │
│             │     │             │     │             │     │             │
│ prev: 0     │     │ prev: CRC#0 │     │ prev: CRC#1 │     │ prev: CRC#2 │
│ crc: CRC#0  │     │ crc: CRC#1  │     │ crc: CRC#2  │     │ crc: CRC#3  │
└─────────────┘     └─────────────┘     └─────────────┘     └─────────────┘
```

## Event Envelope Structure

```rust
pub struct EventEnvelope {
    /// Unique event identifier
    pub event_id: u64,

    /// Sequential number within the aggregate
    pub sequence: u64,

    /// Aggregate/entity identifier
    pub aggregate_id: String,

    /// Event type name
    pub event_type: String,

    /// Serialized event payload
    pub payload: Vec<u8>,

    /// Timestamp (Unix epoch milliseconds)
    pub timestamp: i64,

    /// CRC32 of previous event (chain link)
    pub prev_crc: u32,

    /// CRC32 of this event (includes prev_crc)
    pub crc32: u32,
}
```

## Chain Verification

### On Write

When appending a new event:

```rust
// 1. Get the last event's CRC
let prev_crc = last_event.map(|e| e.crc32).unwrap_or(0);

// 2. Create new event with prev_crc
let mut new_event = EventEnvelope {
    prev_crc,
    crc32: 0, // Will be computed
    // ... other fields
};

// 3. Compute CRC32 of the entire event (excluding crc32 field)
new_event.crc32 = compute_crc32(&new_event);

// 4. Append to log
event_store.append(new_event);
```

### On Read (Validation)

```rust
pub fn validate_chain(events: &[EventEnvelope]) -> Result<(), ChainError> {
    let mut expected_prev_crc = 0u32;

    for (i, event) in events.iter().enumerate() {
        // Verify chain link
        if event.prev_crc != expected_prev_crc {
            return Err(ChainError::BrokenLink {
                event_index: i,
                expected: expected_prev_crc,
                actual: event.prev_crc,
            });
        }

        // Verify event integrity
        let computed_crc = compute_crc32(event);
        if event.crc32 != computed_crc {
            return Err(ChainError::CorruptedEvent {
                event_index: i,
                expected: computed_crc,
                actual: event.crc32,
            });
        }

        expected_prev_crc = event.crc32;
    }

    Ok(())
}
```

## CRC32 Computation

Using the `crc32fast` crate for hardware-accelerated CRC32:

```rust
use crc32fast::Hasher;

fn compute_crc32(event: &EventEnvelope) -> u32 {
    let mut hasher = Hasher::new();

    // Include all fields except crc32
    hasher.update(&event.event_id.to_le_bytes());
    hasher.update(&event.sequence.to_le_bytes());
    hasher.update(event.aggregate_id.as_bytes());
    hasher.update(event.event_type.as_bytes());
    hasher.update(&event.payload);
    hasher.update(&event.timestamp.to_le_bytes());
    hasher.update(&event.prev_crc.to_le_bytes());

    hasher.finalize()
}
```

## Integrity Guarantees

### What the Chain Protects Against

| Threat | Detection |
|--------|-----------|
| Bit rot (storage corruption) | CRC mismatch |
| Accidental modification | Chain break |
| Event deletion | Sequence gap + chain break |
| Event insertion | Chain break |
| Event reordering | Chain break |
| Payload tampering | CRC mismatch |

### What It Does NOT Protect Against

- Malicious actor with write access (they can recompute the chain)
- Loss of the entire chain
- Denial of service

For stronger guarantees, consider:
- Signed events (cryptographic signatures)
- External audit log
- Raft consensus replication

## Recovery Scenarios

### Scenario 1: Corrupted Event Detected

```
Event #0 ✓ → Event #1 ✓ → Event #2 ✗ (CRC mismatch) → Event #3 ?
```

**Recovery options:**
1. Restore from snapshot before Event #2
2. Replay from replica
3. Manual intervention (if payload is recoverable)

### Scenario 2: Broken Chain

```
Event #0 ✓ → Event #1 ✓ → Event #2 ✗ (wrong prev_crc) → Event #3 ✓
```

**Indicates:** Event #2 was replaced or events were reordered

**Recovery:** Restore from backup or replica

### Scenario 3: Missing Events

```
Event #0 ✓ → Event #1 ✓ → [gap] → Event #5 ✗ (prev_crc doesn't match #1)
```

**Recovery:** Restore missing events from replica

## Performance

CRC32 computation is extremely fast with hardware acceleration:

| Operation | Throughput |
|-----------|------------|
| CRC32 (small event ~100 bytes) | ~10 GB/s |
| CRC32 (large event ~10 KB) | ~15 GB/s |
| Chain validation (1M events) | < 1 second |

**Impact on write path:** Negligible (< 1% overhead)

## Storage Format

Events are stored with the following binary layout:

```
┌────────────────────────────────────────────────────────┐
│ Header (fixed size)                                    │
├────────────────────────────────────────────────────────┤
│ event_id:     u64  (8 bytes)                          │
│ sequence:     u64  (8 bytes)                          │
│ timestamp:    i64  (8 bytes)                          │
│ prev_crc:     u32  (4 bytes)                          │
│ crc32:        u32  (4 bytes)                          │
│ agg_id_len:   u32  (4 bytes)                          │
│ type_len:     u32  (4 bytes)                          │
│ payload_len:  u32  (4 bytes)                          │
├────────────────────────────────────────────────────────┤
│ Variable data                                          │
├────────────────────────────────────────────────────────┤
│ aggregate_id: [u8; agg_id_len]                        │
│ event_type:   [u8; type_len]                          │
│ payload:      [u8; payload_len]                       │
└────────────────────────────────────────────────────────┘
```

## Validation Modes

```rust
pub enum ValidationMode {
    /// Validate every event on read (safest, slower)
    Full,

    /// Validate only on startup and periodic checks
    Periodic,

    /// Skip validation (fastest, trust storage)
    None,
}
```

Configure via environment:

```bash
# Full validation (default in dev)
OMNILITH_CHAIN_VALIDATION=full

# Periodic validation (default in prod)
OMNILITH_CHAIN_VALIDATION=periodic

# No validation (benchmark mode)
OMNILITH_CHAIN_VALIDATION=none
```

## Integration with Snapshots

Snapshots include chain metadata for fast validation:

```rust
pub struct SnapshotMetadata {
    /// Last event included in snapshot
    pub last_event_id: u64,

    /// CRC32 of last event (for chain continuity)
    pub last_crc32: u32,

    /// Total events in snapshot
    pub event_count: u64,

    /// Snapshot creation timestamp
    pub created_at: i64,
}
```

On recovery:
1. Load snapshot
2. Verify `last_crc32` matches stored value
3. Replay events after snapshot
4. Verify chain continuity from `last_crc32`

## See Also

- [Event Store](./event-store.md) - Event storage implementation
- [Snapshots](./snapshots.md) - Snapshot system
- [Dual-Mode Serialization](./serialization.md) - Payload serialization
