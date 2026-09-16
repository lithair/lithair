# Evolve a Turso model

Turso models store JSON documents. Declare their version and Rust transformations
in `#[storage(...)]`; the usual `with_model` or `with_declarative_model` registration
applies pending transformations before serving HTTP. These are document migrations,
not generated SQL columns or native `#[schema(...)]` migrations.

This API is available with Lithair 1.12 and `lithair-turso` 0.2 on crates.io.
Use the following dependencies in the application:

```toml
[dependencies]
lithair-core = "1.12"
lithair-turso = "0.2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

If upgrading from adapter 0.1, edit its manifest requirement to `"0.2"` before
running `cargo update`. If using `lithair-macros` directly, update it to `"1.12"`
as well. Core re-exports the macros with its default features.

## Start with a stable collection and version

For a new model, start at version 1:

```rust,ignore
#[derive(Clone, serde::Serialize, serde::Deserialize, lithair_core::DeclarativeModel)]
#[storage(turso, collection = "notes", version = 1)]
struct Note {
    #[db(primary_key)]
    id: String,
    title: String,
}
```

Keep `collection = "notes"`, namespace and data directory stable across upgrades.
Changing a collection or namespace selects a different partition, not a new version
of the old data. Renaming only the Rust struct or its containing module is fine.

Existing unversioned declarations keep working. To adopt version 1 explicitly,
add `version = 1`: Lithair validates all existing documents and records the schema.
Existing databases from adapter 0.1.0, which have no schema metadata, are treated
as version 1 and can also upgrade directly to a later version.

## Add a required field

Add `serde_json = "1"` to the application’s `[dependencies]` for JSON
transformations. Replace the previous declaration with this model and function:

```rust,ignore
use serde_json::{json, Value};

fn note_v2(document: &mut Value) -> lithair_turso::Result<()> {
    let fields = document.as_object_mut().ok_or_else(|| {
        lithair_turso::Error::Validation("expected a document".into())
    })?;
    fields.entry("category").or_insert_with(|| json!("personal"));
    Ok(())
}

#[derive(Clone, serde::Serialize, serde::Deserialize, lithair_core::DeclarativeModel)]
#[storage(turso, collection = "notes", version = 2, migrations(note_v2))]
struct Note {
    #[db(primary_key)]
    id: String,
    title: String,
    category: String,
}
```

Registration stays the same:

```rust,ignore
LithairServer::new()
    .with_model::<Note>("./data/notes", "/api/notes")
    .serve().await?;
```

Old notes receive `"category": "personal"`. New HTTP writes use the new model
and must supply its required fields as usual. Restarting at version 2 does not
run `note_v2` again. An empty partition starts directly at version 2 without
running transformations for nonexistent documents.

For a later rename or type conversion, add a function operating on the JSON
field names and declare `version = 3, migrations(note_v2, note_v3)`. Entry one
always upgrades 1 → 2, entry two upgrades 2 → 3. Keep the complete ordered history,
including already deployed steps, so applications can skip intermediate releases.
Do not edit deployed transformations; add a new version. Function bodies are not
checksummed. A new model can use `serde(default)` or `serde(alias)` inside its
normal deserialization rules, but tracking a declaration change still requires
increasing the version.

## Startup and failure behavior

Each `(namespace, collection)` has its own stored version and schema descriptor.
For an upgrade, Lithair visits documents in primary-key pages of at most 100 and
runs their pending transformations in order. All document changes and schema
metadata commit in one transaction. The final JSON must deserialize into the
current model, pass its validation, retain its primary key and fit within 1 MiB.
JSON fields ignored by the current Rust model are preserved by the migration.

If a callback returns an error or panics, or a document is invalid, startup
returns an error and rolls back the migration. The previous data and version
remain available for retry. SQL errors propagate, including rollback failures;
a commit/storage failure can have an uncertain outcome, so reopen and check the
stored version before retrying. Completed migration steps are not rerun on
ordinary restart. The connection lock also prevents repository reads/writes from
observing partially migrated documents. Atomicity is per partition, not across
all models registered in a server.

A tracked model refuses to start with a lower version or a changed schema at the
same version. The generated descriptor includes field names, Rust type syntax,
serde attributes, primary-key and HTTP validation annotations. It excludes the
model struct/module name and permissions. This is a conservative syntactic check:
field reordering or type spelling changes can require a version bump, and changes
inside custom types, serializers or validation functions must be versioned by the
application because the derive cannot inspect their implementation.

Existing typed repository handles also reject operations once another handle
upgrades their partition. The guard belongs to the new adapter: **old binaries
using adapter 0.1.0 do not understand the metadata**. Do not use an old adapter
binary to reopen an upgraded file. Application rollback requires a compatible
model or restoring the pre-upgrade data; automatic down migrations are not provided.

Callbacks run as trusted application code, without request permission checks.
They must be synchronous, deterministic, and limited to transforming the supplied
JSON. Do not perform external side effects: SQL rollback cannot undo them. Once
admitted, a migration finishes even if its caller is cancelled; await startup or
`prepare()` before shutting down the runtime. Retrying after cancellation rechecks
the stored version. The driver still determines recovery after process or storage
faults; this does not change Turso's experimental status.

Run upgrades during application startup with one process per file, after keeping
a recoverable copy of the previous data. Large datasets need startup time and
transaction/WAL disk space even though document memory is paged. The native
migration mode/lock, admin endpoints and native backups do not control or back up
these SQL migrations. No native-to-Turso conversion is performed.

## Programmatic repositories

Repository callers can prepare explicitly before accepting work:

```rust,ignore
let database = lithair_turso::Database::open("./data/notes/model.db").await?;
let notes = database.store::<Note>("default")?;
notes.prepare().await?;
```

Repository operations also prepare lazily if needed. Clone the shared `Database`
handle; do not open independent handles for one file. Manual `SqlModel`
implementations can supply `VERSION`, a stable nonempty `SCHEMA` descriptor and
`MIGRATIONS: &[lithair_turso::Migration]`; the derive generates these for declarations
with an explicit version. Namespace selection remains trusted application code.
