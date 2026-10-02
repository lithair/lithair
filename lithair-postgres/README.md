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
lithair-core = "1.16"
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
- Postgres models next to a native Lithair cluster (RFC 296, Q5), which is the
  next step.
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

Running the suite with `READ COMMITTED` instead of `SERIALIZABLE` makes the
concurrency tests fail (lost update, two winners), and so does disabling the
retry.
