# E-commerce Tutorial

> This page replaces an earlier tutorial that was written against a
> pre-v0.7 API surface (`Lithair`, `RaftstoneApplication`, manual
> `Route::post` registration, generic `Request<T>` / `Response<T>`
> handlers). That shape is no longer how Lithair is used, and
> following it would not compile against current Lithair.

The maintained e-commerce walkthrough now lives as a runnable example
in the workspace:

- **Code:** [`examples/05-ecommerce/`](https://github.com/lithair/lithair/tree/main/examples/05-ecommerce)
- **Run:** `cargo run -p ecommerce`
- **Browse:** `http://localhost:8080`

## What it covers

The example demonstrates the v0.11+ idiomatic shape:

- Multiple `#[derive(DeclarativeModel)]` entities (`Category`,
  `Product`, `Order`) registered on a single `LithairServer`
- A REST API auto-generated per model — no hand-written handlers
- Foreign-key relations via `#[db(references = "...")]`
- A single binary serving the API, the data, and (optionally) a
  compiled frontend

```rust
use lithair_core::app::LithairServer;
use lithair_core::DeclarativeModel;
use serde::{Deserialize, Serialize};

#[derive(DeclarativeModel, Serialize, Deserialize, Clone, Debug)]
struct Product {
    #[db(primary_key)]
    #[http(expose)]
    id: String,
    #[http(expose, validate = "non_empty")]
    name: String,
    #[http(expose)]
    price: f64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    LithairServer::new()
        .with_port(8080)
        .with_model::<Product>("./data/products", "/api/products")
        .serve()
        .await
}
```

For sessions, RBAC, and admin surfaces layered on top, see the
canonical examples and their docs:

- [`examples/06-auth-sessions/`](https://github.com/lithair/lithair/tree/main/examples/06-auth-sessions) — sessions and authentication
- [`examples/07-auth-rbac-mfa/`](https://github.com/lithair/lithair/tree/main/examples/07-auth-rbac-mfa) — RBAC plus MFA
- [`examples/04-blog/`](https://github.com/lithair/lithair/tree/main/examples/04-blog) — frontend serving with content models
