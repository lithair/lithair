# SQL vs Lithair: A Practical Guide

This guide is for developers coming from traditional SQL (PostgreSQL, MySQL,
SQLite). It explains how to think about Lithair as your primary database
engine, what you gain, what changes, and how to operate it daily.

## High‑level comparison

- **Data model**
  - Traditional RDBMS: tables, rows, SQL schema
  - Lithair engine: Rust structs in `State`, maps/sets, explicit invariants
- **Queries**
  - Traditional RDBMS: SQL planner, joins, ad-hoc query surface
  - Lithair engine: direct in-memory reads, precomputed indexes/projections
- **Transactions**
  - Traditional RDBMS: ACID, multi-row transactions
  - Lithair engine: single-event apply with aggregate-level invariant checks
- **Idempotence**
  - Traditional RDBMS: usually an application-level pattern
  - Lithair engine: built in via `event_id` + payload hash, persisted in
    `dedup.raftids`
- **Persistence**
  - Traditional RDBMS: page files and WAL
  - Lithair engine: append log (JSON envelope or binary) plus snapshots
- **Recovery**
  - Traditional RDBMS: crash recovery and redo
  - Lithair engine: snapshot restore plus replay (registry optional)
- **Compaction**
  - Traditional RDBMS: vacuum and maintenance operations
  - Lithair engine: truncate after snapshot, optional rotation, exactly-once
    preserved
- **Latency expectations**
  - Traditional RDBMS reads: often milliseconds because of I/O and networking
  - Lithair reads: often much lower for direct in-memory access paths
  - Traditional RDBMS writes: often milliseconds with fsync/page work
  - Lithair writes: often lower on append-only, memory-first paths
- **Human audit**
  - Traditional RDBMS: SQL-level inspection tools
  - Lithair engine: JSON log readable, binary path optimized and less human
    readable
- **Migrations**
  - Traditional RDBMS: DDL and schema migration flows
  - Lithair engine: event evolution; `apply` and `State` evolve without DDL

## Mental model: mapping SQL → Lithair

- Table → `HashMap<Id, Row>` in your `State`
- Row → Rust struct instance
- Primary key → map key (or unique field enforced in `apply`)
- Foreign key → index + guard in `apply` (reject events that violate FK)
- Unique index → `HashSet` (e.g., emails) checked in `apply`
- Join → precomputed indexes (O(1) lookups), updated in `apply`
- Transaction → a single event that enforces invariants atomically
- Migration → new event variants; extend `apply` to support old+new

## Enforcing constraints (PK, FK, uniqueness)

A minimal pattern you can reuse:

```rust
#[derive(Default)]
struct State {
    customers: HashMap<u64, Customer>,
    products: HashMap<u64, Product>,
    orders: HashMap<u64, Order>,
    email_index: HashSet<String>,
    orders_by_customer: HashMap<u64, HashSet<u64>>,
}

enum Event {
    CustomerCreated { id: u64, email: String },
    CustomerDeleted { id: u64 },
    ProductCreated { id: u64, name: String },
    OrderCreated { id: u64, customer_id: u64, product_ids: Vec<u64> },
}

impl lithair_core::engine::Event for Event {
    type State = State;
    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap()
    }

    fn apply(&self, s: &mut State) {
        match self {
            Event::CustomerCreated { id, email } => {
                if s.customers.contains_key(id) || s.email_index.contains(email) {
                    return;
                }
                s.customers.insert(
                    *id,
                    Customer {
                        id: *id,
                        email: email.clone(),
                    },
                );
                s.email_index.insert(email.clone());
            }
            Event::CustomerDeleted { id } => {
                if let Some(set) = s.orders_by_customer.get(id) {
                    if !set.is_empty() {
                        return;
                    }
                }
                if let Some(c) = s.customers.remove(id) {
                    s.email_index.remove(&c.email);
                }
            }
            Event::ProductCreated { id, name } => {
                if s.products.contains_key(id) {
                    return;
                }
                s.products.insert(
                    *id,
                    Product {
                        id: *id,
                        name: name.clone(),
                    },
                );
            }
            Event::OrderCreated { id, customer_id, product_ids } => {
                if s.orders.contains_key(id) {
                    return;
                }
                if !s.customers.contains_key(customer_id) {
                    return;
                }
                if !product_ids.iter().all(|pid| s.products.contains_key(pid)) {
                    return;
                }
                s.orders.insert(
                    *id,
                    Order {
                        id: *id,
                        customer_id: *customer_id,
                        product_ids: product_ids.clone(),
                    },
                );
                s.orders_by_customer
                    .entry(*customer_id)
                    .or_default()
                    .insert(*id);
            }
        }
    }
}
```

## Exactly‑once and idempotence

- Add a stable `idempotence_key()` to command‑driven events (e.g., client command id, business key):

```rust
impl Event for PaymentCaptured {
    fn idempotence_key(&self) -> Option<String> {
        Some(format!("payment:{}", self.payment_id))
    }
    // to_json/apply omitted
}
```

- The engine persists `{ event_type, event_id, timestamp, payload }` and
  appends `event_id` to `dedup.raftids`.
- On restart and even after compaction/rotation, duplicates are rejected
  (dedup set rebuilt from envelopes and `dedup.raftids`).

## Snapshots, compaction, rotation

- Snapshots: implement `serialize_state`/`deserialize_state` for full-state
  snapshots (fast restart). Use `engine.save_state_snapshot()` on demand.
- Compaction: after a snapshot, call `engine.compact_after_snapshot()` to
  truncate the log; exactly-once is preserved via `dedup.raftids`.
- Rotation: set `EngineConfig.max_log_file_size`; replay reads rotated log
  segments automatically.

## Binary (optimized) persistence

- Use `persistence_optimized::OptimizedFileStorage` with
  `OptimizedPersistenceConfig` for async buffered writes and `bincode`.
- Typical mode: append binary for speed, keep JSON snapshots for recovery; the
  read path is unchanged because queries hit in-memory state.
- For current validation, prefer the engine and durability test suites that
  exist today in `lithair-core/tests/` and `cucumber-tests/tests/`.

## SQL → Lithair query cookbook

Practical equivalents for common SQL:

- **Lookup by id**

```rust
let customer = state.customers.get(&42);
```

- **Orders for a customer**

```rust
let order_ids = state.orders_by_customer.get(&42).cloned().unwrap_or_default();
let orders: Vec<&Order> = order_ids
    .iter()
    .filter_map(|id| state.orders.get(id))
    .collect();
```

- **Count all orders**

```rust
let total = state.orders.len();
```

- **Count customer orders**

```rust
let n = state.orders_by_customer.get(&42).map(|s| s.len()).unwrap_or(0);
```

- **Simple name contains search**

```rust
let matches: Vec<&Product> = state.products
    .values()
    .filter(|p| p.name.to_lowercase().contains("case"))
    .collect();
// Tip: maintain a simple inverted index if this becomes hot
```

- **Join through maintained indexes**

```rust
// If you maintain email_index: HashMap<String, u64> (email -> customer_id)
if let Some(&cust_id) = state.email_to_customer_id.get("a@b.com") {
    if let Some(order_ids) = state.orders_by_customer.get(&cust_id) {
        let orders: Vec<&Order> = order_ids
            .iter()
            .filter_map(|id| state.orders.get(id))
            .collect();
        // use orders
    }
}
```

- **Aggregates from maintained projections**

```rust
// Example: user_analytics[user_id] updated in each order event
let a = state.user_analytics.get(&42).unwrap();
let total_spent = a.total_spent; // precomputed
let avg_order = a.avg_order_value; // precomputed
```

- **Recent items**

```rust
// Maintain a recent_orders VecDeque<OrderId> in State inside apply
let latest: Vec<&Order> = state.recent_orders
    .iter()
    .take(10)
    .filter_map(|id| state.orders.get(id))
    .collect();
```

- **Pagination**

```rust
let mut list: Vec<&Customer> = state.customers.values().collect();
list.sort_by_key(|c| c.id);
let page = &list[page_start..page_end.min(list.len())];
```

More recipes:

- **Upsert pattern**

```rust
// Pattern 1: explicit event variants
match state.products.get(&id) {
    None => events.apply(ProductCreated { id, name: new_name.clone() }),
    Some(_) => events.apply(ProductRenamed { id, name: new_name.clone() }),
}
// Pattern 2: single event with logic in apply
// (if not exists -> create else update)
```

- **Batch-like category update**

```rust
for p in state.products.values_mut() {
    if p.category == "Electronics" { p.price *= 0.9; }
}
// Prefer an event ProductDiscountApplied { category, factor }
```

- **Guarded delete**

```rust
// Will be ignored if orders_by_customer[42] is not empty
let _ = engine.apply_event(CustomerDeleted { id: 42 });
```

- **Group by status**

```rust
let mut by_status: HashMap<Status, usize> = HashMap::new();
for o in state.orders.values() { *by_status.entry(o.status).or_default() += 1; }
```

- **Time-range sum**

```rust
let sum: f64 = state.orders
    .values()
    .filter(|o| o.created_at >= t1 && o.created_at <= t2)
    .map(|o| o.total)
    .sum();
// Tip: keep time-bucketed projections in apply for O(1)
```

- **Filter by id set**

```rust
let ids = [1,2,3];
let rows: Vec<&Customer> = ids
    .iter()
    .filter_map(|id| state.customers.get(id))
    .collect();
```

- **Existence check**

```rust
let has_orders = state
    .orders_by_customer
    .get(&cust_id)
    .map(|s| !s.is_empty())
    .unwrap_or(false);
```

- **Distinct values**

```rust
let unique_emails: HashSet<&str> = state
    .customers
    .values()
    .map(|c| c.email.as_str())
    .collect();
```

- **Optional related record**

```rust
let with_last_order: Vec<(&Customer, Option<&Order>)> = state
    .customers
    .values()
    .map(|c| {
        let last = state.orders_by_customer
            .get(&c.id)
            .and_then(|ids| ids.iter().max())
            .and_then(|id| state.orders.get(id));
        (c, last)
    })
    .collect();
```

- **Top N by revenue**

```rust
let mut v: Vec<&Customer> = state.customers.values().collect();
v.sort_by(|a,b| b.revenue.partial_cmp(&a.revenue).unwrap());
let top_n = &v[..n.min(v.len())];
```

- **Full-text style lookup**

```rust
// naive contains as shown above
// for speed keep a word->product_id index built in apply
```

- **Optimistic concurrency**

```rust
// include expected_version in event
// in apply reject if current_version != expected_version
```

## Daily operations

- Configure `EngineConfig`: `event_log_path`, `snapshot_every`,
  `fsync_on_append`, `log_verbose`, `max_log_file_size`.
- Safety snapshot: `engine.save_state_snapshot()`; compaction:
  `engine.compact_after_snapshot()`.
- Useful current validation points:
  - `lithair-core/tests/benchmark_tests.rs`
  - `cucumber-tests/tests/durability_test.rs`
  - `cucumber-tests/tests/multi_file_durability_test.rs`
  - `cucumber-tests/tests/snapshot_durability_test.rs`

## When to keep SQL

- Heavy ad-hoc analytics over huge historical datasets (Lithair favors
  pre-computed projections)
- Cross-application shared database (Lithair is embedded and app-scoped)
- Hybrid: export events to a warehouse and keep OLTP in Lithair when that
  memory-first trade-off fits

## Adoption checklist

- Model entities and relations in `State` + indexes
- Define events and guard invariants in `apply`
- Add `idempotence_key()` to command‑driven events
- Provide snapshot serializers for fast restarts
- Enable rotation, schedule compaction
- Cover invariants with tests (uniqueness, FK, concurrency, recovery)
