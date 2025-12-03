# Lithair Configuration Matrix

Quick reference matrix for all configuration options.

## 🎯 Legend

- ✅ = Supported
- ❌ = Not supported
- 🔄 = Hot-reloadable (no restart needed)
- 🔒 = Requires restart

---

## 📊 Complete Configuration Matrix

| Category | Variable | Default | File | Env | Code | Hot-Reload | Notes |
|----------|----------|---------|------|-----|------|------------|-------|
| **SERVER** |
| | `port` | `8080` | ✅ | ✅ | ✅ | 🔒 | Listening port |
| | `host` | `127.0.0.1` | ✅ | ✅ | ✅ | 🔒 | Listening address |
| | `workers` | `num_cpus` | ✅ | ✅ | ✅ | 🔒 | Tokio worker threads |
| | `cors_enabled` | `false` | ✅ | ✅ | ✅ | 🔄 | Enable CORS |
| | `cors_origins` | `["*"]` | ✅ | ✅ | ✅ | 🔄 | Allowed origins |
| | `request_timeout` | `30s` | ✅ | ✅ | ✅ | 🔄 | Request timeout |
| | `max_body_size` | `10MB` | ✅ | ✅ | ✅ | 🔄 | Max request body |
| **SESSIONS** |
| | `enabled` | `true` | ✅ | ✅ | ✅ | 🔒 | Enable sessions |
| | `cleanup_interval` | `300s` | ✅ | ✅ | ✅ | 🔄 | Cleanup interval |
| | `max_age` | `3600s` | ✅ | ✅ | ✅ | 🔄 | Session lifetime |
| | `cookie_enabled` | `true` | ✅ | ✅ | ✅ | 🔄 | Cookie support |
| | `cookie_secure` | `true` | ✅ | ✅ | ❌ | 🔄 | Secure flag |
| | `cookie_httponly` | `true` | ✅ | ✅ | ❌ | 🔄 | HttpOnly flag |
| | `cookie_samesite` | `Lax` | ✅ | ✅ | ❌ | 🔄 | SameSite policy |
| **RBAC** |
| | `enabled` | `false` | ✅ | ✅ | ✅ | 🔒 | Enable RBAC |
| | `default_role` | `guest` | ✅ | ✅ | ✅ | 🔄 | Default role |
| | `audit_enabled` | `true` | ✅ | ✅ | ✅ | 🔄 | Audit trail |
| | `rate_limit_enabled` | `false` | ✅ | ✅ | ✅ | 🔄 | Login rate limit |
| | `max_login_attempts` | `5` | ✅ | ✅ | ❌ | 🔄 | Max login attempts |
| | `lockout_duration` | `300s` | ✅ | ✅ | ❌ | 🔄 | Lockout duration |
| **REPLICATION** |
| | `enabled` | `false` | ✅ | ✅ | ✅ | 🔒 | Enable Raft |
| | `node_id` | `auto` | ✅ | ✅ | ✅ | 🔒 | Node identifier |
| | `cluster_nodes` | `[]` | ✅ | ✅ | ✅ | 🔒 | Cluster nodes |
| | `election_timeout` | `150ms` | ✅ | ✅ | ❌ | 🔄 | Election timeout |
| | `heartbeat_interval` | `50ms` | ✅ | ✅ | ❌ | 🔄 | Heartbeat interval |
| | `snapshot_threshold` | `1000` | ✅ | ✅ | ❌ | 🔄 | Snapshot threshold |
| **ADMIN** |
| | `enabled` | `true` | ✅ | ✅ | ✅ | 🔄 | Enable admin panel |
| | `path` | `/admin` | ✅ | ✅ | ✅ | 🔄 | Admin panel path |
| | `auth_required` | `true` | ✅ | ✅ | ✅ | 🔄 | Require auth |
| | `metrics_enabled` | `true` | ✅ | ✅ | ✅ | 🔄 | Prometheus metrics |
| | `metrics_path` | `/metrics` | ✅ | ✅ | ❌ | 🔄 | Metrics endpoint |
| **DEVELOPMENT** ⚠️ **DEV ONLY** (env-only enforcement) |
| | `dev_reload_token` | `None` | 🚫 **BLOCKED** | ✅ **ONLY** | ❌ | 🔄 | Bypass TOTP/MFA + hot reload (rejected in config.toml) |
| **LOGGING** |
| | `level` | `info` | ✅ | ✅ | ✅ | 🔄 | Log level |
| | `format` | `json` | ✅ | ✅ | ✅ | 🔄 | Log format |
| | `file_enabled` | `false` | ✅ | ✅ | ✅ | 🔄 | Log to file |
| | `file_path` | `./logs` | ✅ | ✅ | ❌ | 🔄 | Log directory |
| | `file_rotation` | `daily` | ✅ | ✅ | ❌ | 🔄 | Rotation policy |
| | `file_max_size` | `100MB` | ✅ | ✅ | ❌ | 🔄 | Max file size |
| **STORAGE** |
| | `data_dir` | `./data` | ✅ | ✅ | ✅ | 🔒 | Data directory |
| | `snapshot_interval` | `1000` | ✅ | ✅ | ❌ | 🔄 | Snapshot interval |
| | `compaction_enabled` | `true` | ✅ | ✅ | ❌ | 🔄 | Auto compaction |
| | `compaction_threshold` | `10000` | ✅ | ✅ | ❌ | 🔄 | Compaction threshold |
| | `backup_enabled` | `false` | ✅ | ✅ | ✅ | 🔄 | Auto backups |
| | `backup_interval` | `24h` | ✅ | ✅ | ❌ | 🔄 | Backup interval |
| | `backup_path` | `./backups` | ✅ | ✅ | ❌ | 🔄 | Backup directory |
| **PERFORMANCE** |
| | `cache_enabled` | `true` | ✅ | ✅ | ✅ | 🔄 | Memory cache |
| | `cache_size` | `1000` | ✅ | ✅ | ❌ | 🔄 | Cache size |
| | `cache_ttl` | `300s` | ✅ | ✅ | ❌ | 🔄 | Cache TTL |
| | `connection_pool_size` | `10` | ✅ | ✅ | ❌ | 🔄 | Pool size |
| | `batch_size` | `100` | ✅ | ✅ | ❌ | 🔄 | Batch size |
| | `compression_enabled` | `false` | ✅ | ✅ | ❌ | 🔄 | Response compression |

---

## 🔄 Hot-Reload Categories

### Runtime Tunable (🔄)
Can be changed without restart via `/admin/config/reload`:
- Timeouts, intervals, thresholds
- Boolean flags (CORS, audit, metrics)
- Log levels and formats
- Cache and performance settings
- RBAC policies (default role, rate limits)

### Structural (🔒)
Require server restart:
- Network bindings (port, host)
- Runtime configuration (workers)
- Feature toggles (sessions, RBAC, replication enabled)
- Storage paths (data_dir)
- Cluster topology (node_id, cluster_nodes)

---

## 🎯 Priority Order (Supersedence)

```
Code Builder > Env Vars > Config File > Defaults
```

### Example

```bash
# 1. Default
port = 8080

# 2. config.toml
[server]
port = 3000

# 3. Environment
export RS_PORT=9000

# 4. Code (WINS)
LithairServer::new()
    .with_port(7000)  # Final: 7000
```

---

## 🔧 Environment Variable Format

All environment variables follow the pattern:

```
RS_<SECTION>_<OPTION>
```

### Shortcuts

Common settings have shortcuts without section prefix:

```bash
RS_PORT=8080              # Shortcut for RS_SERVER_PORT
RS_HOST=0.0.0.0           # Shortcut for RS_SERVER_HOST
RS_LOG_LEVEL=debug        # Shortcut for RS_LOGGING_LEVEL
RS_DATA_DIR=./data        # Shortcut for RS_STORAGE_DATA_DIR
```

### Array Values

Arrays in environment variables use comma-separated values:

```bash
RS_CORS_ORIGINS=https://app.com,https://admin.com
RS_CLUSTER_NODES=node-2:8081,node-3:8082
```

---

## 📝 Config File Formats

### TOML (Recommended)

```toml
[server]
port = 8080
host = "0.0.0.0"

[sessions]
enabled = true
max_age = 3600

[rbac]
enabled = true
default_role = "guest"
```

### YAML (Alternative)

```yaml
server:
  port: 8080
  host: "0.0.0.0"

sessions:
  enabled: true
  max_age: 3600

rbac:
  enabled: true
  default_role: "guest"
```

### JSON (Alternative)

```json
{
  "server": {
    "port": 8080,
    "host": "0.0.0.0"
  },
  "sessions": {
    "enabled": true,
    "max_age": 3600
  },
  "rbac": {
    "enabled": true,
    "default_role": "guest"
  }
}
```

---

## 🚀 Quick Start Examples

### Minimal (All Defaults)

```rust
LithairServer::new()
    .with_model::<Product>("./data/products.events", "/api/products")
    .serve()
    .await
```

### Development

```rust
LithairServer::new()
    .with_port(3000)
    .with_log_level("debug")
    .with_admin_panel(true)
    .with_sessions(SessionManager::new(MemorySessionStore::new()))
    .with_model::<Product>("./data/products.events", "/api/products")
    .serve()
    .await
```

### Production

```rust
LithairServer::new()
    .with_port(8080)
    .with_host("0.0.0.0")
    .with_cors(true)
    .with_sessions(SessionManager::new(MemorySessionStore::new()))
    .with_rbac(RbacConfig::from_file("rbac.toml")?)
    .with_replication(true)
    .with_admin_panel(true)
    .with_admin_auth(true)
    .with_metrics(true)
    .with_log_level("info")
    .with_log_format("json")
    .with_backup(true)
    .with_model::<Product>("./data/products.events", "/api/products")
    .with_model::<User>("./data/users.events", "/api/users")
    .with_model::<Order>("./data/orders.events", "/api/orders")
    .serve()
    .await
```

### Docker/Kubernetes

```bash
# All via environment variables
docker run -e RS_PORT=8080 \
           -e RS_HOST=0.0.0.0 \
           -e RS_REPLICATION_ENABLED=true \
           -e RS_CLUSTER_NODES=node-2:8081,node-3:8082 \
           -e RS_LOG_LEVEL=info \
           -e RS_LOG_FORMAT=json \
           myapp:latest
```

---

## 🔄 Hot-Reload API Reference

### Reload Configuration

```bash
POST /admin/config/reload
Content-Type: application/json
Authorization: Bearer <admin-token>

{
  "session_cleanup_interval": 60,
  "log_level": "debug",
  "cors_enabled": true,
  "cache_size": 2000,
  "metrics_enabled": true
}
```

### Response

```json
{
  "success": true,
  "reloaded": [
    "session_cleanup_interval",
    "log_level",
    "cors_enabled",
    "cache_size",
    "metrics_enabled"
  ],
  "requires_restart": [],
  "errors": [],
  "timestamp": "2025-10-02T14:28:00Z"
}
```

### Get Current Configuration

```bash
GET /admin/config
Authorization: Bearer <admin-token>
```

```json
{
  "server": {
    "port": 8080,
    "host": "127.0.0.1",
    "workers": 4,
    "cors_enabled": true,
    "cors_origins": ["*"],
    "request_timeout": 30,
    "max_body_size": 10485760
  },
  "sessions": {
    "enabled": true,
    "cleanup_interval": 300,
    "max_age": 3600,
    "cookie_enabled": true,
    "cookie_secure": true,
    "cookie_httponly": true
  },
  "rbac": {
    "enabled": true,
    "default_role": "guest",
    "audit_enabled": true,
    "rate_limit_enabled": false,
    "max_login_attempts": 5
  },
  "replication": {
    "enabled": false,
    "node_id": "node-1",
    "cluster_nodes": [],
    "election_timeout": 150,
    "heartbeat_interval": 50
  },
  "admin": {
    "enabled": true,
    "path": "/admin",
    "auth_required": true,
    "metrics_enabled": true
  },
  "logging": {
    "level": "info",
    "format": "json",
    "file_enabled": false
  },
  "storage": {
    "data_dir": "./data",
    "snapshot_interval": 1000,
    "compaction_enabled": true,
    "backup_enabled": false
  },
  "performance": {
    "cache_enabled": true,
    "cache_size": 1000,
    "cache_ttl": 300
  }
}
```

---

## 🎨 Configuration Validation

Lithair validates configuration at startup and provides helpful error messages:

```rust
// Invalid port
Error: Invalid configuration: port must be between 1 and 65535 (got: 70000)

// Missing required field
Error: Invalid configuration: replication.cluster_nodes is required when replication.enabled = true

// Invalid enum value
Error: Invalid configuration: logging.format must be one of: json, text, pretty (got: xml)

// Path doesn't exist
Warning: storage.data_dir does not exist, creating: ./data
```

---

## 🔐 Security Best Practices

### Production Checklist

```bash
# ✅ Use environment variables for secrets
export RS_ADMIN_PASSWORD=<strong-password>
export RS_JWT_SECRET=<random-secret>

# ✅ Enable security features
export RS_SESSION_COOKIE_SECURE=true
export RS_ADMIN_AUTH_REQUIRED=true
export RS_RBAC_ENABLED=true

# ✅ Restrict CORS
export RS_CORS_ORIGINS=https://app.example.com

# ✅ Enable audit trail
export RS_RBAC_AUDIT_ENABLED=true

# ✅ Enable rate limiting
export RS_RBAC_RATE_LIMIT=true
```

### Development Checklist

```bash
# ✅ Relaxed CORS for local dev
export RS_CORS_ENABLED=true
export RS_CORS_ORIGINS=*

# ✅ Verbose logging
export RS_LOG_LEVEL=debug
export RS_LOG_FORMAT=pretty

# ✅ Disable auth for testing
export RS_ADMIN_AUTH_REQUIRED=false

# ✅ Shorter timeouts for faster feedback
export RS_SESSION_MAX_AGE=300
```

---

## 📚 See Also

- [Configuration Reference](./configuration-reference.md) - Detailed documentation
- [Getting Started](./getting-started.md) - Quick start guide
- [Deployment Guide](./deployment.md) - Production deployment
- [Security Guide](./security.md) - Security best practices
