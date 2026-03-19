# Lithair

> Solid as stone, light as air.

Lithair is a Rust framework for building APIs and websites without taking on
more stack complexity than the project actually needs. Define your data model,
enable the features you want -- REST API, authentication, permissions,
replication, frontend serving -- and keep the result coherent.

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
reconstruction on restart. For many projects, that is enough to get useful work
done without assembling a separate database layer, ORM, and service glue.

## The idea

Not every project needs a microservice architecture, a managed database, and
an orchestration layer. Most applications need to store data, serve it over
HTTP, and control who can access what.

Lithair does exactly that, in a single compiled binary. Because it's Rust, you
get native performance with minimal CPU and RAM -- just what your application
actually needs, nothing more. Because it's compiled, there's no runtime, no
interpreter, no garbage collector in the way.

Lithair is modular rather than fixed-menu. Event sourcing, frontend serving,
sessions, permissions, and replication can be enabled when they are useful, and
left out when they are not. The goal is not to replace every architecture; it
is to offer a simpler default when one coherent binary is the right trade-off.

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
Prometheus-compatible monitoring.

**Admin interface** -- Optional data admin API (`/_admin/data/*`) lets you
browse models, export data, inspect event history, and trigger backups. Schema
management (`/_admin/schema/*`) handles migrations with approval workflows,
diffs, and rollback. Enable the `admin-ui` feature for an embedded HTML
dashboard.

**Memory-first static serving** -- Static assets load into memory at startup.
No disk I/O per request.

**Single binary by default** -- Start with one deployable binary and add
external components only when your constraints truly require them.

## Quick Start

See the [Getting Started guide](docs/guides/getting-started.md) for a
walkthrough including sessions, RBAC, and the builder API.

## Examples

| Example                                                | Description                           |
| ------------------------------------------------------ | ------------------------------------- |
| [`01-hello-world`](examples/01-hello-world/)           | Simplest possible server              |
| [`04-blog`](examples/04-blog/)                         | Blog with frontend and content models |
| [`06-auth-sessions`](examples/06-auth-sessions/)       | Sessions and authentication           |
| [`07-auth-rbac-mfa`](examples/07-auth-rbac-mfa/)       | RBAC and MFA patterns                 |
| [`09-replication`](examples/09-replication/)           | Multi-node replication                |
| [`05-ecommerce`](examples/05-ecommerce/)               | E-commerce workflow                   |
| [`08-schema-migration`](examples/08-schema-migration/) | Schema evolution patterns             |
| [`advanced/datatable`](examples/advanced/datatable/)   | Data tables with filtering            |

```bash
cargo run -p hello-world
cargo run -p auth-sessions
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
