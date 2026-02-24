//! Notes App — Astro Frontend
//!
//! Same CRUD backend as 03-rest-api, served with an Astro static site.
//! Demonstrates: `npm run build` → `dist/` → `with_frontend()` → SCC2 memory.
//!
//! ## Run
//! ```bash
//! # 1. Build the frontend
//! cd examples/15-astro/frontend && npm install && npm run build
//!
//! # 2. Start the server
//! cargo run -p notes-astro
//! ```
//!
//! ## Dev (with hot reload)
//! ```bash
//! # Terminal 1: Rust backend
//! cargo run -p notes-astro
//!
//! # Terminal 2: Astro dev server (proxies /api to :8080)
//! cd examples/15-astro/frontend && npm run dev
//! ```

use anyhow::Result;
use lithair_core::app::LithairServer;
use lithair_core::logging::LoggingConfig;
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
struct Note {
    #[http(expose)]
    id: String,

    #[http(expose)]
    title: String,

    #[http(expose)]
    content: String,

    #[http(expose)]
    completed: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("Notes App (Astro)");
    println!("==================");
    println!();
    println!("API:      http://localhost:8080/api/notes");
    println!("Frontend: http://localhost:8080/");
    println!();

    LithairServer::new()
        .with_port(8080)
        .with_host("127.0.0.1")
        .with_logging_config(LoggingConfig::development())
        .with_model::<Note>("./data/notes", "/api/notes")
        .with_frontend("examples/15-astro/frontend/dist")
        .serve()
        .await?;

    Ok(())
}
