# Lithair

> Solid as stone, light as air.

Building a web app shouldn't require assembling a frontend framework, a backend
framework, a database, an ORM, a migration tool, and a deployment pipeline.
Most ideas die in that setup phase.

Lithair takes a different approach: you define your data model, and a complete
backend emerges from it. One struct, one binary, no layers.

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
reconstruction on restart. No database to install, no ORM to configure, no
migrations to manage. `cargo run` and you're live.

## Why

Traditional web development stacks layers: HTTP framework, database driver, ORM,
migration system, session store, auth middleware, permission checks. Each layer
adds complexity, dependencies, and failure modes.

Lithair collapses these layers into one. Your data model is your API, your
schema, your persistence, and your access control. Everything runs in a single
binary with no external services.

This works because most applications don't need a separate database server. They
need to store data, serve it over HTTP, and control who can access what. Lithair
does exactly that, in memory, with event sourcing for durability.

## Install

```toml
[dependencies]
lithair-core = "0.1"
serde = { version = "1.0", features = ["derive"] }
tokio = { version = "1", features = ["full"] }
```

Derive macros (`DeclarativeModel`, `LifecycleAware`, `Page`, `RbacRole`) are
included by default. No need to add `lithair-macros` separately.

## What you get

**Declarative models** -- Annotate fields to control the full stack. `#[db]` for
storage constraints, `#[http]` for API exposure, `#[permission]` for access
control, `#[lifecycle]` for audit trails, `#[persistence]` for replication.

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

**Event sourcing** -- Every mutation is an immutable event in `.raftlog` files.
On restart, events replay to reconstruct state. You get a full audit trail and
time-travel debugging for free.

**Sessions and authentication** -- Built-in session management with persistent
storage, JWT support, and cookie-based auth.

**RBAC** -- Field-level role-based access control. Define who can read and write
each field directly on the struct.

**Distributed consensus** -- OpenRaft integration for multi-node clusters with
leader election and data replication.

**HTTP server** -- Built on Hyper. Includes health checks, firewall with IP
filtering and rate limiting, and gzip compression.

**Memory-first static serving** -- Static assets load into memory at startup.
No disk I/O per request.

**Single binary** -- No PostgreSQL, no Redis, no Docker. One `cargo build`,
one binary, done.

## Quick Start

See the [Getting Started guide](docs/guides/getting-started.md) for a
walkthrough including sessions, RBAC, and the builder API.

## Examples

| Example | Description |
| ------- | ----------- |
| [`minimal_server`](examples/minimal_server/) | Simplest possible server |
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

```text
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
