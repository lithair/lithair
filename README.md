# Lithair

> Solid as stone, light as air.

What if your application compiled from the start? What if you defined your data
model, chose the features you need -- REST API, authentication, permissions,
replication -- and everything just worked? One binary, native performance,
nothing wasted.

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

## The idea

Not every project needs a microservice architecture, a managed database, and
an orchestration layer. Most applications need to store data, serve it over
HTTP, and control who can access what.

Lithair does exactly that, in a single compiled binary. Because it's Rust, you
get native performance with minimal CPU and RAM -- just what your application
actually needs, nothing more. Because it's compiled, there's no runtime, no
interpreter, no garbage collector in the way.

Everything is built in: event sourcing gives you a full history of every change
for free. Replication is integrated if you need it. Sessions, permissions,
firewall -- you pick what you need with annotations and builder methods, and it
compiles into your binary. No external services to install, configure, or maintain.

Your data model is the starting point. Define a struct, annotate the fields,
and the framework generates the rest.

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

**HTTP server** -- Built on Hyper. Includes firewall with IP filtering and
rate limiting, gzip compression, and CORS.

**Built-in operations** -- Every Lithair server comes with `/health`, `/ready`,
and `/info` endpoints out of the box. Enable `/observe/metrics` for
Prometheus-compatible monitoring. No sidecar, no extra service.

**Admin interface** -- Optional data admin API (`/_admin/data/*`) lets you
browse models, export data, inspect event history, and trigger backups. Schema
management (`/_admin/schema/*`) handles migrations with approval workflows,
diffs, and rollback. Enable the `admin-ui` feature for an embedded HTML
dashboard.

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
