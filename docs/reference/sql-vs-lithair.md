# SQL vs Lithair: A Practical Guide

This guide is for developers coming from traditional SQL (PostgreSQL, MySQL, SQLite). It explains how to think about Lithair as your primary database engine, what you gain, what changes, and how to operate it daily.

## High‑level comparison

| Topic            | Traditional RDBMS        | Lithair Engine                                                     |
| ---------------- | ------------------------ | -------------------------------------------------------------------- |
| Data model       | Tables, rows, SQL schema | Rust structs in `State`, HashMaps/sets, explicit invariants          |
| Queries          | SQL (planner, joins)     | Direct in‑memory reads, precomputed indexes/projections              |
| Transactions     | ACID, multi‑row          | Single‑event apply with invariant checks (aggregate‑level)           |
| Idempotence      | App‑level pattern        | Built‑in via `event_id` + payload hash, persisted in `dedup.raftids` |
| Persistence      | Page files, WAL          | Append log (JSON envelope or binary), snapshots                      |
| Recovery         | Crash recovery, redo     | Snapshot restore + replay (registry optional)                        |
| Compaction       | Vacuum/maintenance       | Truncate after snapshot; size‑based rotation; exactly‑once preserved |
| Latency (reads)  | ms (I/O + network)       | ns/μs (in‑memory)                                                    |
| Latency (writes) | ms (fsync/page)          | ~100μs; configurable fsync; binary path faster                       |
| Human audit      | SQL readable             | JSON log readable; binary path optimized (non‑human)                 |
| Migrations       | DDL                      | Event evolution; `apply` and `State` evolve without DDL              |

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
    fn to_json(&self) -> String { serde_json::to_string(self).unwrap() }
    fn apply(&self, s: &mut State) {
        match self {
            Event::CustomerCreated { id, email } => {
                if s.customers.contains_key(id) || s.email_index.contains(email) { return; }
                s.customers.insert(*id, Customer { id: *id, email: email.clone() });
                s.email_index.insert(email.clone());
            }
            Event::CustomerDeleted { id } => {
                if let Some(set) = s.orders_by_customer.get(id) { if !set.is_empty() { return; } }
                if let Some(c) = s.customers.remove(id) { s.email_index.remove(&c.email); }
            }
            Event::ProductCreated { id, name } => {
                if s.products.contains_key(id) { return; }
                s.products.insert(*id, Product { id: *id, name: name.clone() });
            }
            Event::OrderCreated { id, customer_id, product_ids } => {
                if s.orders.contains_key(id) { return; }
                if !s.customers.contains_key(customer_id) { return; }
                if !product_ids.iter().all(|pid| s.products.contains_key(pid)) { return; }
                s.orders.insert(*id, Order { id: *id, customer_id: *customer_id, product_ids: product_ids.clone() });
                s.orders_by_customer.entry(*customer_id).or_default().insert(*id);
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

- The engine persists `{ event_type, event_id, timestamp, payload }` and appends `event_id` to `dedup.raftids`.
- On restart and even after compaction/rotation, duplicates are rejected (dedup set rebuilt from envelopes and `dedup.raftids`).

## Snapshots, compaction, rotation

- Snapshots: implement `serialize_state`/`deserialize_state` for full‑state snapshots (fast restart). Use `engine.save_state_snapshot()` on demand.
- Compaction: after a snapshot, call `engine.compact_after_snapshot()` to truncate the log; exactly‑once is preserved via `dedup.raftids`.
- Rotation: set `EngineConfig.max_log_file_size`; replay reads `events.raftlog.1` then `events.raftlog` automatically.

## Binary (optimized) persistence

- Use `persistence_optimized::OptimizedFileStorage` with `OptimizedPersistenceConfig` for async buffered writes and `bincode`.
- Typical mode: append binary for speed, keep JSON snapshots for recovery; read path is unchanged (always in‑memory state).
- Reference tests: `binary_persistence.rs`, `binary_e2e.rs`.

## SQL → Lithair query cookbook

Practical equivalents for common SQL:

- SELECT \* FROM customers WHERE id = 42

```rust
let customer = state.customers.get(&42);
```

- SELECT \* FROM orders WHERE customer_id = 42

```rust
let order_ids = state.orders_by_customer.get(&42).cloned().unwrap_or_default();
let orders: Vec<&Order> = order_ids.iter().filter_map(|id| state.orders.get(id)).collect();
```

- SELECT COUNT(\*) FROM orders

```rust
let total = state.orders.len();
```

- SELECT COUNT(\*) FROM orders WHERE customer_id = 42

```rust
let n = state.orders_by_customer.get(&42).map(|s| s.len()).unwrap_or(0);
```

- SELECT \* FROM products WHERE name LIKE '%case%'

```rust
let matches: Vec<&Product> = state.products.values()
    .filter(|p| p.name.to_lowercase().contains("case"))
    .collect();
// Tip: maintain a simple inverted index if this becomes hot
```

- SELECT \* FROM orders o JOIN customers c ON o.customer_id = c.id WHERE c.email = 'a@b.com'

```rust
// If you maintain email_index: HashMap<String, u64> (email -> customer_id)
if let Some(&cust_id) = state.email_to_customer_id.get("a@b.com") {
    if let Some(order_ids) = state.orders_by_customer.get(&cust_id) {
        let orders: Vec<&Order> = order_ids.iter().filter_map(|id| state.orders.get(id)).collect();
        // use orders
    }
}
```

- Aggregates: SUM/AVG over projections (kept in apply)

```rust
// Example: user_analytics[user_id] updated in each order event
let a = state.user_analytics.get(&42).unwrap();
let total_spent = a.total_spent; // precomputed
let avg_order = a.avg_order_value; // precomputed
```

- ORDER BY created_at DESC LIMIT 10

```rust
// Maintain a recent_orders VecDeque<OrderId> in State inside apply
let latest: Vec<&Order> = state.recent_orders.iter()
    .take(10)
    .filter_map(|id| state.orders.get(id))
    .collect();
```

- Pagination

```rust
let mut list: Vec<&Customer> = state.customers.values().collect();
list.sort_by_key(|c| c.id);
let page = &list[page_start..page_end.min(list.len())];
```

More recipes:

- UPSERT (insert or update)

```rust
// Pattern 1: explicit event variants
match state.products.get(&id) {
    None => events.apply(ProductCreated { id, name: new_name.clone() }),
    Some(_) => events.apply(ProductRenamed { id, name: new_name.clone() }),
}
// Pattern 2: single event with logic in apply (if not exists -> create else update)
```

- UPDATE products SET price = price \* 0.9 WHERE category = 'Electronics'

```rust
for p in state.products.values_mut() {
    if p.category == "Electronics" { p.price *= 0.9; }
}
// Prefer an event ProductDiscountApplied { category, factor }
```

- DELETE FROM customers WHERE id = 42 (guarded by FK)

```rust
// Will be ignored if orders_by_customer[42] is not empty
let _ = engine.apply_event(CustomerDeleted { id: 42 });
```

- SELECT status, COUNT(\*) FROM orders GROUP BY status

```rust
let mut by_status: HashMap<Status, usize> = HashMap::new();
for o in state.orders.values() { *by_status.entry(o.status).or_default() += 1; }
```

- SUM(total) FROM orders WHERE created_at BETWEEN t1 AND t2

```rust
let sum: f64 = state.orders.values()
    .filter(|o| o.created_at >= t1 && o.created_at <= t2)
    .map(|o| o.total)
    .sum();
// Tip: keep time-bucketed projections in apply for O(1)
```

- WHERE id IN (1,2,3)

```rust
let ids = [1,2,3];
let rows: Vec<&Customer> = ids.iter().filter_map(|id| state.customers.get(id)).collect();
```

- EXISTS (SELECT 1 FROM orders WHERE customer_id = ?)

```rust
let has_orders = state.orders_by_customer.get(&cust_id).map(|s| !s.is_empty()).unwrap_or(false);
```

- DISTINCT emails FROM customers

```rust
let unique_emails: HashSet<&str> = state.customers.values().map(|c| c.email.as_str()).collect();
```

- LEFT JOIN (customers with optional last order)

```rust
let with_last_order: Vec<(&Customer, Option<&Order>)> = state.customers.values()
    .map(|c| {
        let last = state.orders_by_customer.get(&c.id)
            .and_then(|ids| ids.iter().max())
            .and_then(|id| state.orders.get(id));
        (c, last)
    })
    .collect();
```

- Window‑like: TOP N by revenue

```rust
let mut v: Vec<&Customer> = state.customers.values().collect();
v.sort_by(|a,b| b.revenue.partial_cmp(&a.revenue).unwrap());
let top_n = &v[..n.min(v.len())];
```

- Full‑text (simple contains vs small index)

```rust
// naive contains as shown above; for speed keep a word->product_id index built in apply
```

- Optimistic concurrency (expected version)

```rust
// include expected_version in event; in apply reject if current_version != expected_version
```

## Daily operations

- Configure `EngineConfig`: `event_log_path`, `snapshot_every`, `fsync_on_append`, `log_verbose`, `max_log_file_size`.
- Safety snapshot: `engine.save_state_snapshot()`; compaction: `engine.compact_after_snapshot()`.
- Tests utiles:
  - `cargo test -p benchmark_comparison -- --nocapture | cat`
  - `cargo test -p benchmark_comparison --test exactly_once -- --nocapture | cat`
  - `cargo test -p benchmark_comparison --test rotation -- --nocapture | cat`
  - `cargo test -p benchmark_comparison --test compaction -- --nocapture | cat`
  - `cargo test -p benchmark_comparison --test binary_e2e -- --nocapture | cat`

## When to keep SQL

- Heavy ad‑hoc analytics over huge historical datasets (Lithair favors pre‑computed projections)
- Cross‑application shared database (Lithair is embedded and app‑scoped)
- Hybrid: export events to a warehouse; keep OLTP in Lithair for ultra‑low latency

## Adoption checklist

- Model entities and relations in `State` + indexes
- Define events and guard invariants in `apply`
- Add `idempotence_key()` to command‑driven events
- Provide snapshot serializers for fast restarts
- Enable rotation, schedule compaction
- Cover invariants with tests (uniqueness, FK, concurrency, recovery)
