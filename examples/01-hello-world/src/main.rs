//! Minimal LithairServer example
//!
//! This example demonstrates the new unified LithairServer API
//! with configuration, sessions, and HTTP features.
//!
//! Run with:
//! ```bash
//! cargo run -p hello-world
//! ```
//!
//! Binding is configurable via the `HOST` and `PORT` env vars. Defaults to
//! `127.0.0.1:8080` to preserve local-run behavior; set `HOST=0.0.0.0` to
//! bind all interfaces (e.g. inside a container).

use anyhow::Result;
use lithair_core::app::LithairServer;

#[tokio::main]
async fn main() -> Result<()> {
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8080);
    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string());

    println!("🚀 Starting Minimal Lithair Server Example\n");

    LithairServer::new()
        // Server config
        .with_port(port)
        .with_host(host)

        // Admin panel
        .with_admin_panel(true)
        .with_metrics(true)

        // Start server
        .serve()
        .await?;

    Ok(())
}
