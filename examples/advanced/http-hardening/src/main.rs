/// Minimal full declarative HTTP server to exercise HTTP hardening features
use anyhow::Result;
use chrono::{DateTime, Utc};
use clap::Parser;
use lithair_core::app::LithairServer;
use lithair_core::http::FirewallConfig;
use lithair_core::security::anti_ddos::AntiDDoSConfig;
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::Duration;
use uuid::Uuid;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "http-hardening-node",
    about = "Minimal declarative server for HTTP hardening demo"
)]
struct Args {
    /// Port to listen on (default: 8080)
    #[arg(long, default_value_t = 8080)]
    port: u16,
    /// Open demo: disable production firewall posture (default: off)
    #[arg(long, default_value_t = false)]
    open: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
pub struct Product {
    #[db(primary_key, indexed)]
    #[lifecycle(immutable)]
    #[http(expose)]
    #[serde(default = "generate_uuid")]
    pub id: Uuid,

    #[http(expose)]
    pub name: String,

    #[http(expose)]
    pub price: f64,

    #[http(expose)]
    #[serde(default = "chrono::Utc::now")]
    pub created_at: DateTime<Utc>,
}

fn generate_uuid() -> Uuid {
    Uuid::new_v4()
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    // Build the LithairServer explicitly to attach firewall and anti-DDoS config.
    // EventStore path mimics serve_on_port() default: ./data/{model}.events
    std::fs::create_dir_all("./data").ok();
    let event_store_path = "./data/product.events";

    let mut server = LithairServer::new()
        .with_port(args.port)
        .with_model::<Product>(event_store_path, "/api/products");

    // Determine open mode: compile-time feature can default to open, CLI can override
    let open_mode = cfg!(feature = "open_by_default") || args.open;

    // Default to production-like firewall posture; allow disabling with --open or enabling via feature
    if !open_mode {
        let mut allow = HashSet::new();
        // Macro: internal → private IPv4 ranges + IPv6 ULA
        allow.insert("internal".to_string());
        // Example additional exact IP or CIDR you may want to include:
        // allow.insert("10.0.0.10".to_string());
        // allow.insert("192.168.0.0/16".to_string());
        let fw = FirewallConfig {
            enabled: true,
            allow,
            deny: HashSet::new(),
            global_qps: Some(1000),
            per_ip_qps: Some(50),
            protected_prefixes: vec!["/perf".into(), "/metrics".into()],
            exempt_prefixes: vec!["/status".into()],
        };
        server = server.with_firewall_config(fw);
    }

    // Configure anti-DDoS protection if enabled via environment variable
    let enable_anti_ddos = std::env::var("LT_ANTI_DDOS")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false);

    if enable_anti_ddos {
        let max_connections = std::env::var("LT_MAX_CONNECTIONS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1000);
        let rate_limit =
            std::env::var("LT_RATE_LIMIT").ok().and_then(|v| v.parse().ok()).unwrap_or(100);

        let anti_ddos_config = AntiDDoSConfig {
            max_connections_per_ip: max_connections / 10, // 10% per IP
            max_global_connections: max_connections,
            rate_limit_per_ip: rate_limit,
            rate_window_seconds: 60,
            connection_timeout: Duration::from_secs(300),
            circuit_breaker_threshold: 50,
        };

        log::info!("Anti-DDoS protection enabled");
        log::info!("Max connections: {}, Rate limit: {} req/min", max_connections, rate_limit);
        log::debug!("Anti-DDoS config: {:?}", anti_ddos_config);

        // Pass the anti-DDoS config to the server
        server = server.with_anti_ddos_config(anti_ddos_config);
    }

    // Log startup information
    log::info!("Starting HTTP Hardening Node");
    log::info!("Port: {}, Mode: {}", args.port, if args.open { "open" } else { "secure" });
    log::debug!("Event store path: {}", event_store_path);

    if !open_mode {
        log::warn!("Firewall enabled - production mode");
        log::info!("Protected endpoints: /perf, /metrics");
        log::info!("Exempt endpoints: /status");
    } else {
        log::warn!("Firewall disabled - development mode (--open flag)");
    }

    // Start the server
    let result = server.serve().await;

    // Log shutdown
    if let Err(ref e) = result {
        log::error!("Server error: {}", e);
    } else {
        log::info!("Server shutdown gracefully");
    }

    result
}
