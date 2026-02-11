//! Minimal LithairServer example
//!
//! This example demonstrates the new unified LithairServer API
//! with configuration, sessions, and HTTP features.
//!
//! Run with:
//! ```bash
//! cargo run --example minimal_server
//! ```

use anyhow::Result;
use lithair_core::app::LithairServer;
use lithair_core::http::declarative_server::GzipConfig;
use lithair_core::logging::LoggingConfig;

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 Starting Minimal Lithair Server Example\n");

    LithairServer::new()
        // Server config
        .with_port(8080)
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
