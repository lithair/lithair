# Lithair Stress Tests - 1 Million Articles

## Overview

Cucumber test suite to validate the **performance**, **durability** and **consistency** of Lithair at large scale.

## Test Scenarios

### 1. **ULTIMATE STRESS TEST - 1 MILLION articles**

**File**: `features/performance/stress_1m_test.feature`

**Operations:**

- 1,000,000 creates (CREATE)
- 200,000 updates (UPDATE) - 20%
- 100,000 deletions (DELETE) - 10%
- **Final state**: 900,000 active articles

**Verifications:**

- 1,300,000 persisted events (1M + 200K + 100K)
- Chronological event ordering
- Memory/disk consistency (SCC2 vs FileStorage)
- Validated checksums
- Performance metrics

**Run:**

```bash
cd cucumber-tests
cargo test --test database_perf_test --release
```

---

### 2. **Maximum Performance - 500K articles**

**Mode**: `DurabilityMode::Performance`

**Objective:**

- Throughput > 20,000 articles/sec
- Total time < 30 seconds
- Deletion throughput > 15,000 articles/sec

**Characteristics:**

- Maximum performance
- Risk of max 10ms data loss
- Measures theoretical limits

---

### 3. **Guaranteed Consistency - 100K articles**

**Mode**: `DurabilityMode::MaxDurability` (DEFAULT)

**Operations:**

- 100,000 CREATE
- 50,000 UPDATE
- 25,000 DELETE
- Final state: 75,000 articles

**Guarantees:**

- **ZERO data loss**
- fsync after each batch
- Memory/disk consistency validated
- All events persisted

---

### 4. **Resilience - 10K random operations**

**Distribution:**

- 50% CREATE
- 30% UPDATE (if articles exist)
- 20% DELETE (if articles exist)

**Validation:**

- All events persisted
- Memory/disk consistency
- No concurrency errors

---

## Expected Performance

### Full Async + SCC2 + MaxDurability Architecture

| Operation       | Throughput   | Latency P50 | Latency P99 |
| --------------- | ------------ | ----------- | ----------- |
| **CREATE**      | 10-30K/sec   | 5-10ms      | 20-50ms     |
| **READ** (SCC2) | 40M+ ops/sec | < 1us       | < 10us      |
| **UPDATE**      | 5-15K/sec    | 10-20ms     | 50-100ms    |
| **DELETE**      | 5-15K/sec    | 10-20ms     | 50-100ms    |

**Note**: With `DurabilityMode::Performance`, throughput is 3-5x higher but with risk of data loss.

---

## Durability Modes

### MaxDurability (DEFAULT - Production)

```rust
// Default in tests
let writer = AsyncWriter::new(storage, 1000);
```

**Guarantees:**

- ZERO data loss
- fsync after each batch
- PostgreSQL/MySQL compliant

**Performance:**

- 10,000-30,000 writes/sec (depending on disk)

### Performance (Benchmarks only)

```gherkin
Given the durability mode is "Performance"
```

**Characteristics:**

- 30,000-100,000 writes/sec
- Max 10ms loss if crash

**NEVER use in production!**

---

## Integrity Verifications

### 1. **Complete persistence**

```gherkin
Then the events.raftlog file must exist
And the events.raftlog file must contain exactly 1000000 "ArticleCreated" events
```

### 2. **Memory/disk consistency**

```gherkin
Then the number of articles in memory must equal the number on disk
```

Verifies that **SCC2 (RAM)** and **FileStorage (disk)** are synchronized.

### 3. **Chronological order**

```gherkin
And all events must be in chronological order
```

Guarantees event sourcing integrity.

### 4. **Checksums**

```gherkin
And all checksums must match
```

Data corruption detection.

---

## Collected Metrics

### Final Statistics

```
+======================================+
|   FINAL STATISTICS                   |
+======================================+
| Total requests:          1,300,000   |
| Total duration:               65.32s |
| Throughput:              19,902/sec  |
| Errors:                         0   |
+======================================+
```

### Per Operation

- **Creation throughput**: ops/sec
- **Update throughput**: ops/sec
- **Deletion throughput**: ops/sec

---

## Running the Tests

### Full 1M test

```bash
cd cucumber-tests
cargo test --test database_perf_test --release
```

### Specific test

```bash
# Durability test only
cargo test --release -- "Mode MaxDurability"

# Performance test only
cargo test --release -- "Maximum performance"
```

### With detailed logs

```bash
RUST_LOG=debug cargo test --test database_perf_test --release
```

---

## Expected Results

### Success

- All events persisted (100%)
- Memory/disk consistency validated
- Correct checksums
- Throughput meets expectations

### Possible Warnings

- Network timeouts under heavy load
- Increased latency with MaxDurability (normal)
- Slowdowns with traditional HDD

### Failures

- Event loss -> CRITICAL BUG
- Memory/disk inconsistency -> CRITICAL BUG
- Invalid checksum -> DATA CORRUPTION

---

## Configuration

### AsyncWriter Batch Size

```rust
const BATCH_SIZE: usize = 1000;
```

- Smaller -> Reduced latency, lower throughput
- Larger -> Higher throughput, increased latency

### Flush Interval (Performance mode)

```rust
const FLUSH_INTERVAL_MS: u64 = 10;
```

- Shorter -> Less potential loss
- Longer -> Better throughput

---

## Notes

### SSD vs HDD

- **SSD NVMe**: ~10,000 fsync/sec -> Excellent with MaxDurability
- **SSD SATA**: ~5,000 fsync/sec -> Good with MaxDurability
- **HDD 7200rpm**: ~100-500 fsync/sec -> Slow with MaxDurability

### Production Recommendations

1. **Always** use `DurabilityMode::MaxDurability`
2. Use an **SSD** for events
3. Batch size **1000** (optimal balance)
4. Monitor **persistence metrics**

---

## Next Steps

- [ ] WAL Mode (Write-Ahead Log)
- [ ] Event compression
- [ ] Distributed cluster tests (multi-node)
- [ ] Benchmarks vs PostgreSQL/MongoDB
- [ ] Crash recovery tests

---

**Lithair - Event-Sourced Database with Guaranteed Durability**
