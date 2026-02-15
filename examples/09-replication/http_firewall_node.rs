//! Minimal declarative server to demonstrate the in-process HTTP firewall

use anyhow::Result;
use chrono::{DateTime, Utc};
use clap::Parser;
use lithair_core::http::declarative_server::DeclarativeServer;
use lithair_core::http::FirewallConfig;
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Parser, Debug, Clone)]
#[command(name = "replication-firewall-node", about = "Declarative server with firewall demo")]
struct Args {
    /// Port to listen on (default: 8080)
    #[arg(long, default_value_t = 8080)]
    port: u16,

    /// Enable or disable firewall (overrides env)
    #[arg(long, action = clap::ArgAction::Set, default_value_t = true)]
    fw_enable: bool,

    /// CSV of IPs to allow (exact match). If set and non-empty, only these IPs are allowed.
    #[arg(long, default_value = "127.0.0.1")]
    fw_allow: String,

    /// CSV of IPs to deny (exact match). Takes precedence over allow.
    #[arg(long, default_value = "")]
    fw_deny: String,

    /// Global QPS limit (requests per second). Omit for no global limit.
    #[arg(long)]
    fw_global_qps: Option<u64>,

    /// Per-IP QPS limit (requests per second). Omit for no per-IP limit.
    #[arg(long)]
    fw_perip_qps: Option<u64>,

    /// Comma-separated protected URL prefixes (firewall applies only to these if non-empty)
    #[arg(long, default_value = "/api/products")]
    fw_protected_prefixes: String,

    /// Comma-separated exempt URL prefixes (firewall bypassed for these)
    #[arg(long, default_value = "/status,/health")]
    fw_exempt_prefixes: String,
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
    let allow: std::collections::HashSet<String> = args
        .fw_allow
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    let deny: std::collections::HashSet<String> = args
        .fw_deny
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();

    let fw_cfg = FirewallConfig {
        enabled: args.fw_enable,
        allow,
        deny,
        global_qps: args.fw_global_qps,
        per_ip_qps: args.fw_perip_qps,
        protected_prefixes: args
            .fw_protected_prefixes
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect(),
        exempt_prefixes: args
            .fw_exempt_prefixes
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect(),
    };

    let event_store_path = "./data/product.events";
    std::fs::create_dir_all("./data").ok();
    DeclarativeServer::<Product>::new(event_store_path, args.port)?
        .with_firewall_config(fw_cfg)
        .serve()
        .await
}
