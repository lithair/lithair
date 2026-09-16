# Use Turso models in another application

Turso storage is experimental and is not included in the published Lithair
v1.10.0 crates. Until a release includes the adapter, use Git dependencies pinned
to the same tested revision. Cargo retrieves the crates from the repository;
you do not need a local Lithair checkout.

## Dependencies

Create a Rust application using the toolchain in Lithair's
[`rust-toolchain.toml`](../../rust-toolchain.toml) (1.97.1 for this revision).
Use these dependencies in its `Cargo.toml`:

```toml
[dependencies]
lithair-core = { git = "https://github.com/lithair/lithair", rev = "1153d4145a53400bbfe54fcb65702810868bb0ab" }
lithair-turso = { git = "https://github.com/lithair/lithair", rev = "1153d4145a53400bbfe54fcb65702810868bb0ab" }
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread", "signal"] }
```

`lithair-core` re-exports `DeclarativeModel` with its default `macros` feature.
No separate `lithair-macros` dependency is needed. If your application already
depends on it directly, move it to the **same Git URL and revision** too. Replace
existing crates.io dependencies on these crates throughout your workspace;
mixing registry and Git copies produces distinct Rust types and traits.
Commit the application's `Cargo.lock` to keep its resolved dependencies stable.

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

This Git installation makes the integration available for application trials;
it does not change its experimental status. A crates.io release still needs
compatible core/macros versions, a publishable `lithair-turso` manifest and an
updated release publishing order. Recovery under process/filesystem faults and
workload measurements remain separate work before production promotion.
