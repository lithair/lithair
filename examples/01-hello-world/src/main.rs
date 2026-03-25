//! Minimal LithairServer example
//!
//! This example demonstrates the new unified LithairServer API
//! with configuration, sessions, and HTTP features.
//!
//! Run with:
//! ```bash
//! cargo run -p hello-world
//! ```

use anyhow::Result;
use lithair_core::app::LithairServer;
use lithair_core::http::declarative_server::GzipConfig;
use lithair_core::logging::LoggingConfig;

#[tokio::main]
async fn main() -> Result<()> {
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8080);

    println!("🚀 Starting Minimal Lithair Server Example\n");

    LithairServer::new()
        // Server config
        .with_port(port)
        .with_host("127.0.0.1")
        .with_cors(true)

        // HTTP Features
        .with_logging_config(LoggingConfig::development())
        .with_gzip_config(GzipConfig {
            enabled: true,
            min_bytes: 1024,
        })

        // Admin panel
        .with_admin_panel(true)
        .with_metrics(true)

        // Start server
        .serve()
        .await?;

    Ok(())
}
