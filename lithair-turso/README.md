# lithair-turso (experimental)

An unpublished, opt-in typed document repository backed by the embedded Turso
Rust engine. Native Lithair models keep their existing storage and dependencies.
See [RFC 235](../docs/rfcs/235-hybrid-storage.md) for the design and limitations.

```rust,ignore
impl lithair_turso::SqlModel for Archive {
    const COLLECTION: &'static str = "archives_v1";
    const FILTER_FIELDS: &'static [&'static str] = &["category"];
}

let db = lithair_turso::Database::open("archive.db").await?;
let archives = db.store::<Archive>("trusted-tenant")?;
archives.create(item, &authenticated_permissions).await?;
let page = archives.list(
    lithair_turso::Page { limit: 25, offset: 0 },
    Some(lithair_turso::Equal { field: "category", value: "work" }),
    &authenticated_permissions,
).await?;
```

`SqlModel` extends `HttpExposable`; derived models reuse their generated validation,
read and write checks. `update` checks the existing and replacement objects;
`delete` checks the existing object. Permissions and namespaces are supplied by
trusted application code after authentication. There is no automatic session-gate
or full DeclarativeModel annotation integration. HTTP serialization remains the
application's responsibility; do not expose private struct fields accidentally.

## Guarantees and bounds

- One source of truth per model. Native models and SQL models may coexist.
- An acknowledged mutation has committed. A batch commits entirely or rolls back.
- Same-namespace/model primary keys are unique. Model and namespace keys are bound
  parameters. No raw SQL is accepted through the repository.
- Clone one `Database` per file within a process; operations share a serialized
  connection. Concurrent duplicate creates produce one success, other calls fail.
- Once a write is admitted, caller cancellation does not cancel the transaction.
  A timed-out caller must reconcile the outcome; await writes before runtime shutdown.
- Up to 100 mutations per batch, 100 SQL candidates per page, 1 MiB of encoded JSON
  per document, 512 bytes per ID, 128 bytes per namespace/collection.
- Exact string filters, ordering and pagination execute in SQL. JSON filters may
  scan partition rows on disk; no indexed-filter performance claim is made.
- Permission checks filter each bounded SQL candidate page. Offset counts candidates,
  not visible rows; a short/empty page does not establish end-of-data. No total is
  returned. Concurrent modifications can shift offset pages.

## Explicitly unsupported

Secondary uniqueness/foreign keys, relational schema generation/migration, field
immutability or audit history, native retention, replication, SSE, SQL projections,
and cross-backend transactions. Do not request these via annotations on SQL models.
Use versioned collection names and explicit migrations for schema changes. There
is no transparent cache; reads access SQL. Production promotion needs crash/fault
injection and workload benchmarks beyond the tested ordinary reopen/rollback paths.

The dependency is pinned to Turso 0.7.2 with binding defaults disabled. No experimental
engine modes, cloud sync or allocator replacement are enabled by this adapter.
Do not mix SQLite and Turso processes writing the same file.

## Validation

`cidx run test` includes the repository unit tests and the registered
`cucumber-tests/tests/turso_test.rs` Gherkin runner. `cidx run ci` also validates
security, formatting/lints and the complete workspace release build.

See the [mixed HTTP example](../examples/advanced/hybrid-storage/README.md).
