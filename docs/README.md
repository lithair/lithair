# Lithair Documentation

Data-First Rust framework for high-performance backends.

**Philosophy:** "In Memory We Trust, In Data We Believe"

---

## Quick Start

```bash
# Clone and run
git clone https://github.com/lithair/lithair
cd lithair
task scc2:demo
```

```rust
#[derive(DeclarativeModel)]
pub struct Product {
    #[db(primary_key)]
    #[http(expose)]
    #[permission(read = "Public")]
    pub id: Uuid,

    pub name: String,
    pub price: f64,
}
```

1 struct = REST API + Database + Security + Audit

---

## Core Concepts

| Attribute | Purpose |
|-----------|---------|
| `#[db(...)]` | Database constraints (primary_key, indexed, unique) |
| `#[http(...)]` | REST API generation (expose, validate) |
| `#[permission(...)]` | RBAC security (read, write) |
| `#[lifecycle(...)]` | Audit and business rules (audited, immutable) |
| `#[persistence(...)]` | Distributed replication (replicate) |

**Guides:**
- [Data-First Philosophy](guides/data-first-philosophy.md)
- [Getting Started](guides/getting-started.md)

---

## Tutorials

### E-commerce
Build a complete store with products, orders, and payments.
[E-commerce Tutorial](guides/ecommerce-tutorial.md)

### Blog with RBAC
Blog system with roles (Admin, Editor, Viewer).
[RBAC Guide](guides/rbac.md)

### Distributed Cluster
3 nodes with Raft consensus and automatic replication.
[Clustering](features/clustering/overview.md)

---

## API Reference

### Configuration
- [Environment Variables](reference/env-vars.md)
- [Full Configuration](configuration-reference.md)

### Attributes
- [Attributes Reference](reference/declarative-attributes.md)
- [Generated REST API](reference/api-reference.md)

### Architecture
- [Overview](architecture/overview.md)
- [Data Flow](architecture/data-flow.md)

---

## Examples

All examples are in the `examples/` folder:

| Example | Description |
|---------|-------------|
| `scc2_server_demo` | High-performance server (40M+ ops/sec) |
| `raft_replication_demo` | 3-node cluster with consensus |
| `rbac_sso_demo` | SSO Authentication + RBAC |
| `blog_server` | Complete blog with admin |
| `ecommerce_app` | E-commerce store |
| `minimal_server` | Minimal server to get started |

```bash
# Run an example
cd examples/scc2_server_demo
cargo run
```

---

**Author:** Yoan Roblet (Arcker)
**License:** MIT / Apache-2.0
