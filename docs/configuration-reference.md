# Lithair Configuration Reference

Complete reference for all configuration variables in Lithair.

## 📋 Table of Contents

- [Configuration Hierarchy](#configuration-hierarchy)
- [Server Configuration](#server-configuration)
- [Sessions Configuration](#sessions-configuration)
- [RBAC Configuration](#rbac-configuration)
- [Replication Configuration](#replication-configuration)
- [Admin Panel Configuration](#admin-panel-configuration)
- [Development Configuration](#development-configuration)
- [Logging Configuration](#logging-configuration)
- [Storage Configuration](#storage-configuration)
- [Performance Configuration](#performance-configuration)
- [Hot-Reload Support](#hot-reload-support)

---

## Configuration Hierarchy

Lithair uses a layered configuration system with the following priority (lowest to highest):

```
1. Defaults (hardcoded)
   ↓
2. Config File (config.toml)
   ↓
3. Environment Variables
   ↓
4. Code (Builder Pattern)
   ↓
5. Runtime API (Hot-reload)
```

**Example:**
```rust
// 1. Default: port = 8080
// 2. config.toml: port = 3000
// 3. ENV: RS_PORT=9000
// 4. Code (WINS):
LithairServer::new()
    .with_port(7000)  // Final value: 7000
```

---

## Server Configuration

Core HTTP server settings.

| Variable | Default | Config File | Env Var | Code Builder | Hot-Reload | Description |
|----------|---------|-------------|---------|--------------|------------|-------------|
| `port` | `8080` | ✅ | `RS_PORT` | `.with_port(u16)` | ❌ | Server listening port |
| `host` | `"127.0.0.1"` | ✅ | `RS_HOST` | `.with_host(String)` | ❌ | Server listening address |
| `workers` | `num_cpus` | ✅ | `RS_WORKERS` | `.with_workers(usize)` | ❌ | Number of Tokio worker threads |
| `cors_enabled` | `false` | ✅ | `RS_CORS_ENABLED` | `.with_cors(bool)` | ✅ | Enable CORS support |
| `cors_origins` | `["*"]` | ✅ | `RS_CORS_ORIGINS` | `.with_cors_origins(Vec<String>)` | ✅ | Allowed CORS origins (comma-separated in env) |
| `request_timeout` | `30` | ✅ | `RS_REQUEST_TIMEOUT` | `.with_timeout(u64)` | ✅ | Request timeout in seconds |
| `max_body_size` | `10485760` | ✅ | `RS_MAX_BODY_SIZE` | `.with_max_body_size(usize)` | ✅ | Maximum request body size in bytes (10MB default) |

### Example

**config.toml:**
```toml
[server]
port = 8080
host = "0.0.0.0"
workers = 4
cors_enabled = true
cors_origins = ["https://app.example.com", "https://admin.example.com"]
request_timeout = 30
max_body_size = 10485760
```

**Environment:**
```bash
RS_PORT=8080
RS_HOST=0.0.0.0
RS_WORKERS=4
RS_CORS_ENABLED=true
RS_CORS_ORIGINS=https://app.example.com,https://admin.example.com
RS_REQUEST_TIMEOUT=30
RS_MAX_BODY_SIZE=10485760
```

**Code:**
```rust
LithairServer::new()
    .with_port(8080)
    .with_host("0.0.0.0")
    .with_workers(4)
    .with_cors(true)
    .with_cors_origins(vec![
        "https://app.example.com".to_string(),
        "https://admin.example.com".to_string(),
    ])
    .with_timeout(30)
    .with_max_body_size(10 * 1024 * 1024)
```

---

## Sessions Configuration

Session management and authentication settings.

| Variable | Default | Config File | Env Var | Code Builder | Hot-Reload | Description |
|----------|---------|-------------|---------|--------------|------------|-------------|
| `enabled` | `true` | ✅ | `RS_SESSION_ENABLED` | `.with_sessions(SessionManager)` | ❌ | Enable session management |
| `cleanup_interval` | `300` | ✅ | `RS_SESSION_CLEANUP_INTERVAL` | `.with_session_cleanup(u64)` | ✅ | Cleanup interval in seconds (5 min default) |
| `max_age` | `3600` | ✅ | `RS_SESSION_MAX_AGE` | `.with_session_max_age(u64)` | ✅ | Session lifetime in seconds (1 hour default) |
| `cookie_enabled` | `true` | ✅ | `RS_SESSION_COOKIE_ENABLED` | `.with_session_cookie(bool)` | ✅ | Enable cookie-based sessions |
| `cookie_secure` | `true` | ✅ | `RS_SESSION_COOKIE_SECURE` | - | ✅ | Set Secure flag on cookies (HTTPS only) |
| `cookie_httponly` | `true` | ✅ | `RS_SESSION_COOKIE_HTTPONLY` | - | ✅ | Set HttpOnly flag on cookies (XSS protection) |
| `cookie_samesite` | `"Lax"` | ✅ | `RS_SESSION_COOKIE_SAMESITE` | - | ✅ | SameSite policy (Strict/Lax/None) |

### Example

**config.toml:**
```toml
[sessions]
enabled = true
cleanup_interval = 300
max_age = 3600
cookie_enabled = true
cookie_secure = true
cookie_httponly = true
cookie_samesite = "Lax"
```

**Environment:**
```bash
RS_SESSION_ENABLED=true
RS_SESSION_CLEANUP_INTERVAL=300
RS_SESSION_MAX_AGE=3600
RS_SESSION_COOKIE_ENABLED=true
RS_SESSION_COOKIE_SECURE=true
RS_SESSION_COOKIE_HTTPONLY=true
RS_SESSION_COOKIE_SAMESITE=Lax
```

**Code:**
```rust
use lithair_core::session::{SessionManager, SessionManagerConfig, MemorySessionStore};

let session_config = SessionManagerConfig::new()
    .with_cleanup_interval(Duration::from_secs(300))
    .with_auto_cleanup(true);

LithairServer::new()
    .with_sessions(SessionManager::with_config(
        MemorySessionStore::new(),
        session_config
    ))
```

---

## RBAC Configuration

Role-Based Access Control settings.

| Variable | Default | Config File | Env Var | Code Builder | Hot-Reload | Description |
|----------|---------|-------------|---------|--------------|------------|-------------|
| `enabled` | `false` | ✅ | `RS_RBAC_ENABLED` | `.with_rbac(RbacConfig)` | ❌ | Enable RBAC system |
| `default_role` | `"guest"` | ✅ | `RS_RBAC_DEFAULT_ROLE` | `.with_default_role(String)` | ✅ | Default role for unauthenticated users |
| `audit_enabled` | `true` | ✅ | `RS_RBAC_AUDIT_ENABLED` | `.with_audit(bool)` | ✅ | Enable audit trail for RBAC events |
| `rate_limit_enabled` | `false` | ✅ | `RS_RBAC_RATE_LIMIT` | `.with_rate_limit(bool)` | ✅ | Enable rate limiting on login attempts |
| `max_login_attempts` | `5` | ✅ | `RS_RBAC_MAX_LOGIN_ATTEMPTS` | - | ✅ | Maximum login attempts before lockout |
| `lockout_duration` | `300` | ✅ | `RS_RBAC_LOCKOUT_DURATION` | - | ✅ | Account lockout duration in seconds (5 min) |

### Example

**config.toml:**
```toml
[rbac]
enabled = true
default_role = "guest"
audit_enabled = true
rate_limit_enabled = true
max_login_attempts = 5
lockout_duration = 300
```

**Environment:**
```bash
RS_RBAC_ENABLED=true
RS_RBAC_DEFAULT_ROLE=guest
RS_RBAC_AUDIT_ENABLED=true
RS_RBAC_RATE_LIMIT=true
RS_RBAC_MAX_LOGIN_ATTEMPTS=5
RS_RBAC_LOCKOUT_DURATION=300
```

**Code:**
```rust
LithairServer::new()
    .with_rbac(RbacConfig::new()
        .with_role("customer", vec!["product:read"])
        .with_role("employee", vec!["product:read", "product:create"])
        .with_role("admin", vec!["*"])
    )
    .with_default_role("guest")
    .with_audit(true)
    .with_rate_limit(true)
```

---

## Replication Configuration

Raft consensus and cluster replication settings.

| Variable | Default | Config File | Env Var | Code Builder | Hot-Reload | Description |
|----------|---------|-------------|---------|--------------|------------|-------------|
| `enabled` | `false` | ✅ | `RS_REPLICATION_ENABLED` | `.with_replication(bool)` | ❌ | Enable Raft replication |
| `node_id` | `auto` | ✅ | `RS_NODE_ID` | `.with_node_id(String)` | ❌ | Unique node identifier |
| `cluster_nodes` | `[]` | ✅ | `RS_CLUSTER_NODES` | `.with_cluster(Vec<String>)` | ❌ | List of cluster nodes (comma-separated in env) |
| `election_timeout` | `150` | ✅ | `RS_ELECTION_TIMEOUT` | - | ✅ | Election timeout in milliseconds |
| `heartbeat_interval` | `50` | ✅ | `RS_HEARTBEAT_INTERVAL` | - | ✅ | Heartbeat interval in milliseconds |
| `snapshot_threshold` | `1000` | ✅ | `RS_SNAPSHOT_THRESHOLD` | - | ✅ | Number of log entries before snapshot |

### Example

**config.toml:**
```toml
[replication]
enabled = true
node_id = "node-1"
cluster_nodes = ["node-2:8081", "node-3:8082"]
election_timeout = 150
heartbeat_interval = 50
snapshot_threshold = 1000
```

**Environment:**
```bash
RS_REPLICATION_ENABLED=true
RS_NODE_ID=node-1
RS_CLUSTER_NODES=node-2:8081,node-3:8082
RS_ELECTION_TIMEOUT=150
RS_HEARTBEAT_INTERVAL=50
RS_SNAPSHOT_THRESHOLD=1000
```

**Code:**
```rust
LithairServer::new()
    .with_replication(true)
    .with_node_id("node-1")
    .with_cluster(vec![
        "node-2:8081".to_string(),
        "node-3:8082".to_string(),
    ])
```

---

## Admin Panel Configuration

Administrative interface and monitoring settings.

| Variable | Default | Config File | Env Var | Code Builder | Hot-Reload | Description |
|----------|---------|-------------|---------|--------------|------------|-------------|
| `enabled` | `true` | ✅ | `RS_ADMIN_ENABLED` | `.with_admin_panel(bool)` | ✅ | Enable admin panel |
| `path` | `"/admin"` | ✅ | `RS_ADMIN_PATH` | `.with_admin_path(String)` | ✅ | Admin panel base path |
| `auth_required` | `true` | ✅ | `RS_ADMIN_AUTH_REQUIRED` | `.with_admin_auth(bool)` | ✅ | Require authentication for admin panel |
| `metrics_enabled` | `true` | ✅ | `RS_ADMIN_METRICS` | `.with_metrics(bool)` | ✅ | Enable Prometheus metrics endpoint |
| `metrics_path` | `"/metrics"` | ✅ | `RS_ADMIN_METRICS_PATH` | - | ✅ | Prometheus metrics endpoint path |

---

## Development Configuration

⚠️ **DEVELOPMENT ONLY** - These settings should NEVER be used in production environments.

**Security Note**: The variables in this section are **environment-variable-only** for security reasons. They will be **rejected** if found in `config.toml` to prevent accidental git commits of secrets.

| Variable | Default | Config File | Env Var | Code Builder | Hot-Reload | Description |
|----------|---------|-------------|---------|--------------|------------|-------------|
| `dev_reload_token` | `None` | 🚫 **BLOCKED** | ✅ **REQUIRED** | - | ✅ | Development bypass token for TOTP/MFA authentication + hot reload endpoint (⚠️ **DEV ONLY**) |

### RS_DEV_RELOAD_TOKEN

**Purpose**: Simplified development workflow - bypasses TOTP/MFA authentication and enables hot reload without full RBAC.

**Security Warning**: ⚠️ **NEVER use in production!** The server displays a visible warning at startup when this token is enabled.

**Effects**:
- **Login Bypass**: Admin login works with username/password only (no TOTP code required)
- **Reload Bypass**: Reload endpoint accepts `X-Reload-Token` header instead of full RBAC/MFA authentication
- **Development Focus**: Eliminates need to configure authenticator app during development iterations

**Usage**:
```bash
# Development mode with bypass token
RS_DEV_RELOAD_TOKEN=dev123 cargo run -- --dev

# Hybrid mode with bypass token
RS_DEV_RELOAD_TOKEN=dev123 cargo run -- --hybrid

# Login without TOTP
curl -X POST http://localhost:3007/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username":"admin","password":"password123"}'

# Reload without RBAC/MFA
curl -X POST http://localhost:3007/admin/sites/reload \
  -H "X-Reload-Token: dev123"
```

**Config File Validation** 🛡️:
```toml
# ❌ THIS WILL BE REJECTED AT STARTUP!
[development]
dev_reload_token = "dev123"

# Server will fail with:
# Error: Security Error: 'dev_reload_token' must only be set via
# environment variable (RS_DEV_RELOAD_TOKEN), never in config.toml
# to prevent accidental git commits of secrets.
```

**Correct Usage - Environment Variable Only** ✅:
```bash
# Use environment variable
export RS_DEV_RELOAD_TOKEN=dev123

# Or inline
RS_DEV_RELOAD_TOKEN=dev123 cargo run -- --dev
```

### Example

**config.toml:**
```toml
[admin]
enabled = true
path = "/admin"
auth_required = true
metrics_enabled = true
metrics_path = "/metrics"
```

**Environment:**
```bash
RS_ADMIN_ENABLED=true
RS_ADMIN_PATH=/admin
RS_ADMIN_AUTH_REQUIRED=true
RS_ADMIN_METRICS=true
RS_ADMIN_METRICS_PATH=/metrics
```

**Code:**
```rust
LithairServer::new()
    .with_admin_panel(true)
    .with_admin_path("/admin")
    .with_admin_auth(true)
    .with_metrics(true)
```

---

## Logging Configuration

Application logging and observability settings.

| Variable | Default | Config File | Env Var | Code Builder | Hot-Reload | Description |
|----------|---------|-------------|---------|--------------|------------|-------------|
| `level` | `"info"` | ✅ | `RS_LOG_LEVEL` | `.with_log_level(String)` | ✅ | Log level (trace/debug/info/warn/error) |
| `format` | `"json"` | ✅ | `RS_LOG_FORMAT` | `.with_log_format(String)` | ✅ | Log format (json/text/pretty) |
| `file_enabled` | `false` | ✅ | `RS_LOG_FILE_ENABLED` | `.with_log_file(bool)` | ✅ | Enable logging to file |
| `file_path` | `"./logs"` | ✅ | `RS_LOG_FILE_PATH` | - | ✅ | Log file directory path |
| `file_rotation` | `"daily"` | ✅ | `RS_LOG_FILE_ROTATION` | - | ✅ | Log rotation policy (daily/hourly/size) |
| `file_max_size` | `100` | ✅ | `RS_LOG_FILE_MAX_SIZE` | - | ✅ | Max log file size in MB |

### Example

**config.toml:**
```toml
[logging]
level = "info"
format = "json"
file_enabled = true
file_path = "./logs"
file_rotation = "daily"
file_max_size = 100
```

**Environment:**
```bash
RS_LOG_LEVEL=info
RS_LOG_FORMAT=json
RS_LOG_FILE_ENABLED=true
RS_LOG_FILE_PATH=./logs
RS_LOG_FILE_ROTATION=daily
RS_LOG_FILE_MAX_SIZE=100
```

**Code:**
```rust
LithairServer::new()
    .with_log_level("debug")
    .with_log_format("json")
    .with_log_file(true)
```

---

## Storage Configuration

Data persistence and storage settings.

| Variable | Default | Config File | Env Var | Code Builder | Hot-Reload | Description |
|----------|---------|-------------|---------|--------------|------------|-------------|
| `data_dir` | `"./data"` | ✅ | `RS_DATA_DIR` | `.with_data_dir(String)` | ❌ | Base directory for data storage |
| `snapshot_interval` | `1000` | ✅ | `RS_SNAPSHOT_INTERVAL` | - | ✅ | Number of events before creating snapshot |
| `compaction_enabled` | `true` | ✅ | `RS_COMPACTION_ENABLED` | - | ✅ | Enable automatic log compaction |
| `compaction_threshold` | `10000` | ✅ | `RS_COMPACTION_THRESHOLD` | - | ✅ | Events threshold for compaction |
| `backup_enabled` | `false` | ✅ | `RS_BACKUP_ENABLED` | `.with_backup(bool)` | ✅ | Enable automatic backups |
| `backup_interval` | `86400` | ✅ | `RS_BACKUP_INTERVAL` | - | ✅ | Backup interval in seconds (24h default) |
| `backup_path` | `"./backups"` | ✅ | `RS_BACKUP_PATH` | - | ✅ | Backup directory path |

### Example

**config.toml:**
```toml
[storage]
data_dir = "./data"
snapshot_interval = 1000
compaction_enabled = true
compaction_threshold = 10000
backup_enabled = true
backup_interval = 86400
backup_path = "./backups"
```

**Environment:**
```bash
RS_DATA_DIR=./data
RS_SNAPSHOT_INTERVAL=1000
RS_COMPACTION_ENABLED=true
RS_COMPACTION_THRESHOLD=10000
RS_BACKUP_ENABLED=true
RS_BACKUP_INTERVAL=86400
RS_BACKUP_PATH=./backups
```

**Code:**
```rust
LithairServer::new()
    .with_data_dir("./data")
    .with_backup(true)
```

---

## Performance Configuration

Performance tuning and optimization settings.

| Variable | Default | Config File | Env Var | Code Builder | Hot-Reload | Description |
|----------|---------|-------------|---------|--------------|------------|-------------|
| `cache_enabled` | `true` | ✅ | `RS_CACHE_ENABLED` | `.with_cache(bool)` | ✅ | Enable in-memory caching |
| `cache_size` | `1000` | ✅ | `RS_CACHE_SIZE` | - | ✅ | Maximum number of cached items |
| `cache_ttl` | `300` | ✅ | `RS_CACHE_TTL` | - | ✅ | Cache TTL in seconds (5 min default) |
| `connection_pool_size` | `10` | ✅ | `RS_POOL_SIZE` | - | ✅ | Connection pool size |
| `batch_size` | `100` | ✅ | `RS_BATCH_SIZE` | - | ✅ | Default batch size for operations |
| `compression_enabled` | `false` | ✅ | `RS_COMPRESSION_ENABLED` | - | ✅ | Enable response compression |

### Example

**config.toml:**
```toml
[performance]
cache_enabled = true
cache_size = 1000
cache_ttl = 300
connection_pool_size = 10
batch_size = 100
compression_enabled = false
```

**Environment:**
```bash
RS_CACHE_ENABLED=true
RS_CACHE_SIZE=1000
RS_CACHE_TTL=300
RS_POOL_SIZE=10
RS_BATCH_SIZE=100
RS_COMPRESSION_ENABLED=false
```

**Code:**
```rust
LithairServer::new()
    .with_cache(true)
```

---

## Hot-Reload Support

### ✅ Hot-Reloadable (No Restart Required)

These settings can be changed at runtime via the admin API:

- **Server:** `cors_enabled`, `cors_origins`, `request_timeout`, `max_body_size`
- **Sessions:** `cleanup_interval`, `max_age`, `cookie_*` settings
- **RBAC:** `default_role`, `audit_enabled`, `rate_limit_enabled`, `max_login_attempts`
- **Replication:** `election_timeout`, `heartbeat_interval`, `snapshot_threshold`
- **Admin:** `enabled`, `path`, `auth_required`, `metrics_enabled`
- **Logging:** `level`, `format`, `file_*` settings
- **Storage:** `snapshot_interval`, `compaction_*`, `backup_*` settings
- **Performance:** All settings

### ❌ Requires Restart

These settings require a server restart to take effect:

- **Server:** `port`, `host`, `workers`
- **Sessions:** `enabled`
- **RBAC:** `enabled`
- **Replication:** `enabled`, `node_id`, `cluster_nodes`
- **Storage:** `data_dir`

### Hot-Reload API

```bash
# Reload specific settings
POST /admin/config/reload
Content-Type: application/json

{
  "session_cleanup_interval": 60,
  "log_level": "debug",
  "cors_enabled": true,
  "cache_size": 2000
}

# Response
{
  "reloaded": [
    "session_cleanup_interval",
    "log_level",
    "cors_enabled",
    "cache_size"
  ],
  "requires_restart": [],
  "errors": []
}
```

---

## Complete Example

**config.toml:**
```toml
[server]
port = 8080
host = "0.0.0.0"
workers = 4
cors_enabled = true
cors_origins = ["https://app.example.com"]
request_timeout = 30
max_body_size = 10485760

[sessions]
enabled = true
cleanup_interval = 300
max_age = 3600
cookie_enabled = true
cookie_secure = true
cookie_httponly = true

[rbac]
enabled = true
default_role = "guest"
audit_enabled = true
rate_limit_enabled = true
max_login_attempts = 5

[replication]
enabled = false

[admin]
enabled = true
path = "/admin"
auth_required = true
metrics_enabled = true

[logging]
level = "info"
format = "json"
file_enabled = true
file_path = "./logs"

[storage]
data_dir = "./data"
snapshot_interval = 1000
compaction_enabled = true

[performance]
cache_enabled = true
cache_size = 1000
```

**Code:**
```rust
use lithair_core::server::LithairServer;
use lithair_core::session::{SessionManager, MemorySessionStore};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    LithairServer::new()
        // Config loaded from file + env vars automatically
        
        // Override specific settings
        .with_port(8080)
        .with_sessions(SessionManager::new(MemorySessionStore::new()))
        .with_admin_panel(true)
        
        // Add models
        .with_model::<Product>("./data/products.events", "/api/products")
        .with_model::<User>("./data/users.events", "/api/users")
        
        // Start server
        .serve()
        .await
}
```

---

## See Also

- [Getting Started Guide](./getting-started.md)
- [RBAC Guide](./rbac.md)
- [Session Management](./sessions.md)
- [Replication Guide](./replication.md)
- [Admin Panel](./admin-panel.md)
