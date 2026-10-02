# RFC 296: PostgreSQL storage and the storage-tier model

Status: agreed direction, **not implemented**. Decisions: [Decisions](#decisions).
Discussion and tracker: [#296](https://github.com/lithair/lithair/issues/296).

## Summary

Add PostgreSQL as a second opt-in external storage backend (`lithair-postgres`), next to the embedded Turso adapter, and define the storage-tier model it belongs to: **where a model's truth lives (authority)**, **where copies may live (cache tiers)**, and **which data must be durable in an external store**. The goal is hybridization: everything that can stay in Lithair's internal memory-first storage stays there; what must live in an external database goes there; a later cache layer moves copies, never authority, between tiers.

## Motivation

- **Hybridization.** Lithair is memory-first by design. Some data should not be: data whose confidentiality, durability or compliance requirements are better served by a mature external database (encryption at rest, point-in-time recovery, managed backups, auditing, database roles).
- **A shared external store.** Turso is embedded: one process, one file, no high availability ([RFC 235](235-hybrid-storage.md)). Several Lithair nodes cannot share it; #248 instead plans to replicate it through Raft. PostgreSQL is the natural shared, independently operated store.
- **The second backend RFC 235 asked for.** RFC 235: *"Do not freeze a universal repository/transaction trait until a second backend has exercised its different semantics."* It explicitly deferred PostgreSQL. Postgres has genuinely different semantics: network I/O, connection pools, concurrent writers, serialization failures. That makes it the right test of what is common.

## Related work

- **RFC 235 / #235, #241**: Turso document storage, migrations, one authority per model.
- **#285** (shipped in `lithair-turso` 0.3.0): SQL application commands (read, decide, atomic multi-collection write).
- **#248**: three-node clusters, with Turso replicated through Raft. Complementary, see Q5.
- **#73**: tiered/cold storage for native models. This RFC gives it a vocabulary (copies vs authority) but does not implement it.
- **#288**: OIDC verified identity. Sensitive models will usually be accessed through application commands authorized with it.

## Concepts (proposed rules)

**R1. One authority per model** (unchanged from RFC 235). A model's records are owned by exactly one backend: native, Turso or Postgres. No dual writes, and no transaction spans two backends (outbox/saga patterns remain the application's job).

**R2. Authority moves only by explicit migration.** Changing a model's backend is an operator action: transactional or fenced, verified, reversible from a backup. It is never automatic, and the cache never does it. Automatic authority movement is where double writes and lost updates come from.

**R3. Caches hold copies, never authority.** Writes always go to the authority. A copy is keyed by the authority's version, has an invalidation rule, and documents its staleness guarantee. Reads that need current state (authorization, commands) bypass copies.

**R4. Durability is declared and constrains authority.** "Sensitive" means **durability**: data that must survive the loss of any Lithair node and benefit from the database's backups, point-in-time recovery and replication (Q1). A model declared `durable` must have an L3 (external) authority; startup refuses it on native or embedded storage. Cache copies of durable models are allowed under R3, since copies never weaken the authority's durability. Confidentiality (no copies, encryption, redaction) is a separate concern, out of scope here, and may get its own declaration later.

### Tier vocabulary

| Tier | Where | Today |
|---|---|---|
| L1 | Process memory | Native models (SCC2), always resident |
| L2 | Local durable storage | Native event log/snapshots; embedded Turso |
| L3 | External database | **New**: PostgreSQL |

Today each model's authority is in exactly one tier, and nothing is copied across tiers. #73 is "L1 copies of L2-authoritative native data". The future cache is "L1/L2 copies of L3-authoritative data". Both are copy mechanisms under R3; neither moves authority.

## PostgreSQL adapter v1 (`lithair-postgres`, experimental)

**Scope: parity with `lithair-turso`**, nothing more. The same declaration style (`#[storage(postgres, collection = …, namespace = …)]`), with a generated `HttpExposable::storage_factory` so `lithair-core` gains no driver dependency, and the same public surface:
- `Store<T>`: get, list with SQL filtering and pagination, create, update, patch, delete, bounded atomic batches
- `Migration` declarations, run before serving
- application `Commands` (`Decision`, `Write`, `View`)
- the generated CRUD routes, sessions and permission hooks

**Data model (Q4, Q7).** The same document model as Turso, as rows in the `lithair` schema: `lithair.documents(namespace, collection, id, body jsonb, primary key (namespace, collection, id))` plus the schema-tracking table. Namespaces are rows, not Postgres schemas: moving a model between Turso and Postgres stays a row copy, a model migration runs once rather than once per tenant, and adding a tenant needs no DDL. `jsonb` gives indexable filters later. Typed columns and generated relational schemas stay out of scope (no ORM, as in RFC 235).

**Database ownership (Q3).** Lithair owns the database it is given. At startup the adapter creates and upgrades its own objects in a dedicated `lithair` schema, tracked by an internal format version (separate from model schema migrations), under an advisory lock so several nodes starting together upgrade once. Model migrations (`Migration`) run as with Turso, once per partition.

**Connections and secrets.**
- `tokio-postgres` with rustls, `sslmode=verify-full` required. Plain TCP is only allowed through an explicit `*_for_tests` flag limited to loopback, like the OIDC adapter.
- A bounded `deadpool-postgres` pool (Q2), statement and connection timeouts.
- Credentials read from an environment variable or a file, never logged.
- The configured role owns the `lithair` schema (Q3); it needs no rights on the application's other schemas.

**Transactions: the main semantic difference.**
- Turso: one global connection lock plus `BEGIN IMMEDIATE`, so commands never conflict.
- Postgres: real concurrent transactions, potentially from several Lithair processes. **Every adapter transaction runs `SERIALIZABLE`** (Q6), with bounded automatic retry on `40001`/`40P01`. This gives Postgres models the same guarantee Turso's global lock gives today, which several nodes writing one database requires. A command's decision is **re-run** on retry; decisions are already required to be deterministic and free of external effects (#285), which is what makes this safe.
- **Unknown outcomes.** A connection lost during `COMMIT` leaves the outcome unknown. As with Turso and native commands, admitted work continues after caller cancellation, and retries reuse the same business key, resolved by the application's durable receipt.

**Multi-node (Q5).** Several Lithair processes may share one database, and correctness comes from Postgres isolation, not from a process lock. Postgres models are **allowed next to a native cluster**: any node serves them, independently of Raft leadership, and each model still has one authority. This is how X Lithair nodes on one PostgreSQL cluster tolerate node failures. The database must then be highly available itself (managed service or Patroni-style failover), otherwise it is the single point of failure; reconnection after a database failover is part of qualification. Node-local state (pending OIDC logins, memory sessions) is not shared by this.

**Limits.** The same bounds as Turso (document size, batch size, page size), so applications can switch between the two backends without surprises.

## Shared contract (after v1, not before)

Once both adapters pass the **same behavioral test suite**, extract only what they actually share, for example a `lithair-sql` contract crate: `Store`, `Mutation`, `Page`/`Equal`, `Migration`, `Commands`/`Decision`/`Write`/`View`, the HTTP factory. Backend-specific behavior (isolation, retries, pooling, TLS) stays in the adapters. The shared test suite is the evidence that the contract is real; we don't design the trait up front.

## Qualification

- **A real PostgreSQL in CI**: a pinned container started by a cidx test phase. No mocks.
- **A parity suite** run against both Turso and Postgres: CRUD, filters and pagination, permission hooks, migrations (including rollback), namespace isolation, the commands behaviors from #285.
- **Postgres-specific checks**:
  - concurrent writers from two processes, serialization-conflict retries
  - connection loss before, during and after `COMMIT`
  - database restart
  - refusal of plain TCP or an invalid certificate
  - a least-privilege role (and the DDL behavior chosen in Q3)
  - statement timeouts, pool exhaustion

## Phasing

1. **This RFC**: R1–R4 and the decisions below (done).
2. **`lithair-postgres` 0.1**: Turso parity, with the qualification above.
3. **Shared contract extraction**, driven by the parity suite.
4. **Cache tiers**: a separate RFC building on R3/R4 (and absorbing #73's design).

## Decisions

| | Question | Decision |
|---|---|---|
| Q1 | Meaning of "sensitive" | **Durability**: a `durable` model requires an L3 authority; copies allowed (R4). Confidentiality is out of scope. |
| Q2 | Pool | **`deadpool-postgres`** on `tokio-postgres` + rustls. |
| Q3 | DDL ownership | **Lithair owns its database**: creates and upgrades its `lithair` schema at startup, versioned, under an advisory lock. |
| Q4 | Documents first | **Yes**: `jsonb` documents, typed columns later. |
| Q5 | Native clusters + Postgres | **Allowed**, for fault tolerance of N nodes on one (HA) PostgreSQL cluster. |
| Q6 | Isolation | **`SERIALIZABLE` everywhere**, with bounded automatic retry. |
| Q7 | Namespaces | **Rows** in a dedicated `lithair` schema, for migration simplicity. |

## Alternatives considered

- **sqlx**: compile-time query checking needs a database at build time, and the macros weigh on the build. `tokio-postgres` is lighter and enough for a fixed set of statements.
- **An ORM (Diesel, SeaORM)**: rejected, as in RFC 235: Lithair owns the declaration, not a relational schema generator.
- **Only replicated Turso (#248)**: complementary, not a substitute. It keeps data local and replicated through Raft, but doesn't bring an external, independently operated store with PITR, managed backups and database roles.
- **libSQL server / Turso cloud**: a different product with different durability and operational trade-offs. Postgres has the broader operational ecosystem for sensitive data.
