# Lithair Configuration Matrix

Quick reference matrix for all configuration options.

## 🎯 Legend

- ✅ = Supported
- ❌ = Not supported
- 🔄 = Hot-reloadable (no restart needed)
- 🔒 = Requires restart

---

## 📊 Complete Configuration Matrix

| Category        | Variable               | Default     | File           | Env         | Code | Hot-Reload | Notes                                                  |
| --------------- | ---------------------- | ----------- | -------------- | ----------- | ---- | ---------- | ------------------------------------------------------ |
| **SERVER**      |                        |             |                |             |      |            |                                                        |
|                 | `port`                 | `8080`      | ✅             | ✅          | ✅   | 🔒         | Listening port                                         |
|                 | `host`                 | `127.0.0.1` | ✅             | ✅          | ✅   | 🔒         | Listening address                                      |
|                 | `workers`              | `num_cpus`  | ✅             | ✅          | ✅   | 🔒         | Tokio worker threads                                   |
|                 | `cors_enabled`         | `false`     | ✅             | ✅          | ✅   | 🔄         | Enable CORS                                            |
|                 | `cors_origins`         | `["*"]`     | ✅             | ✅          | ✅   | 🔄         | Allowed origins                                        |
|                 | `request_timeout`      | `30s`       | ✅             | ✅          | ✅   | 🔄         | Request timeout                                        |
|                 | `max_body_size`        | `10MB`      | ✅             | ✅          | ✅   | 🔄         | Max request body                                       |
|                 | `tls_cert_path`        | `None`      | ✅             | ✅          | ✅   | 🔒         | TLS certificate (PEM), env: `LT_TLS_CERT`              |
|                 | `tls_key_path`         | `None`      | ✅             | ✅          | ✅   | 🔒         | TLS private key (PEM), env: `LT_TLS_KEY`               |
| **SESSIONS**    |                        |             |                |             |      |            |                                                        |
|                 | `enabled`              | `true`      | ✅             | ✅          | ✅   | 🔒         | Enable sessions                                        |
|                 | `cleanup_interval`     | `300s`      | ✅             | ✅          | ✅   | 🔄         | Cleanup interval                                       |
|                 | `max_age`              | `3600s`     | ✅             | ✅          | ✅   | 🔄         | Session lifetime                                       |
|                 | `cookie_enabled`       | `true`      | ✅             | ✅          | ✅   | 🔄         | Cookie support                                         |
|                 | `cookie_secure`        | `true`      | ✅             | ✅          | ❌   | 🔄         | Secure flag                                            |
|                 | `cookie_httponly`      | `true`      | ✅             | ✅          | ❌   | 🔄         | HttpOnly flag                                          |
|                 | `cookie_samesite`      | `Lax`       | ✅             | ✅          | ❌   | 🔄         | SameSite policy                                        |
| **RBAC**        |                        |             |                |             |      |            |                                                        |
|                 | `enabled`              | `false`     | ✅             | ✅          | ✅   | 🔒         | Enable RBAC                                            |
|                 | `default_role`         | `guest`     | ✅             | ✅          | ✅   | 🔄         | Default role                                           |
|                 | `audit_enabled`        | `true`      | ✅             | ✅          | ✅   | 🔄         | Audit trail                                            |
|                 | `rate_limit_enabled`   | `false`     | ✅             | ❌          | ✅   | 🔄         | Login rate limit (no env var)                          |
|                 | `max_login_attempts`   | `5`         | ✅             | ❌          | ❌   | 🔄         | Max login attempts (no env var)                        |
|                 | `lockout_duration`     | `300s`      | ✅             | ❌          | ❌   | 🔄         | Lockout duration (no env var)                          |
| **REPLICATION** |                        |             |                |             |      |            |                                                        |
|                 | `enabled`              | `false`     | ✅             | ✅          | ✅   | 🔒         | Enable Raft                                            |
|                 | `node_id`              | `auto`      | ✅             | ✅          | ✅   | 🔒         | Node identifier                                        |
|                 | `cluster_nodes`        | `[]`        | ✅             | ✅          | ✅   | 🔒         | Cluster nodes                                          |
|                 | `election_timeout`     | `150ms`     | ✅             | ❌          | ❌   | 🔄         | Election timeout (no env var)                          |
|                 | `heartbeat_interval`   | `50ms`      | ✅             | ❌          | ❌   | 🔄         | Heartbeat interval (no env var)                        |
|                 | `snapshot_threshold`   | `1000`      | ✅             | ❌          | ❌   | 🔄         | Snapshot threshold (no env var)                        |
|                 | `max_resync_gap`       | `1000`      | ✅             | ✅          | ❌   | 🔄         | Env: `LT_MAX_RESYNC_GAP`                               |
|                 | `max_concurrent_resyncs` | `2`       | ✅             | ✅          | ❌   | 🔄         | Env: `LT_MAX_CONCURRENT_RESYNCS`                       |
|                 | `resync_check_interval_ms` | `1000ms` | ✅            | ✅          | ❌   | 🔄         | Env: `LT_RESYNC_CHECK_INTERVAL_MS`                     |
|                 | `snapshot_send_timeout_secs` | `30s` | ✅             | ✅          | ❌   | 🔄         | Env: `LT_SNAPSHOT_SEND_TIMEOUT_SECS`                   |
|                 | `resync_cooldown_secs` | `10s`       | ✅             | ✅          | ❌   | 🔄         | Env: `LT_RESYNC_COOLDOWN_SECS`                         |
| **RAFT**        | ⚠️ legacy `LITHAIR_` prefix |        |                |             |      |            | `[raft]` endpoint section                              |
|                 | `enabled`              | `true`      | ✅             | ✅          | ❌   | 🔒         | Env: `LITHAIR_RAFT_ENABLED`                            |
|                 | `path`                 | `/raft`     | ✅             | ✅          | ❌   | 🔒         | Env: `LITHAIR_RAFT_PATH`                               |
|                 | `auth_token`           | `None`      | ✅             | ✅          | ❌   | 🔒         | Env: `LITHAIR_RAFT_TOKEN` (also sets `auth_required`)  |
|                 | `heartbeat_interval_secs` | `2s`     | ✅             | ✅          | ❌   | 🔒         | Env: `LITHAIR_RAFT_HEARTBEAT_INTERVAL`                 |
|                 | `election_timeout_secs` | `5s`       | ✅             | ✅          | ❌   | 🔒         | Env: `LITHAIR_RAFT_ELECTION_TIMEOUT`                   |
| **ADMIN**       |                        |             |                |             |      |            |                                                        |
|                 | `enabled`              | `true`      | ✅             | ✅          | ✅   | 🔄         | Enable admin panel                                     |
|                 | `path`                 | `/admin`    | ✅             | ✅          | ✅   | 🔄         | Admin panel path                                       |
|                 | `auth_required`        | `true`      | ✅             | ❌          | ✅   | 🔄         | Require auth (no env var)                              |
|                 | `metrics_enabled`      | `true`      | ✅             | ❌          | ✅   | 🔄         | Prometheus metrics (no env var)                        |
|                 | `metrics_path`         | `/metrics`  | ✅             | ❌          | ❌   | 🔄         | Metrics endpoint (no env var)                          |
| **DEVELOPMENT** | ⚠️ **DEV ONLY**        |             |                |             |      |            | env-only enforcement                                   |
|                 | `dev_reload_token`     | `None`      | 🚫 **BLOCKED** | ✅ **ONLY** | ❌   | 🔄         | Bypass TOTP/MFA + hot reload (rejected in config.toml) |
| **LOGGING**     |                        |             |                |             |      |            |                                                        |
|                 | `level`                | `info`      | ✅             | ✅          | ✅   | 🔄         | Log level (`RUST_LOG` wins over `LT_LOG_LEVEL`)        |
|                 | `format`               | `json`      | ✅             | ❌          | ✅   | 🔄         | Log format (no env var)                                |
|                 | `file_enabled`         | `false`     | ✅             | ❌          | ✅   | 🔄         | Log to file (no env var)                               |
|                 | `file_path`            | `./logs`    | ✅             | ❌          | ❌   | 🔄         | Log directory (no env var)                             |
| **STORAGE**     |                        |             |                |             |      |            |                                                        |
|                 | `data_dir`             | `./data`    | ✅             | ✅          | ✅   | 🔒         | Data directory                                         |
|                 | `snapshot_interval`    | `1000`      | ✅             | ❌          | ❌   | 🔄         | Snapshot interval (no env var)                         |
|                 | `compaction_enabled`   | `true`      | ✅             | ❌          | ❌   | 🔄         | Auto compaction (no env var)                           |
|                 | `backup_enabled`       | `false`     | ✅             | ❌          | ✅   | 🔄         | Auto backups (no env var)                              |
|                 | `schema_validation_enabled` | `true` | ✅             | ✅          | ❌   | 🔒         | Env: `LT_SCHEMA_VALIDATION`                            |
|                 | `schema_migration_mode` | `warn`     | ✅             | ✅          | ❌   | 🔒         | Env: `LT_SCHEMA_MIGRATION_MODE`                        |
| **PERFORMANCE** |                        |             |                |             |      |            |                                                        |
|                 | `cache_enabled`        | `true`      | ✅             | ✅          | ✅   | 🔄         | Memory cache                                           |
|                 | `cache_size`           | `1000`      | ✅             | ❌          | ❌   | 🔄         | Cache size (no env var)                                |
|                 | `cache_ttl`            | `300s`      | ✅             | ❌          | ❌   | 🔄         | Cache TTL (no env var)                                 |
|                 | `batch_size`           | `100`       | ✅             | ❌          | ❌   | 🔄         | Batch size (no env var)                                |
| **ENV-ONLY**    | ⚠️ env vars, no config.toml/builder |  |            |             |      |            | Full semantics: configuration-reference.md             |
|                 | `LT_ENABLE_BINARY`     | off         | ❌             | ✅ **ONLY** | ❌   | 🔒         | Binary (bincode) event log format                      |
|                 | `LT_MULTI_FILE`        | off         | ❌             | ✅ **ONLY** | ❌   | 🔒         | Multi-file event store backend                         |
|                 | `LT_OPT_PERSIST`       | off         | ❌             | ✅ **ONLY** | ❌   | 🔒         | Optimized async event writer                           |
|                 | `LT_BUFFER_SIZE`       | writer default | ❌          | ✅ **ONLY** | ❌   | 🔒         | Writer buffer size (with `LT_OPT_PERSIST`)             |
|                 | `LT_MAX_EVENTS_BUFFER` | writer default | ❌          | ✅ **ONLY** | ❌   | 🔒         | Max buffered events (with `LT_OPT_PERSIST`)            |
|                 | `LT_FLUSH_INTERVAL_MS` | `100ms`     | ❌             | ✅ **ONLY** | ❌   | 🔒         | Background flusher interval                            |
|                 | `LT_FSYNC_ON_APPEND`   | off         | ❌             | ✅ **ONLY** | ❌   | 🔒         | fsync after each append                                |
|                 | `LT_EVENT_MAX_BATCH`   | `16384`     | ❌             | ✅ **ONLY** | ❌   | 🔒         | Max event batch size                                   |
|                 | `LT_DEDUP_PERSIST`     | on          | ❌             | ✅ **ONLY** | ❌   | 🔒         | Persisted dedup (`0`/`false` disables)                 |
|                 | `LT_DISABLE_HASH_CHAIN` | chain on   | ❌             | ✅ **ONLY** | ❌   | 🔒         | Disable event hash chain                               |
|                 | `LT_DISABLE_INDEX`     | index on    | ❌             | ✅ **ONLY** | ❌   | 🔒         | Disable in-memory event index                          |
|                 | `LT_DISABLE_CONSENSUS` | off         | ❌             | ✅ **ONLY** | ❌   | 🔒         | Skip Raft consensus on bulk create                     |
|                 | `LT_FW_ENABLE`         | off         | ❌             | ✅ **ONLY** | ❌   | 🔒         | HTTP firewall master switch                            |
|                 | `LT_FW_IP_ALLOW`       | empty       | ❌             | ✅ **ONLY** | ❌   | 🔒         | Firewall allowlist (CSV IP/CIDR)                       |
|                 | `LT_FW_IP_DENY`        | empty       | ❌             | ✅ **ONLY** | ❌   | 🔒         | Firewall denylist (CSV IP/CIDR)                        |
|                 | `LT_FW_RATE_GLOBAL_QPS` | unlimited  | ❌             | ✅ **ONLY** | ❌   | 🔒         | Global QPS limit                                       |
|                 | `LT_FW_RATE_PERIP_QPS` | unlimited   | ❌             | ✅ **ONLY** | ❌   | 🔒         | Per-IP QPS limit                                       |
|                 | `LT_HTTP_ACCESS_LOG`   | off         | ❌             | ✅          | ✅   | 🔒         | Access-log ring buffer (OR'd with builder)             |
|                 | `LT_HTTP_MAX_BODY_BYTES_SINGLE` | `2MiB` | ❌         | ✅ **ONLY** | ❌   | 🔒         | Max body, single-item endpoints                        |
|                 | `LT_HTTP_MAX_BODY_BYTES_BULK` | `12MiB` | ❌           | ✅ **ONLY** | ❌   | 🔒         | Max body, bulk endpoints                               |
|                 | `RUST_LOG`             | unset       | ❌             | ✅ **ONLY** | ❌   | 🔒         | Global tracing filter, wins over `LT_LOG_LEVEL`        |
|                 | `LT_OTEL_ENDPOINT`     | unset       | ❌             | ✅ **ONLY** | ❌   | 🔒         | OTLP/gRPC export endpoint                              |
|                 | `LT_OTEL_SERVICE_NAME` | `lithair`   | ❌             | ✅ **ONLY** | ❌   | 🔒         | OTLP service name                                      |
|                 | `LT_VERBOSE`           | off         | ❌             | ✅ **ONLY** | ❌   | 🔒         | Verbose declarative-handler logging                    |
|                 | `LITHAIR_DATA_DIR`     | `./data`    | ❌             | ✅ **ONLY** | ❌   | 🔒         | Raft WAL/snapshot base dir (legacy prefix)             |
|                 | `EXPERIMENT_DATA_BASE` | -           | ❌             | ✅ **ONLY** | ❌   | 🔒         | Fallback for `LITHAIR_DATA_DIR` (BDD harness)          |

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
Code Builder > Env Vars > .env file > Config File > Defaults
```

> The `.env` file is loaded by your app (`dotenvy::dotenv()` in the `lithair new`
> scaffold), not by Lithair core. dotenvy never overwrites an existing variable,
> which is what places `.env` below real environment variables.

### Example

```bash
# 1. Default
port = 8080

# 2. config.toml
[server]
port = 3000

# 3. Environment
export LT_PORT=9000

# 4. Code (WINS)
LithairServer::new()
    .with_port(7000)  # Final: 7000
```

---

## 🔧 Environment Variable Format

Environment variable names are **literal** — they are not derived from a
`LT_<SECTION>_<OPTION>` pattern, and only the names listed in the matrix above
exist. `LT_SERVER_PORT` or `LT_LOGGING_LEVEL` are silently ignored; the real
names are `LT_PORT` and `LT_LOG_LEVEL`. When in doubt, check the Env column
above or [configuration-reference.md](./configuration-reference.md).

Most variables use the `LT_` prefix; the `[raft]` endpoint section and the Raft
storage roots keep the legacy `LITHAIR_` prefix (see the matrix).

### Array Values

Arrays in environment variables use comma-separated values:

```bash
LT_CORS_ORIGINS=https://app.com,https://admin.com
LT_CLUSTER_NODES=node-2:8081,node-3:8082
```

---

## 📝 Config File Format

Only **TOML** is supported: `LithairConfig::load()` reads `config.toml` from the
working directory (`from_file` parses TOML exclusively — there is no YAML or
JSON loader).

### TOML

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
    .with_rbac(true)
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
docker run -e LT_PORT=8080 \
           -e LT_HOST=0.0.0.0 \
           -e LT_REPLICATION_ENABLED=true \
           -e LT_CLUSTER_NODES=node-2:8081,node-3:8082 \
           -e LT_LOG_LEVEL=info \
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
# ✅ Enable security features
export LT_SESSION_COOKIE_SECURE=true
export LT_RBAC_ENABLED=true

# ✅ Restrict CORS
export LT_CORS_ORIGINS=https://app.example.com

# ✅ Enable audit trail
export LT_RBAC_AUDIT_ENABLED=true

# ✅ Enable the HTTP firewall and rate limits
export LT_FW_ENABLE=true
export LT_FW_RATE_PERIP_QPS=50

# admin.auth_required and rbac.rate_limit_enabled have no env var —
# set them in config.toml or via the builder.
```

### Development Checklist

```bash
# ✅ Relaxed CORS for local dev
export LT_CORS_ENABLED=true
export LT_CORS_ORIGINS=*

# ✅ Verbose logging (RUST_LOG also works and wins)
export LT_LOG_LEVEL=debug

# ✅ Shorter timeouts for faster feedback
export LT_SESSION_MAX_AGE=300
```

---

## 📚 See Also

- [Configuration Reference](./configuration-reference.md) - Detailed documentation
- [Getting Started](./getting-started.md) - Quick start guide
- [Deployment Guide](./deployment.md) - Production deployment
- [Security Guide](./security.md) - Security best practices
