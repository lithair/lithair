# lithair-turso (experimental)

Optional embedded Turso storage selected on a `DeclarativeModel`. Add the
`lithair-turso` dependency to the application, then register models as usual:

```rust,ignore
#[derive(Clone, serde::Serialize, serde::Deserialize, lithair_core::DeclarativeModel)]
#[storage(turso)]
struct Archive {
    #[db(primary_key)]
    id: String,
    #[http(validate = "non_empty")]
    title: String,
}

LithairServer::new()
    .with_model::<LiveTask>("./data/tasks", "/api/tasks") // native by default
    .with_model::<Archive>("./data/archives", "/api/archives")
    .serve().await?;
```

The derive generates `SqlModel` and the adapter factory. `with_model` creates
`./data/archives/model.db` and the CRUD routes on startup. No manual repository
implementation, database opening or HTTP handlers are needed. `with_declarative_model`
and `with_model_full` honor the same selection. The native handler constructors
reject SQL-declared models, including the native `with_model_ref` path.

This crate is **experimental**. Add `lithair-core = "1.12"` and
`lithair-turso = "0.2"` to your application. The
[standalone application guide](https://github.com/lithair/lithair/blob/v1.12.0/docs/guides/turso-in-an-application.md)
provides a complete Cargo manifest and runnable server. Native applications have
no Turso dependency. See [RFC 235](https://github.com/lithair/lithair/blob/v1.12.0/docs/rfcs/235-hybrid-storage.md).

## Declaration and routes

`#[storage(turso)]` defaults to collection = Rust struct name and namespace =
`"default"`. For stable names and optional string equality filters:

```rust,ignore
#[storage(turso, collection = "archives_v1", namespace = "tenant-a", filters("category"))]
```

Choose a separate data directory for each registration. Keep collection/namespace
stable across renames and HTTP path changes. Changing them selects another data
partition; it does not migrate records. Selecting Turso in a directory with
native events/snapshots, or native storage in a directory with `model.db`, refuses
to start. Backend changes require an explicit migration. Filter fields must be `String` with default
serde names/serialization. `limit` and `offset` are reserved. Namespace selection
comes from trusted application code, never query parameters.

| Method | Route | Result |
| --- | --- | --- |
| GET | `/api/archives?category=work&limit=25&offset=0` | `{"data": [...], "has_more": true, "next_offset": 25}` (continuation metadata unreleased) |
| GET | `/api/archives/{id}` | The document |
| POST | `/api/archives` | Create complete model including ID; 201 after commit |
| PUT | `/api/archives/{id}` | Replace complete model; immutable ID |
| PATCH | `/api/archives/{id}` | Merge top-level fields atomically, then validate |
| DELETE | `/api/archives/{id}` | Delete; 200 after commit |
| HEAD / OPTIONS | CRUD routes | Bodyless reads / method discovery |

Writes require `application/json` and at most 1 MiB. Unknown/duplicate query
parameters and unsupported filters are errors. PATCH rejects unknown fields.
Errors distinguish invalid input (400), authorization (401/403), missing records
(404), duplicates (409), media type (415), body limit (413) and storage failure (500).
Bulk HTTP routes, count/schema/SSE subroutes and SQL endpoints are not provided.

### Pagination

Lists default to **50 SQL candidates**, with `limit=1..100` and `offset=0`
by default. They do not return the whole collection. Candidate order is primary
key order. The next adapter release adds `has_more` and `next_offset` alongside
the existing `data` array; published 0.1.0 and 0.2.0 return only `data`.

For 55 readable documents, the default first page has 50 items, `has_more: true`
and `next_offset: 50`. Request the same filter and limit with `offset=50` to read
the final 5 items, `has_more: false` and `next_offset: null`. A full final page
also has `has_more: false`; one extra SQL candidate determines continuation.

Continuation describes **SQL candidates before permission filtering**, not a
promise of further readable documents. Even an empty `data` array can have a
`next_offset`. Follow that value until it is `null`, keeping the same filter,
limit and credentials. No total is computed or exposed. Like existing offset
queries, continuation metadata can reveal the presence of unreadable candidates;
it never returns their content. Offset pagination is not a snapshot across
requests: concurrent edits can shift pages. See the
[application guide](https://github.com/lithair/lithair/blob/main/docs/guides/turso-in-an-application.md#paginate-list-requests)
for a client loop and filter declaration examples.

## Schema versions and migrations

Opt into schema tracking with `#[storage(turso, collection = "notes", version = 1)]`.
For a model change, increase the version and declare the full ordered history,
for example `#[storage(turso, collection = "notes", version = 2, migrations(note_v2))]`.
Each callback has signature `fn(&mut serde_json::Value) -> lithair_turso::Result<()>`.
The ordinary builder migrates existing documents and records their schema in one
transaction before serving. Invalid transformations roll everything back; tracked schema drift and
downgrades fail explicitly. Existing unversioned databases are treated as version 1.

See [model evolution](https://github.com/lithair/lithair/blob/v1.12.0/docs/guides/turso-schema-migrations.md) for runnable
snippets, restart/rollback semantics and compatibility limits. This API requires
Lithair 1.12 and adapter 0.2. Native `#[schema]`, native
migration administration and backend conversion remain separate capabilities.

## Authorization

`with_models_require_session(true)` applies to SQL registrations through
`with_model` and `with_declarative_model`, in any builder order. It requires a
configured session store at startup. Cookie/Bearer precedence, cookie settings,
expiration and cross-site protections use Lithair's shared session machinery.
OPTIONS is exempt from the model session gate.

`#[permission(read = "ArchiveRead", write = "ArchiveWrite")]` uses generated
model-level `can_read`/`can_write` checks on every operation, including anonymous
requests with an empty permission list. Store a trusted `Vec<String>` under
`permissions` in a server-created session, or use the configured RBAC checker to
resolve declared permissions from the session's `role`. `with_model_full` supports
an explicit checker/store. Clients cannot supply permissions via headers or query
parameters. `public_if` read conditions still apply unless an authentication gate
requires a session first. Updates/PATCH authorize both existing and proposed states.

Model permissions are not field-level response filtering. The HTTP representation
is the model's serde representation, also used for storage. Avoid storing secrets
in a model exposed as a whole document.

## Guarantees and limits

- Native memory-first models and SQL models coexist, each with one authority.
- Acknowledged mutations have committed. Batches commit entirely or roll back.
  Programmatic access remains available via `Database::store::<T>(namespace)`.
- Keys are `(namespace, collection, id)`, bound as SQL parameters. A cloned
  `Database` shares one serialized connection; open once per file per process.
- Once admitted, writes complete even when their caller is cancelled. A timeout
  has an unknown outcome: reconcile stable IDs before retrying, and await writes
  before stopping the Tokio runtime.
- At most 100 mutations/batch, 100 candidates/page (default 50), plus one
  lookahead candidate for pagination, 1 MiB encoded
  JSON/document, 512 bytes/ID, 128 bytes/namespace or collection.
- SQL executes filtering, ordering and pagination. Permission checks then filter
  the candidate page: offset counts candidates, an empty page does not prove the
  end of data. Use `next_offset` to continue; no total is returned. Concurrent
  edits can shift offset pages.
- JSON equality may scan partition rows on disk; no indexed-filter performance
  claim or transparent memory cache. Native request paths stay unchanged.

Unsupported declarations fail at compile time: secondary indexes/uniqueness,
foreign keys, native lifecycle/audit/retention/replication, native schema migration, RBAC
owner fields, relations and HTTP serialization modes. Only `db(primary_key)` is
supported among database annotations. Unknown/duplicate storage options also fail.

A server containing SQL models refuses native clustering or native data-admin
configuration at startup: native backups/imports/history cannot represent this
backend. Native RAM metrics report zero resident items for SQL models. Native
SSE/hooks and cross-backend transactions do not apply to SQL mutations.

The exact Turso 0.7.2 binding has defaults disabled. No cloud sync, allocator
replacement or experimental engine options are requested. Do not mix SQLite and
Turso processes on a file. Ordinary restart/rollback are tested; power-loss and
filesystem fault injection plus workload benchmarks remain prerequisites for
production promotion.

`cidx run test` runs repository tests, generated HTTP/restart tests, compile-fail
diagnostics and the Turso Gherkin runner. `cidx run ci` also runs security, code
and the workspace release build. See the [runnable example](https://github.com/lithair/lithair/blob/v1.12.0/examples/advanced/hybrid-storage/README.md).
