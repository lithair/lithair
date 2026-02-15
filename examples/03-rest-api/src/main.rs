//! REST API Example
//!
//! A simple Todo API built with DeclarativeModel.
//! Define a struct → get a full CRUD API automatically.
//! No auth, no persistence — just pure API.
//!
//! ## What you'll learn
//! - `#[derive(DeclarativeModel)]` generates REST endpoints
//! - `#[http(expose)]` controls which fields appear in the API
//! - Automatic GET, POST, PUT, DELETE
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
//!   -d '{"title": "Learn Lithair", "done": false}'
//!
//! # List all todos
//! curl http://localhost:8080/api/todos
//!
//! # Get one todo
//! curl http://localhost:8080/api/todos/<id>
//!
//! # Update a todo
//! curl -X PUT http://localhost:8080/api/todos/<id> \
//!   -H "Content-Type: application/json" \
//!   -d '{"title": "Learn Lithair", "done": true}'
//!
//! # Delete a todo
//! curl -X DELETE http://localhost:8080/api/todos/<id>
//! ```

use anyhow::Result;
use lithair_core::app::LithairServer;
use lithair_core::logging::LoggingConfig;
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};

/// A simple Todo item.
///
/// `DeclarativeModel` generates:
/// - GET    /api/todos       → list all
/// - POST   /api/todos       → create
/// - GET    /api/todos/:id   → get by id
/// - PUT    /api/todos/:id   → update
/// - DELETE /api/todos/:id   → delete
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
struct Todo {
    #[http(expose)]
    id: String,

    #[http(expose)]
    title: String,

    #[http(expose)]
    done: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("📝 Lithair REST API - Todo");
    println!("==========================");
    println!();
    println!("Endpoints (auto-generated from Todo struct):");
    println!("  GET    http://localhost:8080/api/todos");
    println!("  POST   http://localhost:8080/api/todos");
    println!("  GET    http://localhost:8080/api/todos/:id");
    println!("  PUT    http://localhost:8080/api/todos/:id");
    println!("  DELETE http://localhost:8080/api/todos/:id");
    println!();

    LithairServer::new()
        .with_port(8080)
        .with_host("127.0.0.1")
        .with_logging_config(LoggingConfig::development())
        // One line: struct → full CRUD API
        .with_model::<Todo>("./data/todos", "/api/todos")
        .serve()
        .await?;

    Ok(())
}
