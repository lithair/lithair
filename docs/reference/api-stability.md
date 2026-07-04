# API Stability — the v1.0 surface

This is the gate **G3** deliverable on the [road to v1.0](../roadmap/v1.0.md):
every public item of `lithair-core` classified into one of three tiers, plus
the MSRV policy. It records *what Lithair promises to keep stable* once 1.0
ships, and what it explicitly does not.

Until 1.0 is tagged this is the *intended* contract; pre-1.0 minors may still
break, but every break is CHANGELOG'd and the tiers below are how breakage is
weighed.

## The three tiers

| Tier | Meaning | 1.x promise |
|------|---------|-------------|
| **Stable** | The supported application surface. | Additive evolution only; removals go through the [deprecation policy](../policy/deprecation.md). |
| **Unstable** | Public, but low-level or implementation-adjacent. Usable, not promised. | May change in any minor. Marked in rustdoc. |
| **Hidden** | Internal, public only for crate-internal reuse / testability. | `#[doc(hidden)]`. No promise; may move or disappear. |

The strongest promise of all — the **on-disk event-store format** — is covered
separately in [the roadmap](../roadmap/v1.0.md#what-stable-will-mean-at-v10):
any 1.x binary must replay an event store written by any earlier 1.x binary.

## Stable surface

The canonical application import is the prelude:

```rust
use lithair_core::prelude::*;
```

It re-exports exactly the stable items most applications need. Everything below
is stable whether reached via the prelude or its defining module.

| Area | Items |
|------|-------|
| Server | `app::LithairServer` and its builder methods (`with_*`, `serve`, `serve_with_graceful_shutdown`) |
| Custom routes | `app::{Method, RouteRequest, RouteResponse, StatusCode, response}` |
| Macros | `DeclarativeModel`, `lithair_model`, `LifecycleAware`, `RbacRole` (under the default `macros` feature) |
| Declarative attributes | `#[db]`, `#[http]`, `#[permission]`, `#[lifecycle]`, `#[retention]`, `#[pinned]`, `#[relation]`, `#[server]`, `#[schema]` (see [declarative-attributes.md](declarative-attributes.md)) |
| Generated REST shapes | The `/api/{model}` endpoint shapes generated from a model |
| Operational endpoints | `/health`, `/ready`, `/info`, `/metrics` (response shapes) |
| Admin API planes | `/_admin/data/*` and `/_admin/frontend/*` request/response shapes — **secure by default** via `with_data_admin()` (issue #143); `with_data_admin_public()` is the explicit opt-out |
| Security / RBAC | `security::{AuthContext, Permission, Role, User, RBACMiddleware, SecurityError, SecurityEvent, SecurityState}` |
| Sessions | `session::{SessionManager, SessionMiddleware, PersistentSessionStore, MemorySessionStore, Session, SessionStore}` |
| Frontend | `frontend::{FrontendEngine, FrontendServer}` |
| HTTP | `http::{Route, FirewallConfig, DeclarativeHttpHandler}` |
| Config | `config::*` builder/enums (e.g. `SchemaMigrationMode`) |
| Cluster | `cluster::ClusterArgs` |
| Schema | `schema::{load_schema_spec, load_schema_history, load_lock_status, SchemaChangeType, …}` |
| Errors | `Error`, `Result` |

Environment variables (`LT_*`) follow the stable surface; the cluster
wire/replication protocol follows the cluster gate (G1) — production-stable
within the documented [operating envelope](../operations/cluster.md), not a
general wire-format promise.

## Unstable surface

Public today, but low-level or tied to the engine's internals. Usable for
advanced/operational needs, not covered by the additive promise — these may
change in any 1.x minor.

| Item | Why public, why unstable |
|------|--------------------------|
| `engine::EventStore` | The `lithair verify` CLI opens it directly to check a backup's hash chain offline, so it must stay public. Its surface is tied to the on-disk format and may evolve with it. |
| `engine::StateEngine`, `engine::LithairApplication` | Low-level engine traits/types (below the `LithairServer` builder). Most apps should prefer `LithairServer`. |
| `engine::{MultiFileEventStore, EngineConfig, EngineError, EngineResult}` | Engine configuration/handles exposed for advanced embedding and tests. |
| `engine::relations::{AutoJoiner, DataSource, RelationRegistry}` | FK auto-join machinery; shape may change as relations evolve. |

## Hidden surface

Internal types kept `pub` only for crate-internal reuse and testability. Marked
`#[doc(hidden)]`; not part of any promise and may move or disappear without a
deprecation cycle.

| Item | Role |
|------|------|
| `engine::scc2_engine::{Scc2Engine, Scc2EngineConfig, Scc2EngineStats, VersionedEntry}` | The lock-free concurrent engine internals |
| `engine::persistence::{FileStorage, DatabaseStats}` | On-disk storage backend |
| `engine::async_writer::{AsyncWriter, DurabilityMode, WriteEvent}` | Background event-flush writer |
| `engine::persistence_optimized::{AsyncEventWriter, OptimizedPersistenceConfig}` | Alternate persistence path |
| `__private` | Macro support namespace (already hidden) |

## MSRV policy

Lithair pins its toolchain in
[`rust-toolchain.toml`](../../rust-toolchain.toml) — currently **1.95.0** — and
CI builds against that exact version (cidx `rust:1.95.0`). This pin is the
**minimum supported Rust version**: building Lithair requires Rust ≥ the pinned
version.

For 1.x:

- The MSRV will only ever be raised in a **minor** release, never a patch, and
  the bump will be called out in the CHANGELOG.
- The MSRV will not be raised gratuitously — only to adopt a language/stdlib
  feature with clear benefit or to track a dependency's own MSRV.
- There is no commitment to support a trailing window of older compilers
  before 1.0; once 1.0 ships, any tightening of this policy will itself be
  documented here.

## How this is enforced

- The prelude (`lithair_core::prelude`) is the executable definition of the
  stable application surface.
- Hidden items carry `#[doc(hidden)]`, so `cargo doc` shows only the stable +
  unstable surface.
- The [macro parser hardening gate (G2)](../roadmap/v1.0.md#g2--macro-parser-hardening)
  ensures the declarative attribute surface fails the build on unknown tokens
  rather than silently dropping them.
