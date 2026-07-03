# Lithair Framework - API Reference

## Core Types

### `LithairServer`

The recommended high-level entry point for most Lithair applications.

```rust
LithairServer::new()
    .with_port(8080)
    .with_model::<Product>("./data/products", "/api/products")
    .serve()
    .await?;
```

### `lithair_core::prelude`

The recommended one-line import — brings `LithairServer`, the derive/attribute
macros, the custom-route types and the core RBAC types into scope:

```rust
use lithair_core::prelude::*;
```

> **Canonical reference:** this page is a curated overview. The complete,
> always-current API is on [docs.rs/lithair-core](https://docs.rs/lithair-core).
> The stability tier of every public item (stable / unstable / hidden) and the
> MSRV policy are in [`api-stability.md`](api-stability.md); low-level engine
> types (`StateEngine`, `LithairApplication`, `EventStore`) are classified
> *unstable* there — most apps only need `LithairServer` and the declarative
> macros below.

## Macros

### `#[derive(DeclarativeModel)]`

One struct generates the backend: REST endpoints, schema, validation, RBAC,
event sourcing and replication — driven by field attributes (`#[db]`,
`#[http]`, `#[permission]`, `#[lifecycle]`, `#[retention]`, …).

```rust
use lithair_core::prelude::*;
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, DeclarativeModel)]
struct Product {
    #[db(primary_key, indexed)]
    #[http(expose)]
    #[permission(read = "Public")]
    id: String,
    #[http(expose)]
    name: String,
}
```

Register it with `LithairServer::with_model::<Product>("./data/products", "/api/products")`.
The full attribute list is in [`declarative-attributes.md`](declarative-attributes.md).

## HTTP Module

### `HttpServer`

The custom HTTP server implementation.

```rust
pub struct HttpServer {
    // Internal implementation
}

impl HttpServer {
    pub fn new() -> Self
    pub fn bind(addr: &str) -> Result<Self, Error>
    pub fn serve(&self) -> Result<(), Error>
}
```

### `HttpRequest`

Represents an incoming HTTP request.

```rust
pub struct HttpRequest {
    pub method: HttpMethod,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

pub enum HttpMethod {
    GET,
    POST,
    PUT,
    DELETE,
    PATCH,
}
```

### `HttpResponse`

Builder for HTTP responses.

```rust
pub struct HttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn ok() -> Self
    pub fn created() -> Self
    pub fn bad_request() -> Self
    pub fn not_found() -> Self
    pub fn internal_error() -> Self
    pub fn json<T: Serialize>(data: T) -> Self
    pub fn text(content: &str) -> Self
}
```

## Engine Module

### `StateEngine<S>`

Manages application state with concurrent access.

```rust
pub struct StateEngine<S> {
    // Internal implementation with RwLock
}

impl<S> StateEngine<S> {
    pub fn new(initial_state: S) -> Self
    pub fn read<F, R>(&self, f: F) -> R where F: FnOnce(&S) -> R
    pub fn write<F, R>(&self, f: F) -> R where F: FnOnce(&mut S) -> R
}
```

### `Event`

Base trait for all events in the system.

```rust
pub trait Event: Send + Sync {
    type State;

    fn apply(&self, state: &mut Self::State);
    fn serialize(&self) -> Vec<u8>;
    fn deserialize(bytes: &[u8]) -> Result<Self, Error> where Self: Sized;
}
```

### `EventStore`

Manages the append-only event log.

```rust
pub struct EventStore {
    // Internal implementation
}

impl EventStore {
    pub fn new(path: &str) -> Result<Self, Error>
    pub fn append<E: Event>(&mut self, event: E) -> Result<(), Error>
    pub fn replay<E: Event>(&self) -> Result<Vec<E>, Error>
}
```

## Serialization Module

### `JsonValue`

Represents a JSON value for custom serialization.

```rust
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<JsonValue>),
    Object(HashMap<String, JsonValue>),
}
```

### JSON Functions

```rust
pub fn parse_json(input: &str) -> Result<JsonValue, JsonError>
pub fn stringify_json(value: &JsonValue) -> String
```

### `BinarySerializable`

Trait for efficient binary serialization.

```rust
pub trait BinarySerializable {
    fn to_bytes(&self) -> Vec<u8>;
    fn from_bytes(bytes: &[u8]) -> Result<Self, Error> where Self: Sized;
}
```

## Error Types

### `Error`

Main error type for the framework.

```rust
pub enum Error {
    HttpError(String),
    SerializationError(String),
    PersistenceError(String),
    EngineError(String),
}
```

### `Result<T>`

Convenience type alias.

```rust
pub type Result<T> = std::result::Result<T, Error>;
```

## Usage Patterns

### Basic Application

```rust
use lithair_core::prelude::*;
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, DeclarativeModel)]
struct Todo {
    #[db(primary_key)]
    #[http(expose)]
    id: String,
    #[http(expose)]
    title: String,
    #[http(expose)]
    completed: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // One model → REST CRUD at /api/todos, event-sourced + persisted.
    LithairServer::new()
        .with_port(3000)
        .with_model::<Todo>("./data/todos", "/api/todos")
        .serve()
        .await?;
    Ok(())
}
```

See [getting-started.md](../guides/getting-started.md) for sessions, RBAC and
the full builder walkthrough.

### Custom Event Handling

```rust
// Define custom events
struct TodoCompleted {
    todo_id: u64,
    completed_at: DateTime<Utc>,
}

impl Event for TodoCompleted {
    type State = TodoApp;

    fn apply(&self, state: &mut Self::State) {
        if let Some(todo) = state.todos.iter_mut().find(|t| t.id == self.todo_id) {
            todo.completed = true;
        }
    }
}
```

This API is designed to be simple and intuitive while providing maximum performance and type safety.

## Data-first Configuration Quick Guide

Lithair is data-first: the right configuration depends on your workload. A ticketing system (read-heavy, moderate writes) is not the same as a write‑intensive telemetry pipeline. Start here to pick sensible defaults, then refine using the benchmark suites.

- **Choose a storage profile**
  - **high_throughput**: benchmarks, bursty write/read mixes; async writer ON, binary ON, fsync OFF. Recommended `LOADGEN_CONCURRENCY=256` on 3‑node clusters.
  - **balanced**: general apps (ticketing/CRM), moderate writes with frequent reads; index & dedup ON, fsync OFF. Prefer concurrency ≤512; use light reads for SLAs.
  - **durable_security**: compliance/audit; fsync ON, binary OFF; expect higher write tails. Keep concurrency ≤512.

- **Pick the right read path**
  - **Heavy list**: `GET /api/{model}` (full JSON). Very expensive; use only to stress worst‑case.
  - **Light count**: `GET /api/{model}/count`. Recommended for perf validation.
  - **Status**: `GET /status`. Lightest endpoint; good to isolate write/consensus/persistence cost.
  - See measured results in `./http-loadgen.md` under “Heavy vs Light:
    Observations (latest)”.

- **Concurrency sweet spots**
  - On a 3‑node cluster with `STORAGE_PROFILE=high_throughput`, we observed the best throughput vs tail balance around `LOADGEN_CONCURRENCY=256`. Tails grow quickly beyond this; for `balanced`/`durable_security`, stay ≤512.

- **Key configuration knobs (ENV)**
  - `EXPERIMENT_DATA_BASE` – base data directory for examples.
  - EventStore (see bench script): `LT_OPT_PERSIST`, `LT_BUFFER_SIZE`, `LT_MAX_EVENTS_BUFFER`, `LT_FLUSH_INTERVAL_MS`, `LT_FSYNC_ON_APPEND`, `LT_ENABLE_BINARY`, `LT_DISABLE_INDEX`, `LT_DEDUP_PERSIST`.

- **Where to go next**
  - `../guides/performance.md` → current benchmark entry points and
    validation workflow.
  - `./http-loadgen.md` → CLI, best practices, recommended defaults, and
    heavy vs light observations.
  - `../../examples/09-replication/README.md` → scenario guidance and
    measured A/B results.

We welcome proposals and contributions: both to Lithair itself and to configuration recipes for specific domains.

## **Secure CRUD API Endpoints**

### Authentication & Authorization

All CRUD endpoints require JWT authentication with role-based permissions. The secure e-commerce example demonstrates a complete implementation.

#### Authentication Flow

```bash
# 1. Login to get JWT token
POST /auth/login
Content-Type: application/json

{
  "email": "admin@lithair.com",
  "password": "admin123"
}

# Response:
{
  "user_id": 1,
  "role": "Administrator",
  "permissions": ["ProductCreateAny", "ProductReadAny", "ProductUpdateAny", "ProductDeleteAny", "AdminDashboard"],
  "token": "jwt_token_admin_authenticated"
}

# 2. Use token in subsequent requests
Authorization: Bearer jwt_token_admin_authenticated
```

### Product Management Endpoints

#### `GET /api/products` - List Products

**Permission Required:** `ProductReadAny`

```bash
curl -X GET http://127.0.0.1:3002/api/products \
  -H "Authorization: Bearer jwt_token_admin_authenticated"
```

**Response:**

```json
[
  {
    "id": 0,
    "name": "Gaming Laptop",
    "description": "High-performance gaming laptop",
    "price": 1299.99,
    "category": "Electronics",
    "stock_quantity": 10,
    "image_url": null,
    "is_active": true,
    "created_by": 1,
    "created_at": 1753978454,
    "updated_at": 1753978454
  }
]
```

#### `POST /api/products` - Create Product

**Permission Required:** `ProductCreateAny`

```bash
curl -X POST http://127.0.0.1:3002/api/products \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer jwt_token_admin_authenticated" \
  -d '{
    "name": "Gaming Laptop",
    "description": "High-performance gaming laptop",
    "price": 1299.99,
    "stock_quantity": 10
  }'
```

**Response:**

```json
{
  "message": "Product created successfully and event logged!",
  "product": {
    "id": 0,
    "name": "Gaming Laptop",
    "description": "High-performance gaming laptop",
    "price": 1299.99,
    "category": "General",
    "stock_quantity": 10,
    "image_url": null,
    "is_active": true,
    "created_by": 1,
    "created_at": 1753978454,
    "updated_at": 1753978454
  },
  "event_persisted": true
}
```

#### `PUT /api/products` - Update Product

**Permission Required:** `ProductUpdateAny`

```bash
curl -X PUT http://127.0.0.1:3002/api/products \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer jwt_token_admin_authenticated" \
  -d '{
    "id": 0,
    "name": "Gaming Laptop Pro",
    "description": "Updated description",
    "price": 1499.99,
    "stock_quantity": 15
  }'
```

**Response:**

```json
{
  "message": "Product 0 updated successfully and event logged!",
  "product": {
    "id": 0,
    "name": "Gaming Laptop Pro",
    "description": "Updated description",
    "price": 1499.99,
    "category": "General",
    "stock_quantity": 15,
    "image_url": null,
    "is_active": true,
    "created_by": 1,
    "created_at": 1753978454,
    "updated_at": 1753978500
  },
  "updated_by": 1,
  "updated_at": 1753978500
}
```

#### `DELETE /api/products` - Delete Product

**Permission Required:** `ProductDeleteAny` (Administrator only)

```bash
curl -X DELETE http://127.0.0.1:3002/api/products \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer jwt_token_admin_authenticated" \
  -d '{"id": 0}'
```

**Response:**

```json
{
  "message": "Product 0 deleted successfully and event logged!",
  "id": 0,
  "deleted_by": 1,
  "deleted_at": 1753978600
}
```

### Admin Interface Endpoints

#### `GET /admin/login` - Admin Login Page

**Permission Required:** None (public)

```bash
curl -X GET http://127.0.0.1:3002/admin/login
```

Returns HTML login form for admin interface.

#### `GET /admin` - Admin Dashboard

**Permission Required:** `AdminDashboard`

```bash
curl -X GET http://127.0.0.1:3002/admin \
  -H "Authorization: Bearer jwt_token_admin_authenticated"
```

Returns HTML admin dashboard interface.

#### `GET /admin/products` - Product Management Interface

**Permission Required:** `ProductReadAny`

```bash
curl -X GET http://127.0.0.1:3002/admin/products \
  -H "Authorization: Bearer jwt_token_admin_authenticated"
```

Returns HTML interface for product CRUD operations with role-based UI adaptation.

### Role-Based Permissions

#### User Roles & Permissions Matrix

| Role              | ProductCreateAny | ProductReadAny | ProductUpdateAny | ProductDeleteAny | AdminDashboard |
| ----------------- | ---------------- | -------------- | ---------------- | ---------------- | -------------- |
| **Administrator** |                  |                |                  |                  |                |
| **Manager**       |                  |                |                  |                  |                |
| **Employee**      |                  |                |                  |                  |                |
| **Customer**      |                  |                |                  |                  |                |

#### Test Accounts

| Role          | Email                | Password    | Use Case                           |
| ------------- | -------------------- | ----------- | ---------------------------------- |
| Administrator | admin@lithair.com    | admin123    | Full CRUD access + admin dashboard |
| Manager       | manager@lithair.com  | manager123  | Create, read, update products      |
| Employee      | employee@lithair.com | employee123 | Create and read products only      |
| Customer      | customer@lithair.com | customer123 | Read-only product access           |

### Error Responses

#### Authentication Errors

```json
// 401 Unauthorized - Missing or invalid token
{
  "error": "Authentication required"
}

// 401 Unauthorized - Invalid token
{
  "error": "Invalid or expired token"
}
```

#### Authorization Errors

```json
// 403 Forbidden - Insufficient permissions
{
  "error": "Access denied. Required permission: ProductDeleteAny"
}
```

#### Validation Errors

```json
// 400 Bad Request - Invalid JSON
{
  "error": "Invalid JSON in request body"
}

// 400 Bad Request - Missing required fields
{
  "error": "Invalid or missing product ID in request body"
}

// 404 Not Found - Resource not found
{
  "error": "Product not found"
}
```

#### Server Errors

```json
// 500 Internal Server Error - State synchronization issues
{
  "error": "Failed to acquire global state lock"
}

// 500 Internal Server Error - Persistence issues
{
  "error": "Failed to persist event"
}
```

### Event Sourcing Integration

#### Event Types Generated

**ProductCreated Event:**

```json
{
  "ProductCreated": {
    "product": {
      "id": 0,
      "name": "Gaming Laptop",
      "description": "High-performance gaming laptop",
      "price": 1299.99,
      "category": "Electronics",
      "stock_quantity": 10,
      "is_active": true,
      "created_by": 1,
      "created_at": 1753978454,
      "updated_at": 1753978454
    },
    "created_by": 1,
    "timestamp": 1753978454
  }
}
```

**ProductUpdated Event:**

```json
{
  "ProductUpdated": {
    "product": {
      "id": 0,
      "name": "Gaming Laptop Pro",
      "description": "Updated description",
      "price": 1499.99,
      "category": "Electronics",
      "stock_quantity": 15,
      "is_active": true,
      "created_by": 1,
      "created_at": 1753978454,
      "updated_at": 1753978500
    },
    "updated_by": 1,
    "timestamp": 1753978500
  }
}
```

**ProductDeleted Event:**

```json
{
  "ProductDeleted": {
    "product_id": 0,
    "deleted_by": 1,
    "timestamp": 1753978600
  }
}
```

### Database Files

#### Event Log (`events.raftlog`)

Contains all CRUD events in JSON Lines format for complete audit trail and state reconstruction.

#### Metadata (`meta.raftmeta`)

Framework metadata for persistence and clustering.

#### Snapshots (`snapshot.raftsnap`)

Periodic state snapshots for performance optimization.

### Usage Examples

#### Complete CRUD Workflow

```bash
# 1. Authenticate
TOKEN=$(curl -s -X POST http://127.0.0.1:3002/auth/login \
  -H "Content-Type: application/json" \
  -d '{"email":"admin@lithair.com","password":"admin123"}' | \
  jq -r '.token')

# 2. Create product
curl -X POST http://127.0.0.1:3002/api/products \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{"name":"Test Product","description":"Test","price":99.99,"stock_quantity":5}'

# 3. List products
curl -X GET http://127.0.0.1:3002/api/products \
  -H "Authorization: Bearer $TOKEN"

# 4. Update product
curl -X PUT http://127.0.0.1:3002/api/products \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{"id":0,"name":"Updated Product","price":149.99}'

# 5. Delete product (Admin only)
curl -X DELETE http://127.0.0.1:3002/api/products \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{"id":0}'
```

#### Role-Based Access Testing

```bash
# Test Manager permissions (no delete)
MANAGER_TOKEN=$(curl -s -X POST http://127.0.0.1:3002/auth/login \
  -H "Content-Type: application/json" \
  -d '{"email":"manager@lithair.com","password":"manager123"}' | \
  jq -r '.token')

# This will succeed (Manager can create)
curl -X POST http://127.0.0.1:3002/api/products \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $MANAGER_TOKEN" \
  -d '{"name":"Manager Product","description":"Created by manager","price":199.99,"stock_quantity":3}'

# This will fail with 403 Forbidden (Manager cannot delete)
curl -X DELETE http://127.0.0.1:3002/api/products \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $MANAGER_TOKEN" \
  -d '{"id":0}'
```

### Declarative Demo – Lightweight Endpoints

These endpoints are available in the pure declarative demo server to support lightweight benchmarking and operational checks:

- `GET /status` – Service status (very light)
- `GET /api/{model}/count` – Returns item count only: `{ "count": N }`
- `GET /api/{model}/random-id` – Returns one existing id: `{ "id": "..." }`

Related benchmark flags (see `examples/09-replication/bench_1000_crud_parallel.sh`):

- `LIGHT_READS=0` → `GET /api/{model}` (heavy: full list JSON)
- `LIGHT_READS=1|true|status` → `GET /status` (very light)
- `LIGHT_READS=count` → `GET /api/{model}/count` (light)
- `PRESEED_PER_NODE=<N>` → optional pre-seed phase (100% CREATE) to populate IDs before the main workload (useful for 100% UPDATE or read-only scenarios)

These features help isolate write/consensus/persistence costs by minimizing JSON serialization overhead during reads.

---

**Lithair Secure CRUD API** - Production-ready REST endpoints with JWT authentication, role-based access control, and complete event sourcing integration.
