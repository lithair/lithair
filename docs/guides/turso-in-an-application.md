# Use Turso models in another application

Turso storage is experimental and available on crates.io as `lithair-turso` 0.2.1,
alongside Lithair 1.12.1. Cargo retrieves the published crates; you do not need a
Lithair checkout or Git dependencies.

## Dependencies

Create a Rust application using the toolchain in Lithair's
[`rust-toolchain.toml`](../../rust-toolchain.toml) (1.97.1 for this revision).
Use these dependencies in its `Cargo.toml`:

```toml
[dependencies]
lithair-core = "1.12"
lithair-turso = "0.2.1"
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread", "signal"] }
```

`lithair-core` re-exports `DeclarativeModel` with its default `macros` feature.
No separate `lithair-macros` dependency is needed. If your application already
depends on it directly, upgrade it to `"1.12"` too. Replace any previous Git or
path dependencies on these crates throughout your workspace; mixing registry
and Git/path copies produces distinct Rust types and traits. Lithair 1.10 does
not include the storage selector required by the adapter.
When upgrading from adapter 0.1, change the manifest requirement to `"0.2"`;
`cargo update` alone cannot cross that 0.x minor boundary. Adapter 0.2 requires
core 1.12, which includes the new migration declarations. Existing unversioned
models and files remain usable. See the
[schema migration guide](turso-schema-migrations.md) before versioning existing data.
Commit the application's `Cargo.lock` to keep its resolved dependencies stable.
For an application already on adapter 0.2.0, `cargo update -p lithair-turso`
selects 0.2.1 without changing model declarations. The adapter remains compatible
with core 1.12.0. The manifest above requires 0.2.1 so the pagination metadata
used below is always available.

## Declare and register the model

Put this in `src/main.rs`:

```rust
use lithair_core::{app::LithairServer, DeclarativeModel};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, DeclarativeModel)]
#[storage(turso, collection = "notes_v1", filters("category"))]
struct Note {
    #[db(primary_key)]
    id: String,
    #[http(validate = "non_empty")]
    title: String,
    category: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    LithairServer::new()
        .with_host("127.0.0.1")
        .with_port(8080)
        .with_model::<Note>("./data/notes", "/api/notes")
        .serve_with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
```

Start the application with `cargo run`. Lithair creates `./data/notes/model.db`
and exposes CRUD routes. This local example has public routes. Use Lithair's
session and model permission declarations for authenticated access; see the
[adapter's authorization contract](../../lithair-turso/README.md#authorization)
and the [authenticated mixed example](../../examples/advanced/hybrid-storage/README.md).

```bash
curl --fail-with-body -X POST http://127.0.0.1:8080/api/notes \
  -H 'Content-Type: application/json' \
  -d '{"id":"note-1","title":"First note","category":"work"}'

curl --fail-with-body 'http://127.0.0.1:8080/api/notes?category=work&limit=25'

curl --fail-with-body -X PATCH http://127.0.0.1:8080/api/notes/note-1 \
  -H 'Content-Type: application/json' -d '{"title":"Updated note"}'
```

Stop with Ctrl-C, restart from the same working directory, then read the saved
document:

```bash
curl --fail-with-body http://127.0.0.1:8080/api/notes/note-1
```

The result contains `"title":"Updated note"`. Use a persistent absolute data
directory in an existing application so deployment working-directory changes
do not select a new database.

## Paginate list requests

When switching a native model to Turso, update clients that previously loaded
the entire collection: SQL lists default to **50 candidates**, and accept at most
100 per request. A 55-document collection needs a second request. `limit=0` and
`limit=101` return 400; limits are not silently clamped.

**Since adapter 0.2.1**, list responses include `has_more` and `next_offset`
alongside the existing `data` array. Adapter 0.1.0 and 0.2.0 do not include
these fields. With the new metadata, the first page looks like:

```json
{"data": [{"id": "note-1", "title": "First note", "category": "work"}], "has_more": true, "next_offset": 1}
```

That example uses `limit=1`. Follow the returned offset with the same filter,
limit and credentials until it is `null`. This browser client uses the current
session cookie and collects pages of 50:

```javascript
async function loadNotes() {
  const notes = [];
  let offset = 0;
  do {
    const query = new URLSearchParams({ category: "work", limit: "50", offset: String(offset) });
    const response = await fetch(`/api/notes?${query}`, { credentials: "same-origin" });
    if (!response.ok) throw new Error(`Cannot load notes: HTTP ${response.status}`);
    const page = await response.json();
    if (!("next_offset" in page)) throw new Error("This adapter needs pagination metadata support");
    notes.push(...page.data);
    offset = page.next_offset;
  } while (offset !== null);
  return notes;
}
```

SQL selects candidate documents in primary-key order, then model permissions
filter that page. A short or empty `data` array is therefore **not** an end
condition. `has_more` means another SQL candidate exists, which might itself be
unreadable; `next_offset` counts candidates, not returned documents. This exposes
continuation of the filtered SQL partition, not a readable-document count. No
`total` is computed. Concurrent changes can shift offsets between requests.

## Serde defaults and SQL filters

A field listed in `filters(...)` must be a `String` with its ordinary serde
name and representation. The current derive rejects **all** `#[serde(...)]`
attributes on that field or the model itself, including `#[serde(default)]`.
This conservative restriction keeps SQL filtering aligned with stored JSON;
there is no supported opt-out in the declaration.

For example, use a required filter field and put defaults on unfiltered fields:

```rust
#[derive(Clone, serde::Serialize, serde::Deserialize, lithair_core::DeclarativeModel)]
#[storage(turso, collection = "attempts", filters("subject"))]
struct Attempt {
    id: String,
    subject: String, // Supply explicitly in POST/PUT; no #[serde(default)] here.
    #[serde(default)]
    comment: String, // Not a SQL filter: defaults are allowed.
}
```

Alternatively remove `subject` from `filters(...)` if it needs serde attributes;
`?subject=...` then becomes unsupported. For existing documents missing a field,
use a [versioned migration](turso-schema-migrations.md) to materialize the value
before adding the filter, rather than relying on a deserialization default.

## Coexistence and upgrades

Register native models with their usual `with_model` calls and separate data
directories. Models without `#[storage(turso)]` keep native storage. Each model
has one authoritative backend. Changing an existing model's backend requires an
explicit migration; adding the annotation does not convert native data.

Keep `collection = "notes_v1"` stable across Rust type renames. Use one process
per database file. Native data-admin and clustering cannot be enabled on a server
containing SQL models in this revision. See the full
[capabilities and limits](../../lithair-turso/README.md#guarantees-and-limits)
before adapting an existing model.

The adapter follows its own 0.x version series while core/macros follow 1.x.
Publication makes the integration available for application trials; it does not
change its experimental status. Recovery under process/filesystem faults and
workload measurements remain separate work before production promotion.

For model evolution with this adapter, see the
[schema versions and migrations guide](turso-schema-migrations.md). It keeps the
same registration and adds explicit versioned document transformations.

### Coming from Lithair 0.12

Upgrade the framework before reusing session guards: 0.12's `RequireAuth` could
fail with `Failed to downcast session store` when paired with `with_sessions`.
The session-manager recognition fix shipped in 0.15 (issue #143).
`RequireRole` was implemented in 0.16 (issue #149); it previously denied every
request. Both fixes are included in 1.x. See the
[0.15](../../CHANGELOG.md#0150---2026-06-18) and
[0.16](../../CHANGELOG.md#0160---2026-06-22) release notes.
