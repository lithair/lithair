//! Notes App — Frontend Framework Integrations
//!
//! Same CRUD backend served with different frontend frameworks.
//! Demonstrates: `npm run build` -> `dist/` -> `with_frontend()` -> SCC2 memory.
//!
//! ## Run
//! ```bash
//! # 1. Build one frontend (e.g., React)
//! cd examples/11-frontend-integrations/react && npm install && npm run build
//!
//! # 2. Start the server
//! cargo run -p frontend-integrations -- --frontend react
//! ```

use anyhow::Result;
use clap::Parser;
use lithair_core::app::LithairServer;
use lithair_core::logging::LoggingConfig;
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};

#[derive(Parser, Debug)]
#[command(name = "frontend-integrations")]
struct Args {
    /// Frontend framework to serve: react, angular, vue, svelte, astro
    #[arg(long, default_value = "react")]
    frontend: String,

    /// Port to listen on
    #[arg(long, default_value_t = 8080)]
    port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
struct Note {
    #[http(expose)]
    #[lifecycle(immutable)]
    id: String,

    #[http(expose, validate = "non_empty", validate = "max_length(200)")]
    title: String,

    #[http(expose)]
    content: String,

    #[http(expose)]
    completed: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let frontend_path = match args.frontend.as_str() {
        "react" => "examples/11-frontend-integrations/react/dist",
        "angular" => "examples/11-frontend-integrations/angular/dist/angular/browser",
        "vue" => "examples/11-frontend-integrations/vue/dist",
        "svelte" => "examples/11-frontend-integrations/svelte/dist",
        "astro" => "examples/11-frontend-integrations/astro/dist",
        other => {
            eprintln!("Unknown frontend: {}. Choose: react, angular, vue, svelte, astro", other);
            std::process::exit(1);
        }
    };

    println!("Starting Notes API with {} frontend on port {}", args.frontend, args.port);

    LithairServer::new()
        .with_port(args.port)
        .with_host("127.0.0.1")
        .with_logging_config(LoggingConfig::development())
        .with_model::<Note>("./data/notes", "/api/notes")
        .with_frontend(frontend_path)
        .serve()
        .await?;

    Ok(())
}
