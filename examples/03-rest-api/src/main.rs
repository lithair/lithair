//! REST API Example
//!
//! A simple Todo API built with DeclarativeModel.
//! Define a struct → get a full CRUD API automatically.
//!
//! ## What you'll learn
//! - `#[derive(DeclarativeModel)]` generates REST endpoints
//! - `#[http(expose)]` controls which fields appear in the API
//! - `#[http(validate = "...")]` generates compile-time validation
//! - `#[lifecycle(immutable)]` prevents field modification after creation
//! - `#[db(unique)]` enforces uniqueness constraints
//! - Automatic GET, HEAD, POST, PUT, PATCH, DELETE, OPTIONS
//!
//! ## Run
//! ```bash
//! cargo run -p rest-api
//! ```
//!
//! ## Test
//! ```bash
//! # Create a todo
//! curl -X POST http://localhost:8080/api/todos \
//!   -H "Content-Type: application/json" \
//!   -d '{"id": "1", "title": "Learn Lithair", "done": false}'
//!
//! # List all todos
//! curl http://localhost:8080/api/todos
//!
//! # Get one todo
//! curl http://localhost:8080/api/todos/1
//!
//! # Partial update (PATCH)
//! curl -X PATCH http://localhost:8080/api/todos/1 \
//!   -H "Content-Type: application/json" \
//!   -d '{"done": true}'
//!
//! # Full update (PUT)
//! curl -X PUT http://localhost:8080/api/todos/1 \
//!   -H "Content-Type: application/json" \
//!   -d '{"id": "1", "title": "Learn Lithair", "done": true}'
//!
//! # Model schema introspection
//! curl http://localhost:8080/api/todos/_schema
//!
//! # Delete a todo
//! curl -X DELETE http://localhost:8080/api/todos/1
//!
//! # Validation: empty title is rejected
//! curl -X POST http://localhost:8080/api/todos \
//!   -H "Content-Type: application/json" \
//!   -d '{"id": "2", "title": "", "done": false}'
//! # → 400 Bad Request: 'title' must not be empty
//!
//! # Uniqueness: duplicate id is rejected
//! curl -X POST http://localhost:8080/api/todos \
//!   -H "Content-Type: application/json" \
//!   -d '{"id": "1", "title": "Duplicate", "done": false}'
//! # → 409 Conflict: 'id' must be unique
//! ```

use anyhow::Result;
use lithair_core::app::LithairServer;
use lithair_core::logging::LoggingConfig;
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};

/// A simple Todo item.
///
/// `DeclarativeModel` auto-generates from this struct:
/// - GET    /api/todos         → list all (with filters, pagination)
/// - HEAD   /api/todos         → same headers, no body
/// - POST   /api/todos         → create (validates + unique check)
/// - GET    /api/todos/:id     → get by id
/// - PUT    /api/todos/:id     → full update (immutability enforced)
/// - PATCH  /api/todos/:id     → partial update (JSON merge)
/// - DELETE /api/todos/:id     → delete
/// - GET    /api/todos/_schema → model introspection
/// - GET    /api/todos/count   → item count
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
struct Todo {
    /// Primary key — immutable after creation, must be unique
    #[http(expose)]
    #[lifecycle(immutable)]
    #[db(unique)]
    id: String,

    /// Title — cannot be empty, max 200 characters
    #[http(expose, validate = "non_empty", validate = "max_length(200)")]
    #[lifecycle(audited)]
    title: String,

    /// Completion status
    #[http(expose)]
    done: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8080);

    LithairServer::new()
        .with_port(port)
        .with_host("127.0.0.1")
        .with_logging_config(LoggingConfig::development())
        // One line: struct → full CRUD API with validation, uniqueness, and audit
        .with_model::<Todo>("./data/todos", "/api/todos")
        .serve()
        .await?;

    Ok(())
}
