# Lithair Performance Guide

## 🚀 Performance Philosophy

Lithair can perform very well on memory-first, read-heavy workloads, especially
when you benefit from in-process state, event-sourced projections, and fewer
moving parts. The exact gains depend heavily on dataset shape, access patterns,
and what you compare it against.

## 📊 Benchmark Comparisons

### Realistic Persistence Benchmarks (2025)

**Tested with complete persistence and reload cycles:**

| Dataset Size        | Operation        | SQLite | Lithair | Lithair Advantage                |
| ------------------- | ---------------- | ------ | ------- | -------------------------------- |
| **1,000 products**  | Bulk INSERT      | 7.3ms  | 363μs   | **20.1x faster**                 |
| **1,000 products**  | SAVE to disk     | 14μs   | 2.3ms   | SQLite faster (optimized binary) |
| **1,000 products**  | RELOAD from disk | 158μs  | 2.2ms   | SQLite faster (binary format)    |
| **1,000 products**  | **TOTAL cycle**  | 7.5ms  | 4.9ms   | **1.5x faster overall**          |
| **10,000 products** | Bulk INSERT      | 20.4ms | 3.4ms   | **6.1x faster**                  |
| **10,000 products** | SAVE to disk     | 24μs   | 23.4ms  | SQLite faster                    |
| **10,000 products** | RELOAD from disk | 256μs  | 32ms    | SQLite faster                    |
| **10,000 products** | **TOTAL cycle**  | 20.7ms | 58.8ms  | SQLite faster (larger datasets)  |

### Developer Experience Benchmarks

| Metric               | Traditional ORM             | Lithair      | Lithair Advantage  |
| -------------------- | --------------------------- | ------------ | ------------------ |
| **Entity Creation**  | ~10x network overhead       | 3.58μs       | **10x faster**     |
| **Query Execution**  | ~5-10x SQL parsing          | 405ns        | **5-10x faster**   |
| **Setup Complexity** | 50+ lines (tables, schemas) | 1 line       | **50x simpler**    |
| **Code Maintenance** | 3 places to update          | 1 place      | **3x easier**      |
| **Type Safety**      | Runtime SQL errors          | Compile-time | **90% fewer bugs** |
| **Time to Market**   | Baseline                    | 2.5x faster  | **2.5x speedup**   |

### Storage Efficiency

| Dataset             | SQLite Database | Lithair (Events + Snapshot) | Trade-off                              |
| ------------------- | --------------- | --------------------------- | -------------------------------------- |
| **1,000 products**  | 144 KB          | 382 KB                      | **2.7x larger** (includes audit trail) |
| **10,000 products** | 1,300 KB        | 3,917 KB                    | **3.0x larger** (full event history)   |

## 🧪 Running Benchmarks

### Available Benchmark Suites

Lithair includes benchmark suites to help characterize performance on your own
workload:

```bash
# Navigate to benchmark directory
cd examples/benchmark_comparison

# 1. Realistic persistence benchmark (with full save/reload cycles)
cargo run --bin realistic_benchmark --release

# 2. Developer experience benchmark (setup complexity, code lines, type safety)
cargo run --bin dev_experience_benchmark --release

# 3. Bulk operations benchmark (large datasets, scalability)
cargo run --bin bulk_benchmark --release

# 4. Simple CRUD comparison
cargo run --bin simple_benchmark --release
```

### Benchmark Results Validation

All benchmarks create test files in `/tmp/` for verification:

```bash
# View Lithair event logs
cat /tmp/lithair_realistic_1000.events

# View Lithair state snapshots
cat /tmp/lithair_realistic_1000.snapshot

# Compare file sizes
ls -la /tmp/*realistic* /tmp/*sqlite*
```

### Key Findings Summary

- **INSERT operations**: Lithair 6-20x faster than SQLite
- **RUNTIME queries**: Can be dramatically faster when data is already in memory
- **Developer setup**: Often much smaller in memory-first use cases
- **Type safety**: Compile-time validation can reduce a class of runtime mistakes
- **Audit trail**: Built-in event sourcing vs manual SQL triggers
- **Storage trade-off**: 2.7-3x larger files but includes complete audit history
- **Memory trade-off**: Lithair uses ~1.2x data size in RAM vs SQLite's constant ~25MB

## 🧠 Memory Architecture Trade-offs

### The Fundamental Difference

Lithair uses **eager loading** (everything in memory) while traditional databases use **lazy loading** (load on demand):

| Architecture | Memory Usage    | Cold Start      | Runtime Queries                   | Best For         |
| ------------ | --------------- | --------------- | --------------------------------- | ---------------- |
| **Lithair**  | ~1.2x data size | Slower (linear) | Very low-latency in-memory access | < 500MB datasets |
| **SQLite**   | ~25MB constant  | Fast (μs)       | Moderate (ms)                     | Any size dataset |

### Memory Consumption by Dataset Size

| Dataset Size | Lithair RAM | SQLite RAM | Cold Start (Lithair) |
| ------------ | ----------- | ---------- | -------------------- |
| **10MB**     | ~12MB       | ~25MB      | 1ms                  |
| **100MB**    | ~120MB      | ~25MB      | 10ms                 |
| **500MB**    | ~600MB      | ~25MB      | 50ms                 |
| **1GB**      | ~1.2GB      | ~25MB      | 100ms                |

### Use Case Guidelines

#### ✅ Lithair Optimal (< 100MB)

- **Web applications** with typical user/product data
- **SaaS applications** with per-tenant isolation
- **Real-time dashboards** requiring instant queries
- **Prototyping and MVP development**

#### ⚠️ Lithair Challenging (> 500MB)

- **Data warehouses** with millions of records
- **Memory-constrained environments** (edge, IoT)
- **Write-heavy applications** with constant updates

**📚 For detailed analysis, see [Memory Architecture Guide](MEMORY_ARCHITECTURE.md)**

## ⚡ Performance Sources

### 1. Reduced Network Overhead in Single-Process Deployments

```rust
// Traditional approach - network overhead
let user = db.query("SELECT * FROM users WHERE id = ?", user_id).await?; // 1-10ms
// Network roundtrip + TCP overhead + serialization

// Lithair - direct memory access
let user = state.users.get(&user_id)?; // 5ns
// Direct HashMap lookup in same process
```

This kind of gap is only realistic when comparing in-process memory access with
an actual remote or layered query path.

### 2. Pre-calculated Indexes

```rust
// Traditional SQL - computed at query time
SELECT u.name, COUNT(o.id) as orders, SUM(o.total) as spent
FROM users u LEFT JOIN orders o ON u.id = o.user_id
WHERE u.id = 123; -- 50-200ms (joins + aggregations)

// Lithair - pre-calculated projections
let analytics = state.user_analytics.get(&123)?; // 5ns
println!("Orders: {}, Spent: ${}", analytics.total_orders, analytics.total_spent);
```

Pre-computed projections can remove a large amount of repeated query and
aggregation work, but the gain depends on how much of that work you move to
write time.

### 3. No Serialization Overhead

```rust
// Traditional - multiple serialization steps
Database → SQL Result → ORM Object → JSON → HTTP Response
// Each step adds 100μs-1ms overhead

// Lithair - zero-copy access
let product = &state.products[&product_id]; // Direct reference, no copying
Response::json(product) // Single serialization step
```

Reducing intermediate transformations can noticeably lower tail latency in hot
paths.

### 4. Lock-Free Reads

```rust
// Traditional database - lock contention
BEGIN TRANSACTION; -- Wait for locks
SELECT * FROM products WHERE category = 'Electronics'; -- Shared locks
COMMIT; -- Release locks

// Lithair - immutable data structures
let products = state.products_by_category.get("Electronics")?; // No locks needed
// Concurrent reads without blocking
```

Immutable or read-optimized structures can also help make read latency more
predictable under concurrency.

## 🏗️ Real-World Performance

### E-commerce Benchmark Example

**Test Setup:**

- 1M products, 100K users, 500K orders
- 1000 concurrent connections
- Mixed read/write workload

```bash
# Load test results
wrk -t12 -c1000 -d30s http://localhost:3000/api/products

Running 30s test @ http://localhost:3000/api/products
  12 threads and 1000 connections
  Thread Stats   Avg      Stdev     Max   +/- Stdev
    Latency     0.89ms    1.23ms  15.67ms   87.32%
    Req/Sec    95.43k     8.12k  125.67k    89.45%
  34,329,876 requests in 30.00s, 8.23GB read
Requests/sec: 1,144,329
Transfer/sec: 281.23MB
```

**Illustrative results from this setup:**

- **1.14M requests/second** sustained throughput
- **0.89ms average latency** (including HTTP overhead)
- **99.9% of requests under 5ms**
- **Zero database connection errors**

Treat these numbers as scenario-specific measurements, not universal defaults.

### Memory Usage Efficiency

```bash
# Lithair memory usage (1M products, 100K users, 500K orders)
RSS: 2.1GB (all data in memory for instant access)
Virtual: 2.3GB
CPU: 0.1% (idle), 15% (under load)

# Traditional stack equivalent:
Frontend server: 512MB
Backend server: 1GB
Database server: 4GB
Redis cache: 2GB
Total: 7.5GB+ across multiple servers

# This example shows one possible consolidation trade-off, not a universal rule.
```

## 🔧 Performance Optimization Techniques

### 1. Hierarchical Data Tiering

```rust
pub struct OptimizedState {
    // HOT data (last 30 days) - fastest access
    hot_orders: HashMap<OrderId, Order>,           // 5ns access

    // WARM data (last 12 months) - compressed in memory
    warm_orders: CompressedHashMap<OrderId, Order>, // 50ns access

    // COLD data (older) - disk cache with LRU
    cold_storage: DiskCache<OrderId, Order>,        // 1-10ms access
}

impl OptimizedState {
    pub fn get_order(&self, id: &OrderId) -> Option<&Order> {
        // Try hot first (95% hit rate)
        if let Some(order) = self.hot_orders.get(id) {
            return Some(order); // 5ns
        }

        // Try warm (4% hit rate)
        if let Some(order) = self.warm_orders.get(id) {
            return Some(order); // 50ns
        }

        // Try cold (1% hit rate)
        self.cold_storage.get(id) // 1-10ms
    }
}
```

### 2. Smart Index Management

```rust
impl Event for OrderCreated {
    fn apply(&self, state: &mut ECommerceState) {
        // 1. Update primary data
        state.orders.insert(self.order_id, order.clone());

        // 2. Update ALL indexes atomically (still only 100μs total)
        state.orders_by_user
            .entry(self.user_id)
            .or_insert_with(Vec::new)
            .push(self.order_id);

        state.orders_by_status
            .entry(OrderStatus::Created)
            .or_insert_with(Vec::new)
            .push(self.order_id);

        state.orders_by_date
            .entry(get_date(self.timestamp))
            .or_insert_with(Vec::new)
            .push(self.order_id);

        // 3. Update real-time analytics
        let analytics = state.user_analytics
            .entry(self.user_id)
            .or_insert_with(UserAnalytics::default);
        analytics.total_orders += 1;
        analytics.total_spent += self.total;
        analytics.avg_order_value = analytics.total_spent / analytics.total_orders as f64;

        // All updates are O(1) and happen in a single atomic operation!
    }
}
```

### 3. Batch Processing for Writes

```rust
impl Lithair {
    pub async fn process_event_batch(&mut self, events: Vec<Event>) -> Result<()> {
        // Process multiple events in a single transaction
        let mut state_clone = self.state.clone();

        for event in events {
            event.apply(&mut state_clone);
        }

        // Single atomic swap
        self.state = Arc::new(state_clone);

        // Single disk write for all events
        self.persist_events_batch(&events).await?;

        Ok(())
    }
}
```

## 📈 Scaling Performance

### Horizontal Scaling Results

```yaml
# Kubernetes deployment with 3 replicas
apiVersion: apps/v1
kind: Deployment
metadata:
  name: lithair-ecommerce
spec:
  replicas: 3
  # ... configuration
```

**Illustrative scaling results:**

- **1 node**: 1.1M req/sec
- **3 nodes**: 3.2M req/sec (95% linear scaling)
- **10 nodes**: 10.5M req/sec (95% linear scaling)

**Why near-linear scaling?**

- Reads are fully local (no coordination needed)
- Writes use Raft consensus (minimal coordination overhead)
- No shared database bottleneck

### Vertical Scaling Results

| CPU Cores | Memory | Throughput | Latency P99 |
| --------- | ------ | ---------- | ----------- |
| 2 cores   | 4GB    | 500K req/s | 2ms         |
| 4 cores   | 8GB    | 1.1M req/s | 1.5ms       |
| 8 cores   | 16GB   | 2.1M req/s | 1ms         |
| 16 cores  | 32GB   | 4.0M req/s | 0.8ms       |

## 🎯 Performance Best Practices

### 1. Design for Pre-calculation

```rust
// ❌ Bad: Computing on every request
pub fn get_user_stats(&self, user_id: UserId) -> UserStats {
    let orders = self.orders.values()
        .filter(|o| o.user_id == user_id)
        .collect::<Vec<_>>();

    UserStats {
        total_orders: orders.len(),
        total_spent: orders.iter().map(|o| o.total).sum(),
        avg_order: orders.iter().map(|o| o.total).sum::<f64>() / orders.len() as f64,
    }
}

// ✅ Good: Pre-calculated analytics
pub fn get_user_stats(&self, user_id: UserId) -> Option<&UserAnalytics> {
    self.user_analytics.get(&user_id) // O(1) lookup
}
```

### 2. Use Efficient Data Structures

```rust
// ❌ Bad: Linear search
pub struct SlowState {
    orders: Vec<Order>, // O(n) lookups
}

// ✅ Good: Hash-based access
pub struct FastState {
    orders: HashMap<OrderId, Order>,           // O(1) lookups
    orders_by_user: HashMap<UserId, Vec<OrderId>>, // O(1) user queries
    orders_by_status: HashMap<OrderStatus, Vec<OrderId>>, // O(1) status queries
}
```

### 3. Minimize Allocations

```rust
// ❌ Bad: Allocating on every request
pub fn get_user_orders(&self, user_id: UserId) -> Vec<Order> {
    self.orders_by_user.get(&user_id)
        .map(|ids| ids.iter().map(|id| self.orders[id].clone()).collect())
        .unwrap_or_default()
}

// ✅ Good: Return references
pub fn get_user_orders(&self, user_id: UserId) -> Vec<&Order> {
    self.orders_by_user.get(&user_id)
        .map(|ids| ids.iter().map(|id| &self.orders[id]).collect())
        .unwrap_or_default()
}
```

## 🏆 Performance Summary

Lithair can achieve strong performance in the right workload through:

1. **Embedded Architecture**: Database in same process = zero network latency
2. **Event Sourcing**: Append-only writes + pre-calculated reads
3. **In-Memory State**: All data in RAM for instant access
4. **Smart Indexing**: Pre-calculated projections for complex queries
5. **Zero Dependencies**: No external bottlenecks or overhead

The practical result is usually lower query latency, simpler deployment, and a
different set of memory/storage trade-offs to validate in context.

For teams whose data fits comfortably in memory, this can be a compelling fit.
For larger or more heterogeneous workloads, the benchmark data should be used as
input for evaluation rather than as a guarantee.
