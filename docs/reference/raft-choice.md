# Lithair: The Opinionated Choice of Raft Protocol

## 🎯 **Why Raft? Our Architectural Philosophy**

Lithair makes an **opinionated choice**: we want distributed systems to be
more approachable for application developers. We chose the Raft consensus
protocol as a foundation because it lets Lithair expose replication through a
smaller declarative surface:

```rust
#[derive(DeclarativeModel)]
struct Product {
    #[persistence(replicate)]  // ← One line = Full distribution
    pub name: String,
}
```

**Result**: write normal application code and let the framework handle the
consensus path for replicated data.

## 🌟 **The Problem Raft Solves for Lithair**

### **Traditional Distributed Development:**

```rust
// Complex manual replication
fn create_product(product: Product) -> Result<()> {
    // 1. Validate leader status
    if !self.is_leader() { return Err("Not leader"); }

    // 2. Replicate to followers
    for follower in &self.followers {
        follower.send_append_entries(product.clone())?;
    }

    // 3. Wait for majority acknowledgment
    let acks = self.wait_for_majority_acks()?;

    // 4. Apply to state machine
    if acks >= self.majority() {
        self.eventstore.append(product)?;
    }

    // 5. Handle failures, retries, split-brain...
    // ... 50+ lines of consensus logic
}
```

### **Lithair Declarative Approach:**

```rust
#[derive(DeclarativeModel)]
struct Product {
    #[persistence(replicate)]  // Raft handles ALL the above automatically
    pub name: String,
}

// HTTP POST → Raft consensus → All nodes updated
// No separate manual replication layer in application code
```

## ⚡ **Why Raft Over Alternatives?**

### **Raft vs. Simple Database Replication**

| Feature                    | Simple Replication          | Raft Consensus                  |
| -------------------------- | --------------------------- | ------------------------------- |
| **Split-brain protection** | ❌ Manual handling          | ✅ Automatic                    |
| **Consistent ordering**    | ❌ Easy to get wrong        | ✅ Ordered through the protocol |
| **Failure recovery**       | ❌ Complex setup            | ✅ Built-in                     |
| **Network partitions**     | ❌ Higher coordination risk | ✅ Explicit safety rules        |

### **Raft vs. External Services (Redis/PostgreSQL)**

| Aspect            | External Service        | Lithair+Raft                  |
| ----------------- | ----------------------- | ----------------------------- |
| **Dependencies**  | Redis, PostgreSQL, etcd | Fewer moving parts by default |
| **Network hops**  | App → DB → Replication  | Direct peer-to-peer           |
| **Configuration** | Complex cluster setup   | `#[persistence(replicate)]`   |
| **Debugging**     | Multi-service debugging | Single binary                 |

## 🏗️ **Lithair's Raft Integration Architecture**

```text
┌─────────────────────────────────────────────────────────────┐
│                    Lithair Node                           │
├─────────────────────────────────────────────────────────────┤
│  HTTP Request                                               │
│    ↓                                                        │
│  DeclarativeHttpHandler                                     │
│    ↓                                                        │
│  [persistence(replicate)] detected?                        │
│    ↓ YES                                                    │
│  DeclarativeConsensus (OpenRaft)                           │
│    ↓                                                        │
│  Propose operation to cluster                              │
│    ↓                                                        │
│  Majority consensus achieved                               │
│    ↓                                                        │
│  Apply to ALL EventStores                                  │
│    ↓                                                        │
│  Return success to client                                   │
└─────────────────────────────────────────────────────────────┘
```

## 💡 **Transparent Distribution in Practice**

### **Developer Experience**

```rust
#[derive(DeclarativeModel)]
struct Product {
    #[persistence(replicate)]
    pub name: String,

    #[persistence]
    pub local_cache_hint: String,
}
```

```bash
# Runnable public example
cd examples/09-replication
cargo run -p replication --bin replication-declarative-node -- \
  --node-id 1 --port 8080
```

```http
POST /api/products
Content-Type: application/json

{"name": "test-product"}
```

### **What Happens Under The Hood**

1. **POST** hits any node in cluster
2. **Raft leader** receives proposal
3. **Consensus** across all nodes automatically
4. **EventStore** updated on the committed nodes
5. **GET** from any up-to-date node returns the same committed data

### **Guarantees Provided**

- ✅ **Linearizability**: Operations appear atomic
- ✅ **Durability**: Data persists across failures
- ✅ **Consistency**: Nodes converge on the same committed log
- ✅ **Partition tolerance**: Cluster survives network splits

## 🎯 **Why This Matters for Real Applications**

### **E-commerce Platform Example**

```rust
#[derive(DeclarativeModel)]
struct Order {
    #[persistence(replicate)]  // Critical business data
    pub customer_id: String,
    pub amount: Money,
    pub status: OrderStatus,
}

#[derive(DeclarativeModel)]
struct ProductView {
    #[persistence]  // Cache, doesn't need replication
    pub rendered_html: String,
}
```

**Result**: orders can remain consistent across committed nodes for critical
flows like payments or inventory, while caches remain local for performance.

### **Financial Services Example**

```rust
#[derive(DeclarativeModel)]
struct Transaction {
    #[persistence(replicate)]  // Money movements must be consistent
    pub from_account: AccountId,
    pub to_account: AccountId,
    pub amount: Money,
}
```

**Result**: the replicated log helps avoid divergent account state between
healthy quorum members.

## 🔥 **Performance Benefits**

### **Raft + EventStore**

- **EventStore**: Append-only writes
- **Raft**: Ordered consensus
- **Result**: a coherent fit for replicated event streams, with the usual
  latency cost of consensus

### **Illustrative benchmark snapshot** (3-node scenario)

- **240+ ops/sec** distributed writes in a representative workload
- **Sub-10ms** consensus latency in that scenario
- **No data loss observed in this scenario** during node failures

## 🚫 **What Lithair is NOT**

### **Not a Database**

- No SQL queries
- No complex indexes
- Pure event sourcing model

### **Not for Every Use Case**

- Single-node apps: Use `#[persistence]` (local EventStore)
- Cache-heavy apps: Mix `#[persistence]` + `#[persistence(replicate)]`
- Complex queries: Use read projections

### **Not Magic**

- Network failures still exist
- Consensus has latency cost
- Minority partitions can't accept writes

## 📚 **When to Use Each Mode**

| Data Type              | Attribute                   | Reasoning                        |
| ---------------------- | --------------------------- | -------------------------------- |
| **User accounts**      | `#[persistence(replicate)]` | Critical, must be consistent     |
| **Financial records**  | `#[persistence(replicate)]` | Regulatory/accuracy requirements |
| **Session data**       | `#[persistence]`            | Temporary, node-local OK         |
| **Rendered templates** | `#[persistence]`            | Can be regenerated               |
| **Metrics/logs**       | `#[persistence]`            | Local aggregation acceptable     |

## 🎯 **Conclusion: Opinionated but Powerful**

Lithair chooses Raft because we believe:

1. **Distributed systems should be simpler to use**
2. **Consensus is often a good default for critical business data**
3. **Fewer external dependencies can reduce operational complexity**
4. **Declarative tooling can reduce repeated distributed plumbing**

The result: write `#[persistence(replicate)]` and get a replicated path that
fits Lithair's opinionated model without wiring the consensus mechanics
yourself.

For teams that want this model, it can be a practical way to keep distributed
backend code smaller and more uniform.
