//! Static Site Example
//!
//! Serve static files from memory using SCC2.
//! This is the simplest real-world use case: a personal website,
//! a documentation site, or a landing page.
//!
//! ## What you'll learn
//! - Serving static files from SCC2 (in-memory, lock-free)
//! - Zero disk I/O at runtime
//! - Gzip compression built-in
//!
//! ## Run
//! ```bash
//! cargo run -p static-site
//! # Open http://localhost:8080
//! ```

use anyhow::Result;
use lithair_core::app::LithairServer;
use lithair_core::http::declarative_server::GzipConfig;
use lithair_core::logging::LoggingConfig;

#[tokio::main]
async fn main() -> Result<()> {
    println!("🌐 Lithair Static Site");
    println!("======================");
    println!("Serving ./public from memory (SCC2)");
    println!();
    println!("  http://localhost:8080");
    println!();

    LithairServer::new()
        .with_port(8080)
        .with_host("127.0.0.1")
        .with_logging_config(LoggingConfig::development())
        .with_gzip_config(GzipConfig { enabled: true, min_bytes: 512 })
        .with_frontend("examples/02-static-site/public")
        .serve()
        .await?;

    Ok(())
}
