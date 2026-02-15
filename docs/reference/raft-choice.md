# Lithair: The Opinionated Choice of Raft Protocol

## 🎯 **Why Raft? Our Architectural Philosophy**

Lithair makes an **opinionated choice**: **true distributed systems should be effortless for developers**. We chose the Raft consensus protocol as our foundation because it enables something revolutionary:

```rust
#[derive(DeclarativeModel)]
struct Product {
    #[persistence(replicate)]  // ← One line = Full distribution
    pub name: String,
}
```

**Result**: Write normal code, get distributed consensus automatically.

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
// Zero manual replication code required
```

## ⚡ **Why Raft Over Alternatives?**

### **Raft vs. Simple Database Replication**

| Feature | Simple Replication | Raft Consensus |
|---------|-------------------|----------------|
| **Split-brain protection** | ❌ Manual handling | ✅ Automatic |
| **Consistent ordering** | ❌ Race conditions | ✅ Guaranteed |
| **Failure recovery** | ❌ Complex setup | ✅ Built-in |
| **Network partitions** | ❌ Data loss risk | ✅ Safety guaranteed |

### **Raft vs. External Services (Redis/PostgreSQL)**

| Aspect | External Service | Lithair+Raft |
|--------|------------------|-----------------|
| **Dependencies** | Redis, PostgreSQL, etcd | Zero external deps |
| **Network hops** | App → DB → Replication | Direct peer-to-peer |
| **Configuration** | Complex cluster setup | `#[persistence(replicate)]` |
| **Debugging** | Multi-service debugging | Single binary |

## 🏗️ **Lithair's Raft Integration Architecture**

```
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

## 💡 **The Magic: Transparent Distribution**

### **Developer Experience**
```rust
// Step 1: Define model (looks like normal Rust)
#[derive(DeclarativeModel)]
struct User {
    #[persistence(replicate)]
    pub email: String,
    
    #[persistence]  // Local only
    pub cache_data: String,
}

// Step 2: Start cluster (no configuration complexity)
cargo run --bin myapp -- --peers 8082,8083

// Step 3: Use normal HTTP APIs
POST /api/users {"email": "test@example.com"}
```

### **What Happens Under The Hood**
1. **POST** hits any node in cluster
2. **Raft leader** receives proposal
3. **Consensus** across all nodes automatically
4. **EventStore** updated on ALL nodes simultaneously
5. **GET** from any node returns identical data

### **Guarantees Provided**
- ✅ **Linearizability**: Operations appear atomic
- ✅ **Durability**: Data persists across failures
- ✅ **Consistency**: All nodes see same data
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

**Result**: Orders are guaranteed consistent across all nodes (payments, inventory), while caches remain local for performance.

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

**Result**: Impossible to have inconsistent account balances across nodes.

## 🔥 **Performance Benefits**

### **Raft + EventStore = Optimal Combo**
- **EventStore**: Append-only writes (ultra-fast)
- **Raft**: Ordered consensus (consistent)
- **Result**: Fast writes + guaranteed consistency

### **Benchmarks** (3-node cluster)
- **240+ ops/sec** distributed writes
- **Sub-10ms** consensus latency
- **Zero data loss** during node failures

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

| Data Type | Attribute | Reasoning |
|-----------|-----------|-----------|
| **User accounts** | `#[persistence(replicate)]` | Critical, must be consistent |
| **Financial records** | `#[persistence(replicate)]` | Regulatory/accuracy requirements |
| **Session data** | `#[persistence]` | Temporary, node-local OK |
| **Rendered templates** | `#[persistence]` | Can be regenerated |
| **Metrics/logs** | `#[persistence]` | Local aggregation acceptable |

## 🎯 **Conclusion: Opinionated but Powerful**

Lithair chooses Raft because we believe:

1. **Distributed systems should be simple to use**
2. **Consensus is better than eventual consistency for business logic**
3. **Zero external dependencies reduces operational complexity**
4. **Declarative beats imperative for distributed programming**

The result: Write `#[persistence(replicate)]` and get enterprise-grade distributed systems automatically.

**This is the future of backend development.**