# Getting Started with Lithair

Lithair is a **declarative backend framework** where you define your data models and Lithair automatically generates CRUD APIs, event sourcing, sessions, RBAC, and distributed consensus.

## Prerequisites

- Rust 1.95+ installed (the toolchain currently pinned by Lithair)
- Basic knowledge of Rust programming
- Understanding of web APIs (helpful but not required)

## Philosophy: Declare, Don't Implement

Instead of writing controllers, routes, and database queries, you:

1. **Define** your data structures
2. **Configure** server features with `.with_X()`
3. **Let Lithair handle** everything else

## Quick Start: Simple CRUD API

### 1. Create Your Project

For a new application, prefer the CLI and follow the generated README:

```bash
cargo install lithair-cli
lithair new my-app
cd my-app
cargo run
```

The manual setup below is useful when adding Lithair to an existing Cargo
project.

```bash
cargo new my-app
cd my-app
```

### 2. Add Lithair Dependency

Edit `Cargo.toml`:

```toml
[dependencies]
lithair-core = "1.0"
serde = { version = "1.0", features = ["derive"] }
tokio = { version = "1", features = ["full"] }
anyhow = "1.0"
```

> **Note:** `lithair-core` includes derive macros by default (via the `macros` feature).
> No need to add `lithair-macros` separately.

### 3. Define Your Model

Edit `src/main.rs`:

```rust
use lithair_core::app::LithairServer;
use lithair_core::DeclarativeModel;
use serde::{Deserialize, Serialize};

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

### 4. Run Your Server

```bash
cargo run
```

### 5. Test Your API

```bash
# Create a product
curl -X POST http://localhost:3000/api/products \
  -H 'Content-Type: application/json' \
  -d '{"id":"1","name":"Laptop","price":999.99}'

# List all products
curl http://localhost:3000/api/products

# Get specific product
curl http://localhost:3000/api/products/1

# Update product
curl -X PUT http://localhost:3000/api/products/1 \
  -H 'Content-Type: application/json' \
  -d '{"id":"1","name":"Gaming Laptop","price":1299.99}'

# Delete product
curl -X DELETE http://localhost:3000/api/products/1
```

## What Just Happened?

From **10 lines of code**, Lithair automatically provided:

**5 REST endpoints** (GET, GET/:id, POST, PUT, DELETE)
**Event sourcing** - All changes persisted in `.raftlog` files
**JSON serialization** - Automatic conversion
**HTTP routing** - Path matching and request handling
**Error handling** - Proper status codes (400, 404, 500)
**State reconstruction** - Events replayed on server restart

You wrote **zero** lines for any of this infrastructure!

## Add Sessions and Authentication

The shortest path is `with_rbac_config`: it creates the event-sourced
session store, mounts `POST /auth/login`, `POST /auth/logout` and
`GET /auth/validate`, and issues the session cookie at login.
`with_models_require_session(true)` then gates every auto-generated
`/api/*` route behind a valid session (401 otherwise).

```rust
use lithair_core::app::LithairServer;
use lithair_core::rbac::{RbacUser, ServerRbacConfig};
use lithair_core::DeclarativeModel;
use serde::{Deserialize, Serialize};

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
        .with_rbac_config(
            ServerRbacConfig::new()
                .with_role("Admin", vec!["*".to_string()])
                .with_user(RbacUser::new("admin", "password123", "Admin"))
                .with_session_store("./data/sessions"),
        )
        .with_models_require_session(true)
        .with_model::<Product>("./data/products", "/api/products")
        .serve()
        .await
}
```

Login once, then use either transport — the same session is accepted by
both:

```bash
# Login: the body carries the token, and Set-Cookie: session_token=... too
curl -c jar.txt -X POST http://localhost:3000/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"password123"}'

# Browser style: the cookie alone is enough
curl -b jar.txt http://localhost:3000/api/products

# API style: Authorization: Bearer <session_token from the login body>
curl http://localhost:3000/api/products -H "Authorization: Bearer $TOKEN"

# Logout clears the cookie (Max-Age=0) and destroys the session
curl -b jar.txt -X POST http://localhost:3000/auth/logout
```

The cookie's attributes (`Secure; HttpOnly; SameSite=Lax; Path=/` by
default) come from `[sessions]` in `config.toml` / the `LT_SESSION_COOKIE_*`
env vars, or from `.with_session_cookie(CookieConfig { .. })` on the builder.
Sessions are persisted via event sourcing.

If you would rather bring your own login handler, wire the store yourself
with `.with_sessions(SessionManager::from_arc(store))` — see
[docs/features/sessions.md](../features/sessions.md).

## Add RBAC (Role-Based Access Control)

`with_rbac_config` already derives a `PermissionChecker` from the roles you
declare, and the model gate checks it. To plug in your own checker instead,
register the model with `with_model_full`:

```rust
use lithair_core::app::LithairServer;
use lithair_core::rbac::PermissionChecker;
use lithair_core::session::{PersistentSessionStore, SessionManager};
use lithair_core::DeclarativeModel;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(DeclarativeModel, Serialize, Deserialize, Clone, Debug)]
struct Product {
    id: String,
    name: String,
    price: f64,
}

// Define your permission rules
struct RolePermissionChecker;

impl PermissionChecker for RolePermissionChecker {
    fn has_permission(&self, role: &str, permission: &str) -> bool {
        match (role, permission) {
            ("Customer", "ProductRead") => true,
            ("Employee", "ProductRead") | ("Employee", "ProductWrite") => true,
            ("Admin", _) => true, // Admin has all permissions
            _ => false,
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // PersistentSessionStore::new is synchronous and takes a PathBuf.
    let session_store = Arc::new(PersistentSessionStore::new("./data/sessions".into())?);
    let permission_checker = Arc::new(RolePermissionChecker);

    LithairServer::new()
        .with_port(3000)
        // from_arc: the store is shared with the model below (issue #80)
        .with_sessions(SessionManager::from_arc(session_store.clone()))
        .with_model_full::<Product>(
            "./data/products",
            "/api/products",
            Some(permission_checker),
            Some(session_store as Arc<dyn std::any::Any + Send + Sync>),
        )
        .serve()
        .await
}
```

**Lithair now automatically:**

- Extracts the role from the session (Bearer token or `session_token` cookie)
- Checks permissions before CREATE, UPDATE, DELETE
- Returns `403 Forbidden` if unauthorized
- Allows READ for all authenticated users

## The Declarative Pattern

### Traditional (Imperative) Approach

```rust
//  200+ lines of boilerplate code

// Define routes
app.post("/api/products", create_product);
app.get("/api/products", list_products);
app.get("/api/products/:id", get_product);
app.put("/api/products/:id", update_product);
app.delete("/api/products/:id", delete_product);

// Implement each handler
async fn create_product(req: Request) -> Response {
    // Parse JSON
    let body = req.body_json()?;

    // Validate
    if body.name.is_empty() { return error(400); }

    // Check permissions
    let user = authenticate(&req)?;
    if !user.can_create_product() { return error(403); }

    // Save to database
    let product = db.insert(body)?;

    // Return JSON
    Ok(json(product))
}

// ... repeat for 4 more handlers
```

### Lithair (Declarative) Approach

```rust
//  10 lines, all endpoints + auth + validation

#[derive(DeclarativeModel, Serialize, Deserialize)]
struct Product {
    id: String,
    name: String,
    price: f64,
}

LithairServer::new()
    .with_port(3000)
    .with_model::<Product>("./data", "/api/products")
    .serve()
    .await
```

## Next Steps

- **[Session Management Guide](../features/sessions.md)** - Deep dive
  into authentication
- **[RBAC Guide](./rbac.md)** - Advanced permission patterns
- **[Event Sourcing](../modules/storage/event-sourcing.md)** - Understanding .raftlog files
- **[Examples](../examples/README.md)** - Working reference implementations
- **[Distributed Consensus](../modules/consensus/README.md)** - Multi-node clusters

## Working Examples

All examples are in the `examples/` directory with full documentation:

- **`06-auth-sessions`** - Sessions + RBAC (recommended starting point)
- **`09-replication`** - Distributed 3-node cluster and load-testing scripts
- **`advanced/http-firewall`** - DDoS protection and rate limiting scripts
- **`07-auth-rbac-mfa`** - Advanced RBAC and MFA flows

Run any example:

```bash
# From project root
task examples:rbac-session

# Or manually
cargo run -p auth-sessions
```

## Key Concepts

### Event Sourcing by Default

Every change creates an immutable event in `.raftlog` files:

```
./data/products.raftlog
./data/sessions.raftlog
```

On restart, Lithair replays events to reconstruct state. You get:

- Full audit trail
- Time travel debugging
- Automatic persistence
- Zero SQL/migrations

### Builder Pattern API

Configure server features fluently:

```rust
LithairServer::new()
    .with_port(3000)              // Server config
    .with_sessions(manager)        // Authentication
    .with_rbac(true)              // Authorization
    .with_model::<Product>(...)   // Data models
    .with_route(...)              // Custom endpoints
    .serve()                      // Start server
    .await
```

### Zero Boilerplate

You **never write**:

- Route definitions
- Request parsing
- JSON serialization
- Database queries
- Permission checks in handlers
- Error handling

You **only write**:

- Data structures (`struct Product`)
- Business rules (`PermissionChecker`)
- Configuration (`.with_X()`)

## Troubleshooting

### Port Already in Use

```bash
# Error: Address already in use (os error 98)
lsof -ti:3000 | xargs kill -9
```

### Event Log Corruption

```bash
# Remove corrupted .raftlog files
rm -rf ./data/*.raftlog
# Server will start fresh
```

### Build Errors

```bash
# Clean build
cargo clean
cargo build
```

## Philosophy

Lithair follows the **"Declare, Don't Implement"** principle:

> **You describe WHAT you want (data models, permissions, features)**
> **Lithair generates HOW to build it (routes, validation, persistence)**

This reduces:

- 99% less boilerplate code
- Zero infrastructure bugs
- Faster development
- Type-safe everything

---

**Ready to build?** Check out the
[Auth Sessions example](../../examples/06-auth-sessions/README.md) for a
complete working example!
