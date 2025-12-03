//! Declarative Frontend Demo
//! 
//! This example demonstrates the declarative frontend capabilities of Lithair:
//! - Automatic asset loading from directories
//! - Memory-first serving with zero disk I/O
//! - Hot reload capabilities
//! - Fallback mechanisms

use lithair_core::frontend::{
    FrontendConfig, FrontendServer, FrontendState, 
    load_assets_from_directory_shared, StaticAsset
};
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    
    println!("🚀 Lithair Declarative Frontend Demo");
    
    // Create frontend state
    let frontend_state = Arc::new(RwLock::new(FrontendState::default()));
    
    // Demo 1: Load assets from directory
    println!("\n📦 Demo 1: Loading assets from directory");
    match load_assets_from_directory_shared(frontend_state.clone(), "public").await {
        Ok(count) => println!("✅ Loaded {} assets from public directory", count),
        Err(e) => {
            println!("⚠️ Could not load from public: {}", e);
            println!("📦 Creating demo assets in memory...");
            
            // Create demo assets manually
            let demo_assets = vec![
                ("/index.html", r#"<!DOCTYPE html>
<html><head><title>Lithair Demo</title></head>
<body><h1>🚀 Lithair Frontend</h1><p>Served from memory!</p></body></html>"#),
                ("/style.css", "body { font-family: Arial; margin: 2rem; }"),
                ("/app.js", "console.log('Lithair frontend loaded!');"),
            ];
            
            let mut state = frontend_state.write().await;
            for (path, content) in demo_assets {
                let asset = StaticAsset::new(path.to_string(), content.as_bytes().to_vec());
                println!("   📄 {} ({} bytes, {})", path, asset.size_bytes, asset.mime_type);
                state.assets.insert(asset.id, asset.clone());
                state.path_index.insert(path.to_string(), asset.id);
            }
        }
    }
    
    // Demo 2: Frontend configuration
    println!("\n⚙️ Demo 2: Frontend configuration");
    let config = FrontendConfig::enabled()
        .with_static_dir("public")
        .with_hot_reload()
        .with_max_size(5 * 1024 * 1024); // 5MB
    
    println!("✅ Frontend config: enabled={}, hot_reload={}, max_size={}MB", 
             config.enabled, config.watch_static_dir, config.max_asset_size / 1024 / 1024);
    
    // Demo 3: Create frontend server
    println!("\n🌐 Demo 3: Creating frontend server");
    let frontend_server = FrontendServer::new(frontend_state.clone());
    println!("✅ Frontend server created and ready");
    
    // Demo 4: Show loaded assets
    println!("\n📊 Demo 4: Asset inventory");
    let state = frontend_state.read().await;
    println!("Total assets in memory: {}", state.assets.len());
    for (path, asset_id) in &state.path_index {
        if let Some(asset) = state.assets.get(asset_id) {
            println!("   {} → {} bytes ({})", path, asset.size_bytes, asset.mime_type);
        }
    }
    
    println!("\n🎉 Demo completed! All assets are now in memory and ready to serve.");
    println!("💡 In a real application, these would be served with sub-millisecond latency!");
    
    Ok(())
}
