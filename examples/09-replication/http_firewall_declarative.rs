//! Fully declarative HTTP firewall example: the firewall is configured on the model
//! via a struct-level attribute, with no CLI flags controlling firewall behavior.

use anyhow::Result;
use clap::Parser;
use lithair_core::http::declarative_server::DeclarativeServer;
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Parser, Debug, Clone)]
#[command(name = "http_firewall_declarative", about = "Model-level declarative firewall demo")]
struct Args {
    /// Port to listen on (default: 8081)
    #[arg(long, default_value_t = 8081)]
    port: u16,
}

fn generate_uuid() -> Uuid {
    Uuid::new_v4()
}

#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
#[firewall(
    enabled = true,
    allow = "127.0.0.1",
    protected = "/api/products",
    exempt = "/status,/health",
    global_qps = 3,
    per_ip_qps = 2
)]
pub struct Product {
    #[db(primary_key, indexed)]
    #[lifecycle(immutable)]
    #[http(expose)]
    #[serde(default = "generate_uuid")]
    pub id: Uuid,

    #[http(expose)]
    pub name: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Minimal: no firewall flags anywhere; server will read the config from the model
    let port = args.port;
    let event_store_path = "./data/products_declarative.events";

    // Start the server
    DeclarativeServer::<Product>::new(event_store_path, port)?.serve().await
}
