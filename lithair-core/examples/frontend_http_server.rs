//! Lithair Frontend HTTP Server for MCP Playwright testing

use lithair_core::frontend::{FrontendServer, FrontendState, StaticAsset};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    println!("🚀 Lithair Frontend HTTP Server");

    // Create frontend state and assets
    let frontend_state = Arc::new(RwLock::new(FrontendState::default()));
    create_demo_assets(frontend_state.clone()).await;

    let frontend_server = Arc::new(FrontendServer::new(frontend_state));

    // Start server
    let addr = SocketAddr::from(([127, 0, 0, 1], 3005));
    let listener = TcpListener::bind(addr).await?;

    println!("🌐 Server running on http://{}", addr);
    println!("✅ Ready for MCP Playwright testing!");

    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let frontend_server = frontend_server.clone();

        tokio::task::spawn(async move {
            let _ = http1::Builder::new()
                .serve_connection(io, service_fn(move |req| {
                    let frontend_server = frontend_server.clone();
                    async move { frontend_server.handle_request(req).await }
                }))
                .await;
        });
    }
}

async fn create_demo_assets(state: Arc<RwLock<FrontendState>>) {
    let assets = vec![
        ("/index.html", r#"<!DOCTYPE html>
<html><head><title>Lithair Demo</title></head>
<body><h1>🚀 Lithair Frontend</h1><p>Served from memory!</p></body></html>"#),
        ("/style.css", "body { font-family: Arial; margin: 20px; }"),
        ("/app.js", "console.log('Lithair frontend loaded!');"),
    ];

    let mut state_guard = state.write().await;

    // Create default virtual host
    let host_id = "main".to_string();
    let location = state_guard.virtual_hosts.entry(host_id.clone()).or_insert_with(|| {
        lithair_core::frontend::VirtualHostLocation {
            host_id: host_id,
            base_path: "/".to_string(),
            assets: std::collections::HashMap::new(),
            path_index: std::collections::HashMap::new(),
            static_root: "/tmp".to_string(),
            active: true,
        }
    });

    for (path, content) in assets {
        let asset = StaticAsset::new(path.to_string(), content.as_bytes().to_vec());
        location.assets.insert(asset.id, asset.clone());
        location.path_index.insert(path.to_string(), asset.id);
    }
    println!("📦 Assets loaded into memory");
}
