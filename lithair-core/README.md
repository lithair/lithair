# lithair-core

Declarative framework for building APIs and websites in Rust. Define your data
models, enable the features you need, and keep the result coherent: REST
endpoints, event sourcing, sessions, RBAC, frontend serving, and replication.

## Quick Start

```toml
[dependencies]
lithair-core = "0.1"
serde = { version = "1.0", features = ["derive"] }
tokio = { version = "1", features = ["full"] }
anyhow = "1.0"
```

```rust
use lithair_core::prelude::*;
use serde::{Serialize, Deserialize};

#[derive(DeclarativeModel, Serialize, Deserialize, Clone, Debug)]
struct Product {
    id: String,
    name: String,
    price: f64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    LithairServer::new()
        .with_port(3000)
        .with_model::<Product>("./data/products", "/api/products")
        .serve()
        .await
}
```

This gives you 5 REST endpoints (GET, GET/:id, POST, PUT, DELETE), event sourcing
with `.raftlog` persistence, and automatic state reconstruction on restart.

## Features

- **Declarative**: Derive macros generate CRUD APIs from struct definitions
- **Event Sourcing**: All mutations stored as immutable events
- **Sessions**: Built-in session management with persistence
- **RBAC**: Role-based access control with field-level permissions
- **Modular**: Start with one binary and add external pieces only when needed

Derive macros from `lithair-macros` are included by default via the `macros` feature.

## License

See the [repository](https://github.com/lithair/lithair) for license information.
