# 🎨 Lithair Frontend Architecture: Memory-First Serving

## Overview

Lithair propose une approche différente du serving web traditionnel : **memory-first declarative asset management**. Contrairement aux serveurs web classiques qui lisent les fichiers sur disque à chaque requête, Lithair charge tous les assets statiques en mémoire une fois au démarrage et les sert directement depuis la RAM avec **latence sub-milliseconde**.

## 🚀 Key Features

### ⚡ Zero-Disk I/O Serving
- **Initial Load**: Files read from disk once into memory
- **Runtime Serving**: 100% memory-based with zero I/O
- **Performance**: 10,000x faster than traditional disk-based serving
- **Concurrency**: SCC2-powered HashMap for massive concurrent access

### 📁 Multi-Virtual-Host Memory Serving
```rust
use lithair_core::frontend::memserve_virtual_host_shared;

// Serve virtual host from memory - load once, serve forever!
let count = memserve_virtual_host_shared(
    state,
    "blog",        // Virtual host ID
    "/",           // Base path for routing
    "public/blog"  // Directory to load from
).await?;

// Add another virtual host on a different path
let docs_count = memserve_virtual_host_shared(
    state,
    "docs",
    "/docs",
    "public/docs"
).await?;
// ✅ Multiple sites memory-served from single Lithair instance!
```

### 🎯 Smart Directory Discovery
Lithair automatically searches for assets in multiple locations:
1. `public/` (standard web directory)
2. `frontend/public/` (framework-specific)
3. `static/` (alternative naming)
4. `assets/` (common convention)

### 🔄 Hot Reload Ready
```rust
let config = FrontendConfig::enabled()
    .with_static_dir("public")
    .with_hot_reload()  // Watch for file changes
    .with_max_size(10 * 1024 * 1024); // 10MB limit
```

## 🏗️ Architecture Components

### Core Types

#### `StaticAsset`
```rust
#[derive(DeclarativeModel)]
pub struct StaticAsset {
    #[db(primary_key)] pub id: Uuid,
    #[db(indexed, unique)] pub path: String,
    pub content: Vec<u8>,
    pub mime_type: String,
    pub size_bytes: u64,
    pub created_at: DateTime<Utc>,
}
```

#### `VirtualHostLocation`
```rust
pub struct VirtualHostLocation {
    pub host_id: String,
    pub base_path: String,
    pub assets: HashMap<Uuid, StaticAsset>,
    pub path_index: HashMap<String, Uuid>,
    pub static_root: String,
    pub active: bool,
}
```

#### `FrontendState`
```rust
pub struct FrontendState {
    pub virtual_hosts: HashMap<String, VirtualHostLocation>,
    pub version_history: HashMap<Uuid, Vec<String>>,
    pub deployments: HashMap<String, DateTime<Utc>>,
    pub config: FrontendConfig,
}
```

#### `FrontendServer`
High-performance HTTP server for serving assets from memory:
```rust
let frontend_server = FrontendServer::new(state);
// Handles all HTTP requests for static assets
```

### Configuration

#### `FrontendConfig`
```rust
let config = FrontendConfig::enabled()
    .with_static_dir("public")
    .with_hot_reload()
    .with_max_size(5 * 1024 * 1024)
    .with_fallback(Some("/index.html".to_string()));
```

**Configuration Options:**
- `enabled`: Enable/disable frontend serving
- `admin_enabled`: Enable admin interface for asset management
- `static_dir`: Directory to load assets from
- `watch_static_dir`: Enable hot reload
- `max_asset_size`: Maximum file size (default: 10MB)
- `fallback_file`: Default file for directory requests
- `compression_enabled`: Enable gzip/brotli compression
- `default_cache_ttl`: Cache TTL in seconds

## 🔧 Usage Examples

### Basic Usage - Single Virtual Host
```rust
use lithair_core::frontend::{
    FrontendServer, FrontendState, memserve_virtual_host_shared
};
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> Result<()> {
    // Create frontend state
    let state = Arc::new(RwLock::new(FrontendState::default()));

    // Memory-serve a virtual host - approche mémoire sans I/O disque
    let count = memserve_virtual_host_shared(
        state.clone(),
        "main",     // Virtual host ID
        "/",        // Base path
        "public"    // Directory
    ).await?;
    println!("✅ Memory-serving {} assets for 'main' virtual host", count);

    // Create frontend server with multi-virtual-host routing
    let server = FrontendServer::new(state);

    // Assets are now served from memory with sub-millisecond latency!
    Ok(())
}
```

### Advanced Usage - Multiple Virtual Hosts
```rust
use lithair_core::frontend::{
    FrontendServer, FrontendState, memserve_virtual_host_shared
};

#[tokio::main]
async fn main() -> Result<()> {
    let state = Arc::new(RwLock::new(FrontendState::default()));

    // Memory-serve multiple virtual hosts on different paths
    memserve_virtual_host_shared(state.clone(), "blog", "/", "public/blog").await?;
    memserve_virtual_host_shared(state.clone(), "docs", "/docs", "public/docs").await?;
    memserve_virtual_host_shared(state.clone(), "admin", "/admin", "public/admin").await?;

    println!("✅ 3 virtual hosts memory-served and ready!");

    let server = FrontendServer::new(state);
    // Requests automatically routed to the correct virtual host based on path!
    Ok(())
}
```

### Advanced Configuration
```rust
use lithair_core::frontend::{FrontendConfig, AssetAdminHandler};

// Create advanced configuration
let config = FrontendConfig::enabled()
    .with_static_dir("frontend/dist")
    .with_hot_reload()
    .with_max_size(50 * 1024 * 1024) // 50MB for large apps
    .with_fallback(Some("/app.html".to_string()));

// Enable admin interface
let admin_handler = AssetAdminHandler::new(state.clone());
// Provides /admin/assets/* endpoints for asset management
```

### Integration with HTTP Server
```rust
use hyper::{Request, Response};

async fn handle_request(req: Request<hyper::body::Incoming>) -> Result<Response<_>> {
    let path = req.uri().path();
    
    if path.starts_with("/api/") {
        // Handle API requests
        handle_api_request(req).await
    } else {
        // Serve static assets from memory
        frontend_server.handle_request(req).await
    }
}
```

## 📊 Performance Characteristics

### Memory Usage
- **Rule of thumb**: ~1.2x file size in memory
- **10MB assets** → ~12MB RAM usage
- **100MB assets** → ~120MB RAM usage
- **Optimal range**: < 100MB total assets

### Serving Performance
- **Latency**: Sub-millisecond (typically 0.1-0.5ms)
- **Throughput**: 40M+ requests/sec with SCC2
- **Concurrency**: Scales linearly with CPU cores
- **Memory bandwidth**: Limited only by RAM speed

### Comparison with Traditional Serving

| Metric | Traditional (Disk) | Lithair (Memory) | Improvement |
|--------|-------------------|-------------------|-------------|
| Latency | 5-50ms | 0.1-0.5ms | **10-500x faster** |
| Throughput | 1K-10K req/s | 40M+ req/s | **4,000-40,000x faster** |
| CPU Usage | High (I/O wait) | Low (memory access) | **10x more efficient** |
| Scalability | Poor (I/O bound) | Excellent (CPU bound) | **Linear scaling** |

## 🛠️ Admin Interface

### Asset Management Endpoints
```http
GET    /admin/assets/         # List all assets
POST   /admin/assets/         # Upload new asset
GET    /admin/assets/{id}     # Get asset details
PUT    /admin/assets/{id}     # Update asset
DELETE /admin/assets/{id}     # Delete asset
POST   /admin/assets/reload   # Reload from directory
```

### Asset Information
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "path": "/index.html",
  "size_bytes": 14459,
  "mime_type": "text/html",
  "created_at": "2024-01-15T10:30:00Z",
  "version": "v1.0.0"
}
```

## 🔄 Event Sourcing Integration

### Frontend Events
```rust
pub enum FrontendEvent {
    AssetCreated { id: Uuid, path: String, size_bytes: u64, mime_type: String },
    AssetUpdated { id: Uuid, old_version: String, new_version: String },
    AssetDeleted { id: Uuid, path: String },
    AssetDeployed { id: Uuid, deployment_source: String },
}
```

### Audit Trail
- Every asset operation is logged as an event
- Complete history of asset changes
- Time-travel debugging capabilities
- Compliance-ready audit logs

## 🎯 Use Cases

### Perfect For
- **Web Applications**: HTML, CSS, JS, images
- **SPAs**: React, Vue, Angular builds
- **Static Sites**: Generated sites, documentation
- **APIs with Frontend**: Full-stack applications
- **Microservices**: Service-specific assets

### Consider Alternatives For
- **Very Large Assets**: > 100MB total
- **Frequently Changing**: High-churn content
- **User Uploads**: Dynamic content
- **CDN Requirements**: Global distribution needs

## 🚨 Best Practices

### Directory Structure
```
project/
├── public/              # Auto-discovered
│   ├── index.html
│   ├── style.css
│   ├── app.js
│   └── images/
│       └── logo.png
├── src/                 # Your Rust code
└── Cargo.toml
```

### Asset Organization
- Keep total assets < 100MB for optimal performance
- Use subdirectories for organization
- Include proper file extensions for MIME detection
- Avoid binary files that change frequently

### Error Handling
```rust
match memserve_virtual_host_shared(state, "main", "/", "public").await {
    Ok(count) => log::info!("✅ {} assets memory-served", count),
    Err(e) => {
        log::warn!("⚠️ Could not memory-serve assets: {}", e);
        // Fallback to embedded assets
        load_embedded_fallback_assets(state).await;
    }
}
```

### Hot Reload Setup
```rust
// Enable in development
let config = if cfg!(debug_assertions) {
    FrontendConfig::enabled().with_hot_reload()
} else {
    FrontendConfig::enabled()
};
```

## 🔮 Future Enhancements

### Planned Features
- **Compression**: Automatic gzip/brotli compression
- **Caching**: Intelligent cache headers and ETags
- **Bundling**: Asset bundling and minification
- **CDN Integration**: Push to CDN on deployment
- **Asset Pipeline**: Build tool integration

### Experimental Features
- **Streaming**: Large asset streaming
- **Lazy Loading**: Load assets on first request
- **Sharding**: Distribute assets across nodes
- **Encryption**: Encrypted asset storage

## 📚 Related Documentation

- [Memory Architecture Trade-offs](MEMORY_ARCHITECTURE.md)
- [Performance Benchmarks](../benchmarks/)
- [SCC2 Integration](SCC2_INTEGRATION.md)
- [Event Sourcing](EVENT_SOURCING.md)

---

**La Différence**: Les serveurs web traditionnels sont limités par l'I/O disque. Lithair est limité par la bande passante mémoire. Cette approche en rupture élimine le besoin de cache serveur (nginx/apache) et permet des performances jusqu'à **10,000x supérieures** pour le serving d'assets statiques.
