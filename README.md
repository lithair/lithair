# Lithair

> Solid as stone, light as air.

Declarative backend framework for Rust. Define your data models and Lithair
generates the complete backend: REST endpoints, event sourcing, sessions, RBAC,
and distributed consensus.

```rust
use lithair_core::prelude::*;
use serde::{Serialize, Deserialize};

#[derive(DeclarativeModel, Serialize, Deserialize, Clone, Debug)]
struct Product {
    id: String,
    name: String,
    price: f64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    LithairServer::new()
        .with_port(3000)
        .with_model::<Product>("./data/products", "/api/products")
        .serve()
        .await
}
```

This gives you 5 REST endpoints, event-sourced persistence, and automatic state
reconstruction on restart. No database, no ORM, no boilerplate.

## Install

```toml
[dependencies]
lithair-core = "0.1"
serde = { version = "1.0", features = ["derive"] }
tokio = { version = "1", features = ["full"] }
```

Derive macros (`DeclarativeModel`, `LifecycleAware`, `Page`, `RbacRole`) are
included by default via the `macros` feature. No need to add `lithair-macros`
separately.

## Features

**Declarative models** - Derive macros generate CRUD APIs from struct definitions.
Annotate fields with `#[db]`, `#[http]`, `#[permission]`, `#[lifecycle]`, and
`#[persistence]` to control database constraints, API exposure, access control,
audit trails, and replication.

```rust
#[derive(DeclarativeModel)]
pub struct User {
    #[db(primary_key, indexed)]
    #[http(expose)]
    #[permission(read = "Public")]
    pub id: Uuid,

    #[db(unique)]
    #[http(expose, validate = "email")]
    #[permission(write = "UserEdit")]
    #[lifecycle(audited)]
    pub email: String,
}
```

**Event sourcing** - All mutations are stored as immutable events in `.raftlog`
files. On restart, events are replayed to reconstruct state. Full audit trail
and time-travel debugging included.

**Sessions and authentication** - Built-in session management with persistent
storage, JWT support, and cookie-based authentication.

**RBAC** - Role-based access control with field-level permissions. Define who
can read and write each field.

**Distributed consensus** - OpenRaft integration for multi-node clusters with
automatic leader election and data replication.

**HTTP server** - Built on Hyper with sub-millisecond latency. Includes health
checks (`/health`), firewall (IP filtering, rate limiting), and gzip compression.

**Memory-first static serving** - Load static assets into memory at startup and
serve them directly from RAM. No disk I/O per request.

**Single binary** - No external databases or services required. Everything runs
in one process.

## Quick Start

See the [Getting Started guide](docs/guides/getting-started.md) for a complete
walkthrough including sessions, RBAC, and the builder API.

## Examples

| Example | Description |
|---------|-------------|
| [`minimal_server`](examples/minimal_server/) | Simplest possible Lithair server |
| [`blog_server`](examples/blog_server/) | Blog with posts and comments |
| [`rbac_session_demo`](examples/rbac_session_demo/) | Sessions + role-based access control |
| [`rbac_sso_demo`](examples/rbac_sso_demo/) | RBAC with SSO integration |
| [`raft_replication_demo`](examples/raft_replication_demo/) | 3-node distributed cluster |
| [`ecommerce_app`](examples/ecommerce_app/) | E-commerce with cart and products |
| [`schema_migration_demo`](examples/schema_migration_demo/) | Schema evolution patterns |
| [`datatable_demo`](examples/datatable_demo/) | Data tables with filtering |

```bash
cargo run -p minimal_server
cargo run -p rbac_session_demo
```

## Architecture

```
lithair-core/src/
  engine/       Event-sourced storage engine (SCC2, lock-free)
  http/         Hyper HTTP server, router, firewall
  rbac/         Role-based access control
  session/      Session management
  consensus/    OpenRaft distributed consensus
  frontend/     Memory-first static file serving
  security/     Authentication, JWT, validation
  lifecycle/    Audit trails, history tracking
  schema/       Auto-generated database schema
```

## Development

Requires [Task](https://taskfile.dev) for build commands:

```bash
task ci:full       # Format + build + clippy + tests (~2-3 min)
task ci:github     # Full validation with smoke tests (~10-15 min)
task test          # Run all workspace tests
task lint          # Clippy with -D warnings
task fmt           # Format code
task help          # List all available tasks
```

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT License](LICENSE-MIT) at your option.

---

Built by [Yoan Roblet (Arcker)](https://github.com/arcker)
