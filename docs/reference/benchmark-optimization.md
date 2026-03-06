# Lithair Performance Guide

## 🎯 Overview

Lithair can offer strong performance for in-memory, event-sourced workloads,
especially when you want a smaller deployment surface and a built-in audit
trail. This guide outlines the main performance characteristics, trade-offs,
and optimization strategies to validate against your own workload.

## 🚀 Performance Characteristics

### Lithair vs Traditional Databases

| Aspect           | Lithair                  | Traditional SQL              | Advantage                                   |
| ---------------- | ------------------------ | ---------------------------- | ------------------------------------------- |
| **Architecture** | Single binary by default | Multi-tier stack             | Simpler default deployment                  |
| **Latency**      | In-memory + append-only  | Network + B-tree             | Often lower latency on read-heavy workloads |
| **Deployment**   | Fewer moving parts       | External services are common | Faster local setup                          |
| **Audit Trail**  | Native event sourcing    | External logging             | **Built-in**                                |
| **Consistency**  | Event-sourced state flow | Transaction-oriented model   | Different trade-offs depending on workload  |

## 🎯 Performance Optimization Strategies

### 1. Lifecycle-Aware Storage

Lithair automatically optimizes storage based on declared field lifecycles:

```rust
#[derive(LifecycleAware)]
struct Product {
    #[lifecycle(immutable)]
    id: u64,                    // Stored once, never updated

    #[lifecycle(versioned = 5)]
    name: String,               // Keep 5 versions max

    #[lifecycle(snapshot_only)]
    computed_score: f64,        // Only in snapshots, not events
}
```

### 2. Declarative Performance Tuning

Configure performance characteristics declaratively:

```rust
#[lithair_config]
struct AppConfig {
    snapshot_frequency: SnapshotPolicy::Every(1000),
    persistence_mode: PersistenceMode::Async,
    memory_mode: MemoryMode::Adaptive,
}
```

### 3. Adaptive Memory Management

Lithair automatically adapts memory usage based on load:

```rust
pub enum MemoryMode {
    Eager,      // Keep everything in memory
    Hybrid,     // Smart caching based on access patterns
    Lazy,       // Minimal memory footprint
}
```

**Benefits:**

- **Eager**: Ultra-fast queries, higher memory usage
- **Hybrid**: Balanced performance and memory efficiency
- **Lazy**: Minimal memory, suitable for resource-constrained environments

### 4. Built-in Performance Monitoring

Monitor performance in real-time:

```rust
// Performance metrics automatically available
GET /api/stats
{
    "events_per_second": 85000,
    "memory_usage_mb": 245,
    "persistence_lag_ms": 12,
    "query_latency_avg_ms": 3.2
}
```

## 🎯 Best Practices

### Production Configuration

```rust
#[lithair_config]
struct ProductionConfig {
    // Optimize for your workload
    snapshot_frequency: SnapshotPolicy::Every(1000),
    persistence_mode: PersistenceMode::Async,
    memory_mode: MemoryMode::Adaptive,

    // Security built-in
    rbac_enabled: true,
    audit_trail: AuditLevel::Full,
}
```

### Monitoring and Observability

Lithair provides built-in metrics and health checks:

```bash
# Health check
GET /health
{
    "status": "healthy",
    "uptime_seconds": 3600,
    "events_processed": 1000000
}

# Performance metrics
GET /metrics
{
    "throughput_eps": 85000,
    "latency_p99_ms": 5.2,
    "memory_usage_mb": 245
}
```

## 🚀 Why Lithair Outperforms Traditional Databases

### 1. **No Network Overhead**

- Traditional: Application ↔ Network ↔ Database
- Lithair: Application **IS** the database

### 2. **Append-Only Storage**

- Traditional: Complex B-tree updates with locks
- Lithair: Simple append operations, no locks needed

### 3. **Event Sourcing by Design**

- Traditional: Manual audit trail implementation
- Lithair: Complete history automatically preserved

### 4. **Lifecycle-Aware Optimization**

- Traditional: Generic storage for all data
- Lithair: Storage optimized per field lifecycle

These advantages are strongest when the workload fits in memory and benefits
from event-sourced projections. They are not a universal replacement for every
database shape or deployment model.

## 📚 Learn More

- [System Overview](SYSTEM_OVERVIEW.md) - Core Lithair philosophy
- [Developer Guide](DEVELOPER_GUIDE.md) - Building Lithair applications
- [API Reference](API_REFERENCE.md) - Complete API documentation

---

_Lithair: simplifying application architecture for event-sourced, memory-first workloads._
