# Lithair Serving Modes Guide

Lithair provides three distinct serving modes optimized for different use cases: **Development**, **Production**, and **Hybrid**. Each mode balances performance, development experience, and resource usage differently.

## Overview

| Mode | Asset Serving | Memory Usage | Hot Reload | Performance | Use Case |
|------|---------------|--------------|------------|-------------|----------|
| **Dev** | Disk (real-time) | Low | Auto | Slow | Quick prototyping |
| **Prod** | Memory (SCC2) | High | No | ⚡ 40M+ ops/sec | Production |
| **Hybrid** | Memory (SCC2) | High | API reload | ⚡ 40M+ ops/sec | Active dev + max perf |

## Development Mode (`--dev`)

**Best for**: Local development with instant asset updates

### Characteristics
- **Asset Serving**: All assets served directly from disk on each request
- **Memory Usage**: Minimal - no asset caching in memory
- **Hot Reload**: Full support - changes are immediately visible
- **Performance**: Slower response times, but instant updates
- **Headers**: `X-Served-From: Disk-Dev-Mode`, `Cache-Control: no-cache`

### Example Usage
```bash
cargo run -- --port 3000 --dev
```

### Asset Flow
```
Request → Check disk → Read file → Serve with no-cache headers
```

### When to Use
- Local development
- Frequent asset changes (CSS, JS, images)
- Debugging asset loading issues
- Prototyping with quick iterations

## Production Mode (default)

**Best for**: Production deployments with maximum performance

### Characteristics
- **Asset Serving**: All assets loaded into memory at startup
- **Memory Usage**: Higher - all assets cached in memory
- **Hot Reload**: None - requires restart for asset updates
- **Performance**: Fastest response times
- **Headers**: Standard caching headers for browser optimization

### Example Usage
```bash
cargo run -- --port 3000  # Production is default
# or explicitly:
cargo run -- --port 3000 --prod
```

### Asset Flow
```
Startup → Load all assets into memory → Serve from memory cache
```

### When to Use
- Production deployments
- High-traffic scenarios
- When asset changes are infrequent
- Maximum performance requirements

## Hybrid Mode (`--hybrid`)

**Best for**: Active development with production-level performance

### Characteristics
- **Asset Serving**: In-memory via SCC2 lock-free HashMap (same as production)
- **Memory Usage**: Same as production - all assets cached in memory
- **Hot Reload**: API-triggered reload via `/admin/sites/reload` endpoint
- **Performance**: Production-level (40M+ concurrent ops/sec)
- **Headers**: Standard production caching headers

### Example Usage
```bash
# Basic hybrid mode
cargo run -- --port 3000 --hybrid

# With custom admin path
RS_ADMIN_PATH=random cargo run -- --port 3000 --hybrid
```

### Asset Flow
```
Startup → Load all assets into SCC2 memory
Request → Serve from SCC2 memory (ultra-fast)
Reload API call → Scan filesystem → Reload assets → Atomically swap → Continue serving
```

### Hot Reload Workflow

**Option 1: Development Mode (Recommended for rapid iterations)**

```bash
# 1. Start server with development reload token
RS_DEV_RELOAD_TOKEN=dev123 cargo run -- --port 3000 --hybrid

# 2. Make changes to frontend
vim frontend/src/pages/index.astro

# 3. Rebuild frontend
cd frontend && npm run build

# 4. Hot reload with simple token
curl -X POST http://localhost:3000/admin/sites/reload \
  -H "X-Reload-Token: dev123"
```

⚠️ **SECURITY WARNING**: `RS_DEV_RELOAD_TOKEN` is for **DEVELOPMENT ONLY**!
- Server displays visible warning at startup when enabled
- Never use in production environments
- Bypasses TOTP/MFA for admin login (username/password only)
- Bypasses RBAC/MFA authentication for reload endpoint

**Option 2: Production-Safe Mode (Full authentication)**

```bash
# 1. Start server normally (no dev token)
cargo run -- --port 3000 --hybrid

# 2. Make changes to frontend
vim frontend/src/pages/index.astro

# 3. Rebuild frontend
cd frontend && npm run build

# 4. Authenticate and reload (with MFA if required)
TOKEN=$(curl -X POST http://localhost:3000/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username":"admin","password":"password123","totp_code":"123456"}' \
  | jq -r '.session_token')

curl -X POST http://localhost:3000/admin/sites/reload \
  -H "Authorization: Bearer $TOKEN"
```

### When to Use
- Active frontend development with rapid iterations
- Production-like performance testing during development
- Dual-frontend development (public + admin)
- When you need both speed AND hot reload
- Alternative to restarting the entire server for asset updates

### Reload Endpoint

**Endpoint**: `POST /admin/sites/reload`
**Authentication**: Required (Admin role or session token)

**Response example**:
```json
{
  "status": "success",
  "message": "Sites reloaded successfully",
  "reloaded_at": "2025-10-21T18:30:00Z",
  "frontends": [
    { "path": "/", "assets": 729 },
    { "path": "/dashboard", "assets": 24 }
  ]
}
```

**Authentication example**:
```bash
# Get session token
TOKEN=$(curl -X POST http://localhost:3000/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username":"admin","password":"password123"}' \
  | jq -r '.session_token')

# Reload sites
curl -X POST http://localhost:3000/admin/sites/reload \
  -H "Authorization: Bearer $TOKEN"
```

**How it works**:
1. Scan filesystem for updated frontend assets
2. Load new assets into SCC2 memory engine
3. Atomically swap old assets with new ones (zero downtime)
4. Continue serving requests with new assets

## Implementation Details

### Asset Detection
Lithair automatically detects your asset directory from these candidates:
1. `public/`
2. `frontend/public/`
3. `static/`
4. `assets/`

### Memory Management
```rust
// Assets are loaded using the core virtual host system
match memserve_virtual_host_shared(
    frontend_state,
    "blog",           // Virtual host ID
    "/",              // Base path
    public_dir        // Asset directory
).await {
    Ok(count) => log::info!("✅ {} assets loaded", count),
    Err(e) => log::warn!("⚠️ Asset loading failed: {}", e),
}
```

### MIME Type Detection
All modes include automatic MIME type detection:
- **CSS files**: `text/css`
- **JavaScript files**: `application/javascript`
- **Images**: `image/png`, `image/jpeg`, etc.
- **HTML files**: `text/html; charset=utf-8`
- **Unknown files**: `application/octet-stream`

### Performance Characteristics

| Operation | Dev Mode | Prod Mode | Hybrid Mode |
|-----------|----------|-----------|-------------|
| Startup Time | Fast | Slower | Slower (loads to memory) |
| First Request | Slow | Very Fast | Very Fast |
| Subsequent Requests | Slow | Very Fast | Very Fast |
| Throughput | Low | 40M+ ops/sec | 40M+ ops/sec |
| Memory Usage | ~10MB | ~50-200MB | ~50-200MB |
| Asset Updates | Instant (auto) | Restart Required | API reload (no restart) |

## Best Practices

### Development Workflow
```bash
# Start with dev mode for active development
cargo run -- --port 3000 --dev

# Switch to hybrid for integration testing
cargo run -- --port 3000 --hybrid

# Use production mode for final testing
cargo run -- --port 3000
```

### Asset Organization
```
your-project/
├── public/              # Recommended structure
│   ├── css/
│   ├── js/
│   ├── images/
│   └── index.html
├── src/
└── Cargo.toml
```

### Configuration Examples

#### Development Team Setup
```rust
// main.rs
let dev_mode = env::var("LITHAIR_DEV").is_ok();
let server = CleanSiteServer::new(
    port,
    &data_dir,
    dev_mode,      // Enable dev mode for development
    false,         // Disable hybrid
    false,         // No admin firewall in dev
    ""
).await;
```

#### Production Deployment
```rust
// main.rs
let server = CleanSiteServer::new(
    port,
    &data_dir,
    false,         // Disable dev mode
    false,         // Disable hybrid
    true,          // Enable admin firewall
    "10.0.0.0/8,127.0.0.1"  // Restrict admin access
).await;
```

#### CI/CD Pipeline
```rust
// main.rs
let hybrid_mode = env::var("CI").is_ok();
let server = CleanSiteServer::new(
    port,
    &data_dir,
    false,         // No dev mode in CI
    hybrid_mode,   // Hybrid for CI testing
    true,          // Secure admin in CI
    "127.0.0.1"
).await;
```

## Environment Variables

### Hybrid Mode Variables

**`RS_DEV_RELOAD_TOKEN`** (Development only)
- **Purpose**: Simplified development workflow - bypasses TOTP/MFA authentication + enables hot reload
- **Usage**: `RS_DEV_RELOAD_TOKEN=dev123 cargo run -- --dev` or `--hybrid`
- **Security**: ⚠️ **NEVER use in production!** Server displays warning at startup
- **Effects**:
  - **Login**: Admin login works with username/password only (no TOTP code required)
  - **Reload**: Reload endpoint accepts `X-Reload-Token` header instead of full RBAC/MFA
  - **Development**: Eliminates need to configure authenticator app during dev

**`RS_ADMIN_PATH`**
- **Purpose**: Set custom admin panel path or generate random path
- **Usage**: `RS_ADMIN_PATH=/custom` or `RS_ADMIN_PATH=random`
- **Default**: `/admin`
- **Random**: Generates `/secure-XXXXXX` and persists to `.admin-path` file

**`RUST_LOG`**
- **Purpose**: Control logging verbosity
- **Usage**: `RUST_LOG=info` or `RUST_LOG=debug`
- **Recommended**: `info` for development, `warn` for production

### Example Configurations

**Development with hot reload**:
```bash
RS_DEV_RELOAD_TOKEN=mytoken RS_ADMIN_PATH=random RUST_LOG=info cargo run -- --hybrid
```

**Production (secure)**:
```bash
RS_ADMIN_PATH=random ./target/release/lithair-blog --port 3000
```

## Troubleshooting

### Assets Not Loading
1. **Check directory**: Ensure `public/` directory exists
2. **Verify mode**: Use `--dev` mode to bypass caching
3. **Check logs**: Look for asset loading messages at startup
4. **Test direct access**: Try accessing assets directly via URL

### Performance Issues
1. **Dev mode**: Expected in development - use `--hybrid` or production for testing
2. **Memory usage**: Monitor with `htop` - production mode uses more RAM
3. **Startup time**: Production mode takes longer to start due to asset pre-loading

### Hot Reload Not Working
1. **Dev mode**: Only works in `--dev` mode
2. **File permissions**: Ensure asset directory is readable
3. **Caching**: Clear browser cache and check `Cache-Control` headers
4. **File watching**: Some editors may not trigger filesystem events

## Advanced Configuration

### Custom Asset Directory
```rust
// If you need a custom asset directory location
let possible_dirs = vec!["custom_assets", "static", "public"];
let public_dir = possible_dirs.iter()
    .find(|dir| std::path::Path::new(dir).exists())
    .map(|s| s.to_string());
```

### Memory Optimization
```rust
// For memory-constrained environments, prefer dev mode
if available_memory < 512_000_000 {  // Less than 512MB
    log::info!("Limited memory detected, using dev mode");
    dev_mode = true;
}
```

This guide covers all three serving modes and their appropriate use cases. Choose the mode that best fits your deployment scenario and development workflow.