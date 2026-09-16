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

This workspace crate is **unpublished and experimental**. Use a path dependency
from a checkout (as in the example), or follow the
[standalone application guide](../docs/guides/turso-in-an-application.md) for
Git dependencies pinned to a tested revision. It is not part of the published v1.10.0 crates.
Native applications have no Turso dependency. See [RFC 235](../docs/rfcs/235-hybrid-storage.md).

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
| GET | `/api/archives?category=work&limit=25&offset=0` | `{"data": [...]}` |
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
- At most 100 mutations/batch, 100 candidates/page (default 50), 1 MiB encoded
  JSON/document, 512 bytes/ID, 128 bytes/namespace or collection.
- SQL executes filtering, ordering and pagination. Permission checks then filter
  the candidate page: offset counts candidates, an empty page does not prove the
  end of data, and no total is returned. Concurrent edits can shift offset pages.
- JSON equality may scan partition rows on disk; no indexed-filter performance
  claim or transparent memory cache. Native request paths stay unchanged.

Unsupported declarations fail at compile time: secondary indexes/uniqueness,
foreign keys, native lifecycle/audit/retention/replication, schema migration, RBAC
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
and the workspace release build. See the [runnable example](../examples/advanced/hybrid-storage/README.md).
