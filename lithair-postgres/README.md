# lithair-postgres (experimental)

Optional PostgreSQL storage for declarative Lithair models, designed in
[RFC 296](https://github.com/lithair/lithair/blob/main/docs/rfcs/296-postgres-storage-tiers.md).
It has the same surface as [`lithair-turso`](https://github.com/lithair/lithair/blob/main/lithair-turso/README.md): typed
stores, generated CRUD routes, schema migrations and application commands. The
difference is that records live in an external database that several Lithair
processes can share, and that brings its own backups, point-in-time recovery and
replication.

Use it for data that must be **durable** beyond any Lithair node. Everything
that can stay in Lithair's memory-first storage should stay there: each model
has exactly one authority, and no transaction spans two backends.

## Usage

```toml
[dependencies]
lithair-core = "1.17"
lithair-postgres = "0.1"
```

```rust,ignore
use lithair_postgres::{Database, PostgresConfig};

#[derive(Clone, serde::Serialize, serde::Deserialize, lithair_core::DeclarativeModel)]
#[storage(postgres, durable, collection = "invoices", filters("customer"))]
struct Invoice {
    #[db(primary_key)]
    id: String,
    customer: String,
    total_cents: u64,
}

// The URL is a secret: read it from the environment, never log it.
Database::connect(
    PostgresConfig::from_env("LITHAIR_POSTGRES_URL")?.with_ca_file("/etc/ssl/db-ca.pem"),
)
.await?
.install()?; // used by every #[storage(postgres)] model

LithairServer::new()
    .with_model::<Invoice>("./data/invoices", "/api/invoices")
    .serve()
    .await?;
```

`#[storage(postgres)]` accepts the same options as Turso (`collection`,
`namespace`, `filters`, `version`, `migrations`), plus `durable`. `durable` is
only accepted with an external backend: declaring it on native or Turso storage
fails at compile time (RFC 296, R4). The model directory passed to `with_model`
holds no data. It is checked so that a model that used to be native or Turso
cannot silently switch backends: moving a model's authority is an explicit
migration.

The programmatic API matches `lithair-turso`: `Database::store::<T>(namespace)`
with `get`, `list`, `create`, `update`, `patch`, `delete`, `batch` and `prepare`,
and `Database::commands(namespace, collections)` with `execute`.

## Differences from Turso

| | Turso | PostgreSQL |
| --- | --- | --- |
| Location | Embedded file, one process | External server, shared by any number of processes |
| Concurrency | One connection behind a lock | A pool (`deadpool-postgres`, 16 connections by default) |
| Isolation | Serialized by the lock | `SERIALIZABLE`, replayed on conflict (up to 10 attempts) |
| Command decision | `FnOnce` | `Fn`: it runs again after a conflict |
| Transport | Local file | TLS required and verified |

**Transactions.** Every statement runs in a `SERIALIZABLE` transaction. When
PostgreSQL aborts one because of a concurrent conflict (serialization failure or
deadlock), the whole transaction is replayed from the start, including a
command's decision, which must therefore be deterministic and free of external
effects. Without this, two processes patching different fields of one document,
or racing on the same revision, would lose updates; the test suite demonstrates
it.

**TLS.** Connections always use TLS with certificate and host name
verification, whatever `sslmode` the URL says. The CA comes from
`with_ca_file`, or from the system trust store by default. Unix sockets are
refused. `allow_insecure_loopback_for_tests()` allows plain TCP to loopback
hosts only, for local test servers; never enable it in a deployment.

**Database ownership.** Lithair owns the database (or at least the `lithair`
schema) it is given. On connect it creates or upgrades its objects
(`lithair.documents`, `lithair.schemas`, `lithair.format`) under an advisory
lock, so nodes starting together upgrade once. A database written by a newer
format is refused. The role needs `CREATE` on the database the first time;
afterwards it only uses its own schema.

**Records.** Documents are `jsonb` rows keyed by `(namespace, collection, id)`,
with byte-order (`COLLATE "C"`) keys so pagination matches Turso. Namespaces
are rows, not PostgreSQL schemas: moving a model between Turso and PostgreSQL is
a row copy, and a migration runs once, not once per tenant. Model migrations
also run under a per-partition advisory lock, so several nodes preparing the
same model migrate it exactly once.

## L1 cache (since 0.1.1)

`#[retention(memory = N, max_mb = M, ttl = "5m")]` on a `#[storage(...)]` model
keeps L1 copies of its records in this node's memory
([RFC 304](https://github.com/lithair/lithair/blob/main/docs/rfcs/304-external-storage-cache.md)).
Only point reads (`get`, `GET /api/x/{id}`) are served from copies; lists and
filters always query the database. Permission hooks run on every read, cached or
not. Bounds: `memory` (records, least recently used evicted first), `max_mb`
(encoded size) and `ttl` (maximum age of a copy). "Not found" is cached for at
most 5 seconds. The `LT_<MODEL>_MEMORY_RETENTION`, `LT_<MODEL>_MEMORY_MAX_MB`
and `LT_<MODEL>_CACHE_TTL` variables override them at deploy time. Without
`#[retention]`, nothing is cached. `memory = "<duration>"` and `#[pinned]` stay
native-only.

Writes always go to the database and evict the copies they touch (batches,
commands, migrations) as soon as the transaction ends, so a node always reads
its own writes. Application commands and the read inside a write never use
copies; `Store::get_fresh` reads the current state explicitly, and
`Store::cache_stats` reports hits, misses, evictions and resident size.

**Across nodes**, every write transaction also runs `pg_notify` for each record
it touches: the notification exists only if the transaction commits. Each node
keeps one listening connection (`application_name = lithair-cache-listener`) and
evicts the named copies, typically milliseconds after commit. Caches stay
disabled until that connection listens. If it is lost, every copy is flushed
and caching stays off until it reconnects (with backoff), since notifications
sent meanwhile are lost. The TTL is the last safety net. A migration on any node
flushes the partition everywhere. If the listener cannot start, preparing a
cached model fails, unless `PostgresConfig::with_ttl_only_cache()` explicitly
chooses copies that only expire with their TTL (every cached model then needs a
`ttl`, and another node's write can stay invisible until it expires).

Every write publishes its notifications, whether or not any node caches that
model, so a node that enables a cache (for example during a rolling upgrade)
never depends on other nodes' declarations. This adds a small cost to every
write and serializes commits on PostgreSQL's notification queue.

## Next to a native cluster

PostgreSQL models can be registered on a server that runs a native consensus
cluster (`with_native_cluster`, RFC 296 Q5). Native models keep their consensus
rules: writes go through the Raft leader and need a quorum. PostgreSQL models
are served by **every node**, follower or leader, and keep working when the
cluster has lost its quorum, since their authority is the shared database:

```rust,ignore
Database::connect(config).await?.install()?;
LithairServer::new()
    .with_admin_panel(false)
    .with_native_cluster(cluster)                    // native, replicated models
    .with_model::<Invoice>("./data/invoices", "/api/invoices") // PostgreSQL
    .serve()
    .await?;
```

`build()` still refuses local models next to a cluster (native handlers outside
the cluster, Turso), as well as a PostgreSQL route that overlaps a native
cluster route. The native cluster's other restrictions apply to the whole
server, including no local sessions or RBAC stores yet. PostgreSQL models are
therefore served with their anonymous permission hooks.

## Guarantees and limits

- Acknowledged writes have committed. Batches and commands commit entirely or
  not at all.
- Once admitted, a write completes even if its caller is dropped. A timeout, or
  a connection lost during `COMMIT`, is an **unknown outcome**: reconcile
  stable IDs, or retry a command with the same business key so its receipt
  resolves it.
- The pool replaces broken connections, and operations work again after the
  server restarts. For node fault tolerance, the database must itself be highly
  available (managed service or a failover manager); otherwise it is the single
  point of failure.
- The same bounds as Turso: 100 mutations per batch, 100 candidates per page,
  1 MiB documents, 512-byte IDs, 128-byte namespaces and collections.
  Statements time out after 30 seconds by default.
- Permission checks filter SQL candidate pages, exactly as with Turso.

**Not yet provided:**
- Field-level encryption and confidentiality rules.
- Typed relational columns, cross-backend transactions, and the cache tiers.

## Qualification

`cidx run postgres-test` (part of `cidx run test`) runs
`scripts/postgres-tests.sh`. It installs PostgreSQL 17 in the CI container,
generates a test-only PKI, and runs the suite against the real server over TLS:

- CRUD, filters, pagination, permissions, validation and batch rollback
- refusal of a foreign CA and of insecure non-loopback transport; URL redaction
- two pools ("processes") patching one document and racing on one ID
- four nodes migrating one partition concurrently, exactly once; downgrades refused
- atomic commands, receipt replay, revocation before replay, a revision race
  across processes, rollback on rejection, refusal of raw writes to typed
  partitions, decision panics
- a newer `lithair` format refused
- generated HTTP routes from the installed database
- killed connections and a server restart (a separate test binary)
- three native nodes plus one database: a follower serves PostgreSQL writes,
  every node reads them, native writes on a follower are refused, and after
  two nodes stop the last one still serves PostgreSQL but not native models;
  local models and overlapping routes are refused next to the cluster

Running the suite with `READ COMMITTED` instead of `SERIALIZABLE` makes the
concurrency tests fail (lost update, two winners), and so does disabling the
retry.
