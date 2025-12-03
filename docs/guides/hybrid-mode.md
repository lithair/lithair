# Lithair Hybrid Mode

## Overview

Lithair supports three serving modes for optimal performance and developer experience:

| Mode | Assets Storage | Hot Reload | Performance | Use Case |
|------|---------------|------------|-------------|----------|
| **Production** | In-memory (SCC2) | ❌ No | ⚡ 40M+ ops/sec | Production deployments |
| **Hybrid** | In-memory (SCC2) | ✅ Via API | ⚡ 40M+ ops/sec | Development with max performance |
| **Dev** | Disk (filesystem) | ✅ Auto | 🐢 Slower | Quick prototyping |

## Hybrid Mode - Best of Both Worlds

**Hybrid mode** combines production-level performance with development-friendly hot reload capabilities.

### Features

- ✅ **In-Memory Assets**: Uses SCC2 lock-free HashMap for maximum throughput (40M+ concurrent ops/sec)
- ✅ **Hot Reload API**: Reload assets via `/admin/sites/reload` endpoint without server restart
- ✅ **Zero Downtime**: Reload frontends while server continues serving requests
- ✅ **Secure Reload**: Reload endpoint protected by RBAC authentication
- ✅ **Dual Frontend Support**: Works seamlessly with multiple frontends (public + admin)

### Usage

#### Starting the Server

```bash
# Basic hybrid mode
cargo run -- --port 3007 --data-dir ./blog_data --hybrid

# With custom admin path
RS_ADMIN_PATH=random cargo run -- --port 3007 --data-dir ./blog_data --hybrid

# Production build with hybrid mode
RS_ADMIN_PATH=/secure-admin ./target/release/lithair-blog --port 3007 --data-dir ./blog_data --hybrid
```

#### Development Workflow

**Option 1: Simplified Development Mode (Recommended)**

```bash
# 1. Start server with development reload token
RS_DEV_RELOAD_TOKEN=dev123 RS_ADMIN_PATH=random cargo run -- --hybrid

# 2. Make changes to your frontend
vim frontend/src/pages/index.astro

# 3. Rebuild frontend
cd frontend && npm run build

# 4. Hot reload with simple token!
curl -X POST http://localhost:3007/admin/sites/reload \
  -H "X-Reload-Token: dev123"
```

⚠️ **SECURITY WARNING**: `RS_DEV_RELOAD_TOKEN` is for **DEVELOPMENT ONLY**!
- The server will display a visible warning at startup when dev token is enabled
- Never use this in production environments
- Bypasses TOTP/MFA for admin login (username/password only)
- Bypasses RBAC/MFA authentication for the reload endpoint

**Option 2: Production-Safe Mode (Full Authentication)**

```bash
# 1. Start server normally (no dev token)
RS_ADMIN_PATH=random cargo run -- --hybrid

# 2. Authenticate and get session token (with MFA if required)
TOKEN=$(curl -X POST http://localhost:3007/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username":"admin","password":"password123","totp_code":"123456"}' \
  | jq -r '.session_token')

# 3. Hot reload with full authentication
curl -X POST http://localhost:3007/admin/sites/reload \
  -H "Authorization: Bearer $TOKEN"

# Or use the admin UI
# Navigate to: http://localhost:3007/<admin-path>/ and use the reload button
```

#### Reload Endpoint

**Endpoint:** `POST /admin/sites/reload`

**Authentication:** Required (Admin role or session token)

**Response:**
```json
{
  "status": "success",
  "message": "Sites reloaded successfully",
  "reloaded_at": "2025-10-21T18:30:00Z",
  "frontends": [
    { "path": "/", "assets": 729 },
    { "path": "/secure-xy3xir", "assets": 24 }
  ]
}
```

**Example with curl:**
```bash
# Get session token first
TOKEN=$(curl -X POST http://localhost:3007/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username":"admin","password":"password123"}' \
  | jq -r '.session_token')

# Reload sites
curl -X POST http://localhost:3007/admin/sites/reload \
  -H "Authorization: Bearer $TOKEN"
```

## Mode Comparison

### Production Mode (Default)

```bash
./target/release/lithair-blog --port 3007
```

- **Performance**: Maximum (40M+ ops/sec)
- **Memory**: All assets loaded at startup
- **Reload**: Requires server restart
- **Best for**: Production deployments, high-traffic sites

### Hybrid Mode (Recommended for Development)

```bash
cargo run -- --port 3007 --hybrid
```

- **Performance**: Maximum (40M+ ops/sec)
- **Memory**: All assets in SCC2 engine
- **Reload**: Via API endpoint (hot reload)
- **Best for**: Active development, frontend iteration

### Dev Mode

```bash
cargo run -- --port 3007 --dev
```

- **Performance**: Lower (disk I/O overhead)
- **Memory**: Minimal (reads from disk)
- **Reload**: Automatic on file change
- **Best for**: Quick prototyping, testing

## Architecture Details

### How Hybrid Mode Works

1. **Startup**: Assets loaded into SCC2 in-memory engine (same as production)
2. **Serving**: Ultra-fast lock-free serving from memory
3. **Reload Trigger**: POST to `/admin/sites/reload`
4. **Reload Process**:
   - Scan filesystem for updated assets
   - Load new assets into SCC2 engine
   - Atomically swap old assets with new ones
   - Zero downtime during reload
5. **Continue**: Server keeps serving with new assets

### Dual-Frontend Example

```rust
// main.rs
let server = LithairServer::new()
    .with_port(3007)

    // Public frontend at "/"
    .with_frontend_at("/", "public")

    // Admin frontend at dynamic path
    .with_frontend_at(&admin_path, "admin-public");
```

When you reload via `/admin/sites/reload`, both frontends are reloaded atomically.

## Tips & Best Practices

### 1. Use Hybrid Mode During Development

```bash
# Start once
RS_ADMIN_PATH=random cargo run -- --hybrid

# Iterate freely
cd frontend && npm run build && curl -X POST localhost:3007/admin/sites/reload
```

### 2. Switch to Production for Deployment

```bash
# Production: No hot reload, maximum security
./target/release/lithair-blog --port 3007
```

### 3. Automate Reload in Your Build Script

```json
// package.json
{
  "scripts": {
    "build": "astro build",
    "reload": "curl -X POST http://localhost:3007/admin/sites/reload",
    "dev": "npm run build && npm run reload"
  }
}
```

### 4. Use Admin UI for Visual Reload

Navigate to your admin panel and look for the "Reload Sites" button (protected by authentication).

## Environment Variables

- `RS_ADMIN_PATH`: Custom admin path or "random" for generated path
- `RUST_LOG`: Set to `info` or `debug` for detailed reload logging

```bash
RS_ADMIN_PATH=random RUST_LOG=info cargo run -- --hybrid
```

## Troubleshooting

### Reload Returns 401 Unauthorized

Make sure you're authenticated. The reload endpoint requires admin privileges.

```bash
# Login first, get token, then reload
TOKEN=$(curl -s -X POST http://localhost:3007/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username":"admin","password":"password123"}' \
  | jq -r '.session_token')

curl -X POST http://localhost:3007/admin/sites/reload \
  -H "Authorization: Bearer $TOKEN"
```

### Changes Not Appearing After Reload

1. Verify frontend build completed: `cd frontend && npm run build`
2. Check build output directory matches server config
3. Check server logs for reload confirmation
4. Try hard refresh in browser (Ctrl+F5)

### Performance Degradation

If you notice performance issues in hybrid mode:
1. Check asset count (too many files?)
2. Monitor memory usage
3. Consider switching to production mode for benchmarking

## Conclusion

**Hybrid mode** is the recommended mode for active development on Lithair applications. It provides:

- Production-level performance
- Developer-friendly hot reload
- Zero-downtime asset updates
- Secure reload endpoint

Use it during development, switch to production mode for deployment!
