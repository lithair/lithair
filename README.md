# Lithair

> Solid as stone, light as air.

Lithair is a Rust framework for building APIs and websites without taking on
more stack complexity than the project actually needs. Define your data model,
enable the features you want -- REST API, authentication, permissions,
replication, frontend serving -- and keep the result coherent.

```rust
use lithair_core::app::LithairServer;
use lithair_core::DeclarativeModel;
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
lithair-core = "0.13"
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

**Distributed consensus** -- Multi-node clusters with leader election and
majority-acknowledged replication, production-stable within a documented
operating envelope ([cluster runbook](docs/operations/cluster.md)). Opt-in via
the `cluster` feature; single-node deployments don't pay for it.

**HTTP server** -- Built on Hyper. Includes firewall with IP filtering and
rate limiting, gzip compression, and CORS.

**Live updates over SSE** -- Each registered model exposes a
`GET /api/{model}/stream` endpoint that streams creates, updates,
and deletes to subscribed clients as Server-Sent Events. Writes
made through the REST API, the programmatic handler, or
replicated from peers all broadcast on the same channel. Opt-in
via `.with_sse(true)`.

**Host-header routing** -- A single binary can serve multiple hostnames, each
with its own frontend. Models and custom routes remain host-agnostic in this
first iteration. Background and rationale:
[The Layer I Stopped Choosing](https://arcker.org/blog/2026-04-24-lithair-vhost-routing/).

```rust
LithairServer::new()
    .with_vhost("arcker.org", |v| v.with_frontend_at("/", "sites/arcker.org"))
    .with_vhost("lithair.net", |v| v.with_frontend_at("/", "sites/lithair.net"))
    .with_default_vhost(|v| v.with_frontend_at("/", "sites/lithair.net"))
    .serve()
    .await
```

**Host-to-host redirects** -- Declarative 301 redirects between hostnames,
useful for canonical-URL enforcement (e.g. `www.` to bare domain) without a
separate reverse proxy.

```rust
LithairServer::new()
    .with_redirect("www.arcker.org", "arcker.org")
    .with_redirect("www.lithair.net", "lithair.net")
    .serve()
    .await
```

**Built-in operations** -- Every Lithair server comes with `/health`, `/ready`,
`/info`, and `/metrics` endpoints out of the box. The `/metrics` endpoint
exposes Prometheus-compatible gauges, including per-model storage stats
(item count, .raftlog size, snapshot size).

**Admin interface** -- Optional data admin API (`/_admin/data/*`) lets you
browse models, export data, inspect event history, and trigger backups. Schema
management (`/_admin/schema/*`) handles migrations with approval workflows,
diffs, and rollback. Enable the `admin-ui` feature for an embedded HTML
dashboard.

**Memory-first static serving** -- Static assets load into memory at startup.
No disk I/O per request.

**Single binary by default** -- Start with one deployable binary and add
external components only when your constraints truly require them.

## Storage and memory model

Before adopting Lithair for a project with a non-trivial dataset, read
this section. The storage model is a first-order operational property
of Lithair and the README is the right place to surface it.

**What lives in RAM.** Every item registered via `with_model::<T>(...)`
is held in memory, in full, for the lifetime of the server. After
startup, the framework replays the `.raftlog` (and any latest
snapshot) and reconstructs the full collection into a lock-free
concurrent HashMap (SCC2). By default — without `#[retention]` — there
is no eviction, no LRU, and no on-demand reload: if you registered a
model, you pay for its full size in RAM. The *Memory/disk tiering*
subsection below covers how `#[retention]` bounds this for large
datasets.

**What lives on disk.** The `.raftlog` event log holds every mutation
(create / update / delete) in append-only form, plus periodic
snapshots. Disk is the durability and replay surface, not the query
surface. Queries always hit RAM.

**Memory sizing.** Rough formula for a single model:

```text
RAM(T) ≈ item_count × average_serialized_size(T)
```

For a model with 50 000 items averaging 4 KB each, count on ~200 MB
of RAM just for that model's collection. Mutation history in the
`.raftlog` is *additional* disk cost — `.raftlog` auto-compaction
is built in (since v0.8.0): configure with `with_auto_compaction(threshold, interval)`
to bound disk growth.

**What this is good for.** Datasets that comfortably fit in RAM at
your target host's size. The single-binary, event-sourced shape lets
you skip a database and get sub-millisecond queries from the same
process that serves HTTP. Common comfortable sizes today are tens of
thousands to a few hundred thousand items per model.

**Memory/disk tiering (v0.12+).** Datasets where you expect cold
archive growth that can't fit in RAM (multi-year audit logs, large
mail archives) are now addressable via the `#[retention]` +
`#[pinned]` annotations. Declare how many items stay fully in
memory (or for how long, or under what byte budget) and which
fields survive eviction:

```rust
#[derive(DeclarativeModel)]
#[retention(memory = 1000)]      // or memory = "30d" or max_mb = 512
pub struct Email {
    #[pinned] pub from: String,   // always in RAM
    #[pinned] pub subject: String,
    pub body: String,             // reloaded from event store on demand
}
```

The event store remains the source of truth — evicted items are
reloaded by replaying their events when accessed. Listing/filtering
on pinned fields stays instant. Use this for the cold tail; the
hot working set should still fit in RAM comfortably.

**Operational checklist** before deploying:

- Estimate `item_count × average_serialized_size` per model.
- Set your host's RAM with margin (`2-3 ×` the estimated total, to
  cover replay spikes, snapshot generation, and Rust allocator
  overhead).
- Plan `.raftlog` disk growth — enable `with_auto_compaction(threshold, interval)`
  if mutations are frequent.

For full RAM/disk/CPU sizing — including the retention-bounded RAM
model and disk compaction — see
[`docs/operations/capacity-planning.md`](docs/operations/capacity-planning.md).
For the durability semantics of the `.raftlog` (fsync mode, crash
safety), see [`lithair-core/DURABILITY.md`](lithair-core/DURABILITY.md).

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
| [`10-blog-distributed`](examples/10-blog-distributed/) | Multi-node blog with consensus       |
| [`11-frontend-integrations`](examples/11-frontend-integrations/) | Astro / SPA integration patterns     |
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

### Containerized CI with cidx (optional)

[cidx](https://github.com/cidx-org/cidx) runs the same code-quality, security, test,
and build phases locally in Docker containers, matching what GitHub Actions does.
Useful for reproducing CI failures without pushing.

```bash
cidx run code      # rustfmt + clippy
cidx run security  # cargo-audit + gitleaks + trivy
cidx run test      # workspace unit tests (lib + bins)
cidx run build     # workspace release build
cidx run ci        # full pipeline
```

CI mirrors the same phases via `.github/workflows/cidx.yml`, which installs
the latest cidx release at workflow build time (`go install
github.com/cidx-org/cidx/cmd/cidx@latest`). It runs alongside the existing
`ci.yml` and `ci-fast.yml` workflows during the integration cycle.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT License](LICENSE-MIT) at your option.

---

Built by [Yoan Roblet (Arcker)](https://github.com/arcker)
