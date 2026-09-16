# RFC 235: optional SQL storage, declarative Turso integration

Status: experimental prototype, not a stable 1.x storage API.
Issue: https://github.com/lithair/lithair/issues/235

## Decision

Keep native Lithair models as the default. Add an experimental `lithair-turso`
crate that applications explicitly depend on. A model has one authoritative
store: either the existing memory-first/event-sourced path or SQL. An application
may use both. The model opts in with `#[storage(turso)]`; the usual `with_model` registers
its storage and HTTP routes. SQL I/O never enters native model reads.

Three different products must not be conflated:

1. Native models: existing event sourcing, retention and optional replication.
2. SQL models: SQL owns the current state and transactions; reads query SQL.
3. SQL projections: future derived state, rebuilt from a durable native stream
   with an atomic projection checkpoint, idempotency and catch-up semantics.

The first milestone implements (2) alongside (1). PostgreSQL and (3) are deferred.
There is no distributed transaction between native and SQL models. An application
workflow spanning both needs a separately designed outbox/saga or compensation.

## Prototype boundary

`Database` opens an embedded Turso file. Clone that handle inside one process;
its connection is serialized to keep transaction ownership explicit. `store<T>`
binds a stable `SqlModel::COLLECTION` and a trusted namespace (for example a
server-selected tenant). Namespaces and identifiers are bound values, never SQL.
The namespace is a data partition, not authentication: never let an untrusted
request select arbitrary namespaces or supply its own permission list.

`DeclarativeModel` generates `SqlModel` and an optional `HttpExposable::storage_factory`
hook. The default hook is `None` (native); the generated Turso hook references
only the application dependency, keeping core independent of the driver. All
three builder registration methods honor this hook. Native handler constructors
reject externally stored models instead of creating a second authority.

The application
chooses a collection version such as `archive_v1`; a schema change requires an
explicit migration/new collection. Records use `(namespace, model, id)` as primary
key and a JSON body. This demonstrates storage ownership without prematurely
committing to a generic SQL schema generator or ORM.

The repository calls the existing `HttpExposable::validate`, `can_read` and
`can_write` hooks. Updates authorize both the stored and proposed objects; deletes
authorize the stored object. A change never changes the primary key. The generated HTTP adapter uses shared session extraction and cookie cross-site
checks. The builder's session-presence gate applies in any registration order.
Permissions come from a trusted session or the configured role checker; model
hooks always execute. Programmatic repository callers still supply trusted
permissions explicitly. Responses use the stored serde representation, without
field-level response filtering. No arbitrary SQL endpoint is exposed.

Supported: typed create/get/update/patch/delete, atomic bounded batches within one model
and namespace, exact string equality on explicitly allowed JSON fields, stable ID
ordering, bounded limit/offset pagination, restart and partition isolation.

Not provided: secondary uniqueness, relational foreign keys, schema migration,
lifecycle auditing/immutability, native history, native retention, SSE, Raft,
SQL projections, joins through the repository API or transparent cache coherence.
The macro rejects declarations requesting those capabilities. SQL models
must opt in explicitly. Startup rejects native clustering and data-admin on a
server with SQL models so native backups/history cannot silently omit SQL data.
Native RAM metrics report zero resident SQL records without scanning SQL.
`with_declarative_model` selects the adapter but does not run native migrations.
See the adapter README for the exact annotation and HTTP surface.

## Pagination and authorization

Filtering, ordering and limit/offset execute in SQL, then read-permission hooks
filter the bounded candidate page. No collection-wide in-memory load occurs.
A permission-filtered page may be short or empty even if later pages contain
visible records; offset counts SQL candidates, not visible records. There is no
unfiltered total or inaccessible-row ID exposed. This limited contract is explicit
until an authorization-aware SQL query interface is designed. Concurrent edits can
shift offset pages; snapshot/keyset pagination is a later capability.

The primary-key index bounds keyed lookups. JSON field equality may scan matching
partition rows on disk; the prototype makes no indexed-filter performance claim.
Limits on page size, batch count and encoded document size bound allocations.

## Transactions, cancellation and errors

Writes use a dedicated owned Tokio task and `BEGIN IMMEDIATE`. Dropping the caller
future does not cancel an admitted write: the task finishes commit/rollback while
holding the connection lock. A timeout/disconnect therefore has an unknown outcome
to the caller; use stable IDs and reconcile before retrying. There is no exactly-once
claim. Await writes before shutting down the Tokio runtime. Process termination
relies on Turso recovery, not async destructor execution.

A failed batch is explicitly rolled back; rollback failures propagate instead of
being hidden. One shared connection prevents reads from observing partial batches.
No connection or SQL transaction is exposed publicly. Completed writes are
acknowledged only after commit. Ordinary reopen is tested; power-loss durability
and filesystem fault injection remain prerequisites for production promotion.

## Dependency and rollout

Pin the published `turso = 0.7.2` binding with default features disabled (no allocator
replacement, FTS or cloud sync requested). Enable no experimental engine options.
This is the embedded Rust engine, not the libSQL cloud driver. Check the resolved
transitive graph: disabling binding defaults does not eliminate all SDK deps.

Upstream references, reviewed 2026-09-15:
- https://github.com/tursodatabase/turso
- https://github.com/tursodatabase/turso/blob/main/COMPAT.md
- https://docs.rs/turso/0.7.2/turso/

Compatibility is not complete; do not concurrently open the same file with SQLite
and Turso processes. Start with a single process and share one Database handle.
Validate a released version before dependency updates, rather than tracking main.

## Acceptance and next decision

The mixed example must serve a native model and an SQL model in one Lithair server.
Contract tests cover persistence, transaction rollback, SQL filters/pagination,
permission checks, concurrent writes and namespace/model isolation. A registered
Gherkin runner exercises the SQL promises in the cidx test gate. Core-only downstream
builds must not acquire Turso as a dependency.

Keep `with_model` as the application entry point. Next decisions concern
SQL-aware administration/backup, authorization-aware queries and production
recovery evidence. Do not freeze a universal repository/transaction trait until
a second backend has exercised its different semantics. The current extension
point is a model-handler factory, not a generic SQL ORM.
