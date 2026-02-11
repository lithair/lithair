# lithair-macros

Procedural macros for the [Lithair](https://crates.io/crates/lithair-core) framework.

## Usage

You typically don't need to add this crate directly. `lithair-core` re-exports all
macros by default via its `macros` feature:

```toml
[dependencies]
lithair-core = "0.1"  # includes macros
```

```rust
use lithair_core::prelude::*;

#[derive(DeclarativeModel, Serialize, Deserialize, Clone, Debug)]
struct Product {
    id: String,
    name: String,
    price: f64,
}
```

## Available Macros

| Macro | Type | Description |
|-------|------|-------------|
| `DeclarativeModel` | derive | Generates CRUD specs from field attributes (`#[db]`, `#[http]`, `#[permission]`) |
| `LifecycleAware` | derive | Generates lifecycle policies (immutability, versioning, auditing) |
| `Page` | derive | Generates page-centric API with CORS, CRUD, and RBAC |
| `RbacRole` | derive | Generates permission checks from `#[permissions]` attributes |
| `lithair_model` | attribute | Adds serde defaults for schema migration |

## License

See the [repository](https://github.com/lithair/lithair) for license information.
