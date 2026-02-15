//! Lithair Playground - Interactive Showcase
//!
//! Demonstrates ALL Lithair capabilities in a single interactive demo:
//! - Raft consensus with leader election and failover
//! - Live replication visualization
//! - Integrated benchmarks
//! - Cluster control (kill/restart nodes)
//! - Security features (rate limiting, firewall)
//! - RBAC and sessions
//!
//! ## Running a 3-node cluster
//!
//! ```bash
//! # Use the cluster script
//! ./run_playground.sh start
//!
//! # Or manually:
//! # Terminal 1 - Leader (node 0)
//! cargo run --bin playground_node -- --node-id 0 --port 8080 --peers 8081,8082
//!
//! # Terminal 2 - Follower 1
//! cargo run --bin playground_node -- --node-id 1 --port 8081 --peers 8080,8082
//!
//! # Terminal 3 - Follower 2
//! cargo run --bin playground_node -- --node-id 2 --port 8082 --peers 8080,8081
//! ```
//!
//! Then open http://localhost:8080 for the Playground UI

mod benchmark;
mod models;
mod playground_api;
mod sse_events;

use anyhow::Result;
use bytes::Bytes;
use clap::Parser;
use http::{Method, Response, StatusCode};
use http_body_util::Full;
use lithair_core::app::LithairServer;
use lithair_core::cluster::ClusterArgs;
use lithair_core::frontend::{FrontendEngine, FrontendServer};
use std::sync::Arc;

use crate::models::{AuditLog, Order, PlaygroundItem};
use crate::playground_api::PlaygroundState;
use crate::sse_events::SseEventBroadcaster;

fn json_response(status: StatusCode, body: serde_json::Value) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .header("Access-Control-Allow-Origin", "*")
        .body(Full::new(Bytes::from(body.to_string())))
        .unwrap()
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    let args = ClusterArgs::parse();
    let peer_ports = args.peers.clone().unwrap_or_default();
    let peers: Vec<String> = peer_ports.iter().map(|p| format!("127.0.0.1:{}", p)).collect();

    // Data directories
    let base_dir = std::env::var("PLAYGROUND_DATA_BASE").unwrap_or_else(|_| "data".to_string());
    let data_dir = format!("{}/playground_node_{}", base_dir, args.node_id);
    std::fs::create_dir_all(&data_dir)?;

    // Model data paths
    let items_path = format!("{}/items_events", data_dir);
    let orders_path = format!("{}/orders_events", data_dir);
    let logs_path = format!("{}/logs_events", data_dir);

    // SSE Event broadcaster for live updates (used by benchmark engine)
    let event_broadcaster = Arc::new(SseEventBroadcaster::new());

    // Playground state for API
    let playground_state = Arc::new(PlaygroundState::new(
        args.node_id,
        args.port,
        peer_ports.clone(),
        event_broadcaster.clone(),
    ));
    let state_status = playground_state.clone();
    let state_benchmark_start = playground_state.clone();
    let state_benchmark_status = playground_state.clone();
    let state_benchmark_stop = playground_state.clone();
    let state_migration_begin = playground_state.clone();
    let state_migration_step = playground_state.clone();
    let state_migration_commit = playground_state.clone();
    let state_migration_rollback = playground_state.clone();
    let state_migration_status = playground_state.clone();

    // Frontend engine (SCC2-based, memory-first)
    let frontend_engine = Arc::new(
        FrontendEngine::new("playground", &format!("{}/frontend", data_dir))
            .await
            .expect("Failed to create frontend engine"),
    );

    // Load frontend assets - try multiple paths
    let frontend_paths =
        ["frontend", "examples/advanced/playground/frontend", "../playground/frontend"];

    // Website public assets (includes /docs from mdBook)
    let website_paths = ["../lithair-website/public", "../../lithair-website/public"];

    let mut loaded = false;
    for path in &frontend_paths {
        if std::path::Path::new(path).exists() {
            match frontend_engine.load_directory(path).await {
                Ok(count) => {
                    log::info!("Loaded {} frontend assets from {}", count, path);
                    loaded = true;
                    break;
                }
                Err(e) => log::debug!("Could not load from {}: {}", path, e),
            }
        }
    }
    if !loaded {
        log::warn!("Could not load frontend assets - UI may not work");
    }

    // Load website assets (vitrine + docs)
    for path in &website_paths {
        if std::path::Path::new(path).exists() {
            match frontend_engine.load_directory(path).await {
                Ok(count) => {
                    log::info!("Loaded {} website assets from {}", count, path);
                    break;
                }
                Err(e) => log::debug!("Could not load website from {}: {}", path, e),
            }
        }
    }

    let frontend_server = Arc::new(FrontendServer::new_scc2(frontend_engine.clone()));

    log::info!("═══════════════════════════════════════════════════════════════════");
    log::info!("  LITHAIR PLAYGROUND - Interactive Showcase");
    log::info!("═══════════════════════════════════════════════════════════════════");
    log::info!("  Node ID:    {}", args.node_id);
    log::info!("  Port:       {}", args.port);
    log::info!("  Schema:     {}", models::get_schema_version());
    log::info!("  Peers:      {:?}", peers);
    log::info!("  Data dir:   {}", data_dir);
    log::info!("═══════════════════════════════════════════════════════════════════");
    log::info!("  Playground Endpoints:");
    log::info!("    GET    /_playground/cluster/status     - Full cluster status");
    log::info!("    POST   /_playground/benchmark/start    - Start benchmark");
    log::info!("    GET    /_playground/benchmark/status   - Benchmark status");
    log::info!("    POST   /_playground/benchmark/stop     - Stop benchmark");
    log::info!("═══════════════════════════════════════════════════════════════════");
    log::info!("  Migration Endpoints:");
    log::info!("    POST   /_playground/migration/begin    - Begin migration");
    log::info!("    POST   /_playground/migration/step     - Apply migration step");
    log::info!("    POST   /_playground/migration/commit   - Commit migration");
    log::info!("    POST   /_playground/migration/rollback - Rollback migration");
    log::info!("    GET    /_playground/migration/status   - Migration status");
    log::info!("═══════════════════════════════════════════════════════════════════");
    log::info!("  Data Endpoints:");
    log::info!("    GET    /api/items      - List items");
    log::info!("    POST   /api/items      - Create item (replicated)");
    log::info!("    GET    /api/items/:id  - Get item");
    log::info!("    PUT    /api/items/:id  - Update item (replicated)");
    log::info!("    DELETE /api/items/:id  - Delete item (replicated)");
    log::info!("    GET    /_raft/health   - Cluster health");
    log::info!("    GET    /_admin         - Admin UI");
    log::info!("═══════════════════════════════════════════════════════════════════");
    log::info!("  Open http://localhost:{} for the Playground UI", args.port);
    log::info!("═══════════════════════════════════════════════════════════════════");

    // Clone state for consistency check
    let state_consistency = playground_state.clone();

    let mut server = LithairServer::new()
        .with_port(args.port)
        // Models with automatic CRUD (3 tables for multi-table benchmark)
        .with_model::<PlaygroundItem>(&items_path, "/api/items")
        .with_model::<Order>(&orders_path, "/api/orders")
        .with_model::<AuditLog>(&logs_path, "/api/logs")
        // Admin dashboard
        .with_admin_panel(true)
        .with_data_admin()
        .with_data_admin_ui("/_admin")
        // Playground API: cluster status
        .with_route(Method::GET, "/_playground/cluster/status".to_string(), move |_req| {
            let state = state_status.clone();
            Box::pin(async move {
                let status = state.get_cluster_status().await?;
                Ok(json_response(StatusCode::OK, status))
            })
        })
        // Playground API: benchmark start
        .with_route(Method::POST, "/_playground/benchmark/start".to_string(), move |req| {
            let state = state_benchmark_start.clone();
            Box::pin(async move {
                let result = state.start_benchmark(req).await?;
                Ok(json_response(StatusCode::OK, result))
            })
        })
        // Playground API: benchmark status
        .with_route(Method::GET, "/_playground/benchmark/status".to_string(), move |_req| {
            let state = state_benchmark_status.clone();
            Box::pin(async move {
                let status = state.get_benchmark_status().await;
                Ok(json_response(StatusCode::OK, status))
            })
        })
        // Playground API: benchmark stop
        .with_route(Method::POST, "/_playground/benchmark/stop".to_string(), move |_req| {
            let state = state_benchmark_stop.clone();
            Box::pin(async move {
                state.stop_benchmark().await;
                Ok(json_response(StatusCode::OK, serde_json::json!({"stopped": true})))
            })
        })
        // Migration API: begin migration
        .with_route(Method::POST, "/_playground/migration/begin".to_string(), move |req| {
            let state = state_migration_begin.clone();
            Box::pin(async move {
                let result = state.migration_begin(req).await?;
                Ok(json_response(StatusCode::OK, result))
            })
        })
        // Migration API: apply migration step
        .with_route(Method::POST, "/_playground/migration/step".to_string(), move |req| {
            let state = state_migration_step.clone();
            Box::pin(async move {
                let result = state.migration_step(req).await?;
                Ok(json_response(StatusCode::OK, result))
            })
        })
        // Migration API: commit migration
        .with_route(Method::POST, "/_playground/migration/commit".to_string(), move |req| {
            let state = state_migration_commit.clone();
            Box::pin(async move {
                let result = state.migration_commit(req).await?;
                Ok(json_response(StatusCode::OK, result))
            })
        })
        // Migration API: rollback migration
        .with_route(Method::POST, "/_playground/migration/rollback".to_string(), move |req| {
            let state = state_migration_rollback.clone();
            Box::pin(async move {
                let result = state.migration_rollback(req).await?;
                Ok(json_response(StatusCode::OK, result))
            })
        })
        // Migration API: get status
        .with_route(Method::GET, "/_playground/migration/status".to_string(), move |_req| {
            let state = state_migration_status.clone();
            Box::pin(async move {
                let result = state.migration_status().await?;
                Ok(json_response(StatusCode::OK, result))
            })
        })
        // Playground API: consistency check (compare data across nodes)
        .with_route(Method::GET, "/_playground/consistency".to_string(), move |_req| {
            let state = state_consistency.clone();
            Box::pin(async move {
                let result = state.check_consistency().await?;
                Ok(json_response(StatusCode::OK, result))
            })
        })
        // Frontend: serve static assets from SCC2 memory
        .with_route(Method::GET, "/*".to_string(), move |req| {
            let server = frontend_server.clone();
            Box::pin(async move {
                use http_body_util::BodyExt;
                let response = server.handle_request(req).await
                    .map_err(|e| anyhow::anyhow!("{:?}", e))?;

                let (parts, body) = response.into_parts();
                let bytes = body.collect().await
                    .map_err(|_| anyhow::anyhow!("Failed to collect body"))?
                    .to_bytes();

                Ok(Response::from_parts(parts, Full::new(bytes)))
            })
        });

    // Enable Raft cluster if peers are provided
    if !peers.is_empty() {
        server = server.with_raft_cluster(args.node_id, peers);
        log::info!("Cluster mode enabled with {} peers", peer_ports.len());
    } else {
        log::info!("Single-node mode (no peers)");
    }

    server.build()?.serve().await
}
