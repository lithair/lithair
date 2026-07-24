# Lithair Configuration Reference

Complete reference for all configuration variables in Lithair.

## Table of Contents

- [Configuration Hierarchy](#configuration-hierarchy)
- [Server Configuration](#server-configuration)
- [Sessions Configuration](#sessions-configuration)
- [RBAC Configuration](#rbac-configuration)
- [Replication Configuration](#replication-configuration)
- [Raft Endpoint Configuration](#raft-endpoint-configuration)
- [Admin Panel Configuration](#admin-panel-configuration)
- [Development Configuration](#development-configuration)
- [Logging Configuration](#logging-configuration)
- [Storage Configuration](#storage-configuration)
- [Performance Configuration](#performance-configuration)
- [Frontend Configuration](#frontend-configuration)
- [Environment-Only Variables](#environment-only-variables)
- [Hot-Reload Support](#hot-reload-support)

---

## Configuration Hierarchy

Lithair uses a layered configuration system with the following priority (lowest to highest):

```
1. Defaults (hardcoded)
   ↓
2. Config File (config.toml)
   ↓
3. .env file (loaded by your app — see below)
   ↓
4. Environment Variables
   ↓
5. Code (Builder Pattern)
   ↓
6. Runtime API (Hot-reload)
```

**Example:**

```rust
// 1. Default: port = 8080
// 2. config.toml: port = 3000
// 3. ENV: LT_PORT=9000
// 4. Code (WINS):
LithairServer::new()
    .with_port(7000)  // Final value: 7000
```

### The `.env` layer

Lithair core never reads `.env` itself. Applications scaffolded with `lithair new`
call `dotenvy::dotenv().ok()` at the top of `main()`, which loads `.env` into the
process environment *before* `LithairServer::new()` resolves configuration.
dotenvy never overwrites a variable that is already set, so a real environment
variable always wins over the same name in `.env`.

### Resolution notes

- A missing `config.toml` is not an error — defaults apply.
- An existing but unparseable `config.toml` makes `LithairConfig::load()` /
  `load_from()` return an error. `LithairServer::new()` catches that error,
  continues on defaults plus environment variables, and logs a warning once
  the log bridge is installed at `serve()`.
- Not every variable participates in the full hierarchy: some are read directly
  from the environment and cannot be set via `config.toml`. See
  [Environment-Only Variables](#environment-only-variables).

---

## Server Configuration

Core HTTP server settings.

| Variable          | Default       | Config File | Env Var              | Code Builder                      | Hot-Reload | Description                                       |
| ----------------- | ------------- | ----------- | -------------------- | --------------------------------- | ---------- | ------------------------------------------------- |
| `port`            | `8080`        |             | `LT_PORT`            | `.with_port(u16)`                 |            | Server listening port                             |
| `host`            | `"127.0.0.1"` |             | `LT_HOST`            | `.with_host(String)`              |            | Server listening address                          |
| `workers`         | `num_cpus`    |             | `LT_WORKERS`         | `.with_workers(usize)`            |            | Number of Tokio worker threads                    |
| `cors_enabled`    | `false`       |             | `LT_CORS_ENABLED`    | `.with_cors(bool)`                |            | Enable CORS support                               |
| `cors_origins`    | `["*"]`       |             | `LT_CORS_ORIGINS`    | `.with_cors_origins(Vec<String>)` |            | Allowed CORS origins (comma-separated in env)     |
| `request_timeout` | `30`          |             | `LT_REQUEST_TIMEOUT` | `.with_timeout(u64)`              |            | Request timeout in seconds                        |
| `max_body_size`   | `10485760`    |             | `LT_MAX_BODY_SIZE`   | `.with_max_body_size(usize)`      |            | Maximum request body size in bytes (10MB default) |
| `tls_cert_path`   | `None`        |             | `LT_TLS_CERT`        | `.with_tls(cert, key)`            |            | Path to the TLS certificate (PEM)                 |
| `tls_key_path`    | `None`        |             | `LT_TLS_KEY`         | `.with_tls(cert, key)`            |            | Path to the TLS private key (PEM)                 |

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
LT_PORT=8080
LT_HOST=0.0.0.0
LT_WORKERS=4
LT_CORS_ENABLED=true
LT_CORS_ORIGINS=https://app.example.com,https://admin.example.com
LT_REQUEST_TIMEOUT=30
LT_MAX_BODY_SIZE=10485760
```

The pre-1.0 legacy aliases `LT_COLT_ENABLED`/`LT_COLT_ORIGINS` are no longer accepted — use `LT_CORS_*`.

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

| Variable           | Default | Config File | Env Var                       | Code Builder                     | Hot-Reload | Description                                   |
| ------------------ | ------- | ----------- | ----------------------------- | -------------------------------- | ---------- | --------------------------------------------- |
| `enabled`          | `true`  |             | `LT_SESSION_ENABLED`          | `.with_sessions(SessionManager)` |            | Enable session management                     |
| `cleanup_interval` | `300`   |             | `LT_SESSION_CLEANUP_INTERVAL` | `.with_session_cleanup(u64)`     |            | Cleanup interval in seconds (5 min default)   |
| `max_age`          | `3600`  |             | `LT_SESSION_MAX_AGE`          | `.with_session_max_age(u64)`     |            | Session lifetime in seconds (1 hour default)  |
| `cookie_enabled`   | `true`  |             | `LT_SESSION_COOKIE_ENABLED`   | `.with_session_cookie(bool)`     |            | Enable cookie-based sessions                  |
| `cookie_secure`    | `true`  |             | `LT_SESSION_COOKIE_SECURE`    | -                                |            | Set Secure flag on cookies (HTTPS only)       |
| `cookie_httponly`  | `true`  |             | `LT_SESSION_COOKIE_HTTPONLY`  | -                                |            | Set HttpOnly flag on cookies (XSS protection) |
| `cookie_samesite`  | `"Lax"` |             | `LT_SESSION_COOKIE_SAMESITE`  | -                                |            | SameSite policy (Strict/Lax/None)             |

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
LT_SESSION_ENABLED=true
LT_SESSION_CLEANUP_INTERVAL=300
LT_SESSION_MAX_AGE=3600
LT_SESSION_COOKIE_ENABLED=true
LT_SESSION_COOKIE_SECURE=true
LT_SESSION_COOKIE_HTTPONLY=true
LT_SESSION_COOKIE_SAMESITE=Lax
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

| Variable             | Default   | Config File | Env Var                      | Code Builder                 | Hot-Reload | Description                                 |
| -------------------- | --------- | ----------- | ---------------------------- | ---------------------------- | ---------- | ------------------------------------------- |
| `enabled`            | `false`   |             | `LT_RBAC_ENABLED`            | `.with_rbac(RbacConfig)`     |            | Enable RBAC system                          |
| `default_role`       | `"guest"` |             | `LT_RBAC_DEFAULT_ROLE`       | `.with_default_role(String)` |            | Default role for unauthenticated users      |
| `audit_enabled`      | `true`    |             | `LT_RBAC_AUDIT_ENABLED`      | `.with_audit(bool)`          |            | Enable audit trail for RBAC events          |
| `rate_limit_enabled` | `false`   |             | -                            | `.with_rate_limit(bool)`     |            | Enable rate limiting on login attempts (no env var)      |
| `max_login_attempts` | `5`       |             | -                            | -                            |            | Maximum login attempts before lockout (no env var)       |
| `lockout_duration`   | `300`     |             | -                            | -                            |            | Account lockout duration in seconds (5 min) (no env var) |

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
LT_RBAC_ENABLED=true
LT_RBAC_DEFAULT_ROLE=guest
LT_RBAC_AUDIT_ENABLED=true
# rate_limit_enabled, max_login_attempts and lockout_duration have no
# environment variable — set them in config.toml or code.
```

**Code:**

```rust
LithairServer::new()
    .with_rbac(true)
    .with_default_role("guest")
    .with_audit(true)
    .with_rate_limit(true)
```

---

## Replication Configuration

Raft consensus and cluster replication settings.

| Variable             | Default | Config File | Env Var                  | Code Builder                 | Hot-Reload | Description                                    |
| -------------------- | ------- | ----------- | ------------------------ | ---------------------------- | ---------- | ---------------------------------------------- |
| `enabled`                    | `false` |             | `LT_REPLICATION_ENABLED`        | `.with_replication(bool)`    |            | Enable Raft replication                              |
| `node_id`                    | `auto`  |             | `LT_NODE_ID`                    | `.with_node_id(String)`      |            | Unique node identifier                               |
| `cluster_nodes`              | `[]`    |             | `LT_CLUSTER_NODES`              | `.with_cluster(Vec<String>)` |            | List of cluster nodes (comma-separated in env)       |
| `election_timeout`           | `150`   |             | -                               | -                            |            | Election timeout in milliseconds (no env var)        |
| `heartbeat_interval`         | `50`    |             | -                               | -                            |            | Heartbeat interval in milliseconds (no env var)      |
| `snapshot_threshold`         | `1000`  |             | -                               | -                            |            | Number of log entries before snapshot (no env var)   |
| `max_resync_gap`             | `1000`  |             | `LT_MAX_RESYNC_GAP`             | -                            |            | Max index gap before forcing a snapshot resync       |
| `max_concurrent_resyncs`     | `2`     |             | `LT_MAX_CONCURRENT_RESYNCS`     | -                            |            | Maximum concurrent snapshot resyncs                  |
| `resync_check_interval_ms`   | `1000`  |             | `LT_RESYNC_CHECK_INTERVAL_MS`   | -                            |            | Resync check interval in milliseconds                |
| `snapshot_send_timeout_secs` | `30`    |             | `LT_SNAPSHOT_SEND_TIMEOUT_SECS` | -                            |            | Snapshot send timeout in seconds                     |
| `resync_cooldown_secs`       | `10`    |             | `LT_RESYNC_COOLDOWN_SECS`       | -                            |            | Minimum seconds between resyncs of the same follower |

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
LT_REPLICATION_ENABLED=true
LT_NODE_ID=node-1
LT_CLUSTER_NODES=node-2:8081,node-3:8082
# election_timeout, heartbeat_interval and snapshot_threshold have no
# environment variable — set them in config.toml.
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

## Raft Endpoint Configuration

The `[raft]` section configures the Raft HTTP endpoint and its consensus timers.
It is separate from `[replication]` above. Each variable reads its `LT_RAFT_*`
name first and falls back to the legacy pre-migration `LITHAIR_RAFT_*` alias
(both stay accepted throughout 1.x; `LT_RAFT_*` wins when both are set).

| Variable                  | Default    | Config File | Env Var                           | Code Builder | Hot-Reload | Description                                      |
| ------------------------- | ---------- | ----------- | --------------------------------- | ------------ | ---------- | ------------------------------------------------ |
| `enabled`                 | `true`     |             | `LT_RAFT_ENABLED`                 | -            |            | Enable the Raft HTTP endpoint                    |
| `path`                    | `"/raft"`  |             | `LT_RAFT_PATH`                    | -            |            | Raft endpoint base path                          |
| `auth_required`           | `false`    |             | `LT_RAFT_TOKEN` (see note)        | -            |            | Require authentication on the Raft endpoint      |
| `auth_token`              | `None`     |             | `LT_RAFT_TOKEN`                   | -            |            | Shared token; setting it enables `auth_required` |
| `heartbeat_interval_secs` | `2`        |             | `LT_RAFT_HEARTBEAT_INTERVAL`      | -            |            | Heartbeat interval in **seconds**                |
| `election_timeout_secs`   | `5`        |             | `LT_RAFT_ELECTION_TIMEOUT`        | -            |            | Election timeout in **seconds**                  |

---

## Admin Panel Configuration

Administrative interface and monitoring settings.

| Variable          | Default      | Config File | Env Var                  | Code Builder               | Hot-Reload | Description                            |
| ----------------- | ------------ | ----------- | ------------------------ | -------------------------- | ---------- | -------------------------------------- |
| `enabled`         | `true`       |             | `LT_ADMIN_ENABLED`       | `.with_admin_panel(bool)`  |            | Enable admin panel                     |
| `path`            | `"/admin"`   |             | `LT_ADMIN_PATH`          | `.with_admin_path(String)` |            | Admin panel base path                  |
| `auth_required`   | `true`       |             | -                        | `.with_admin_auth(bool)`   |            | Require authentication for admin panel (no env var) |
| `metrics_enabled` | `true`       |             | -                        | `.with_metrics(bool)`      |            | Enable Prometheus metrics endpoint (no env var)     |
| `metrics_path`    | `"/metrics"` |             | -                        | -                          |            | Prometheus metrics endpoint path (no env var)       |

---

## Development Configuration

**DEVELOPMENT ONLY** - These settings should NEVER be used in production environments.

**Security Note**: The variables in this section are **environment-variable-only** for security reasons. They will be **rejected** if found in `config.toml` to prevent accidental git commits of secrets.

| Variable           | Default | Config File | Env Var      | Code Builder | Hot-Reload | Description                                                                                |
| ------------------ | ------- | ----------- | ------------ | ------------ | ---------- | ------------------------------------------------------------------------------------------ |
| `dev_reload_token` | `None`  | **BLOCKED** | **REQUIRED** | -            |            | Development bypass token for TOTP/MFA authentication + hot reload endpoint ( **DEV ONLY**) |

### LT_DEV_RELOAD_TOKEN

**Purpose**: Simplified development workflow - bypasses TOTP/MFA authentication and enables hot reload without full RBAC.

**Security Warning**: **NEVER use in production!** The server displays a visible warning at startup when this token is enabled.

**Effects**:

- **Login Bypass**: Admin login works with username/password only (no TOTP code required)
- **Reload Bypass**: Reload endpoint accepts `X-Reload-Token` header instead of full RBAC/MFA authentication
- **Development Focus**: Eliminates need to configure authenticator app during development iterations

**Usage**:

```bash
# Development mode with bypass token
LT_DEV_RELOAD_TOKEN=dev123 cargo run -- --dev

# Hybrid mode with bypass token
LT_DEV_RELOAD_TOKEN=dev123 cargo run -- --hybrid

# Login without TOTP
curl -X POST http://localhost:3007/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username":"admin","password":"password123"}'

# Reload without RBAC/MFA
curl -X POST http://localhost:3007/admin/sites/reload \
  -H "X-Reload-Token: dev123"
```

**Config File Validation** :

```toml
#  THIS WILL BE REJECTED AT STARTUP!
[development]
dev_reload_token = "dev123"

# Server will fail with:
# Error: Security Error: 'dev_reload_token' must only be set via
# environment variable (LT_DEV_RELOAD_TOKEN), never in config.toml
# to prevent accidental git commits of secrets.
```

**Correct Usage - Environment Variable Only** :

```bash
# Use environment variable
export LT_DEV_RELOAD_TOKEN=dev123

# Or inline
LT_DEV_RELOAD_TOKEN=dev123 cargo run -- --dev
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
LT_ADMIN_ENABLED=true
LT_ADMIN_PATH=/admin
# auth_required, metrics_enabled and metrics_path have no environment
# variable — set them in config.toml or code.
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

| Variable        | Default    | Config File | Env Var                | Code Builder               | Hot-Reload | Description                             |
| --------------- | ---------- | ----------- | ---------------------- | -------------------------- | ---------- | --------------------------------------- |
| `level`        | `"info"`   |             | `LT_LOG_LEVEL` | `.with_log_level(String)`  |            | Log level (trace/debug/info/warn/error) |
| `format`       | `"json"`   |             | -              | `.with_log_format(String)` |            | Log format (json/text/pretty) (no env var) |
| `file_enabled` | `false`    |             | -              | `.with_log_file(bool)`     |            | Enable logging to file (no env var)     |
| `file_path`    | `"./logs"` |             | -              | -                          |            | Log file directory path (no env var)    |

> **`RUST_LOG` precedence:** the global log filter honors `RUST_LOG` (full
> tracing directive syntax) first. `LT_LOG_LEVEL` — the simple knob advertised
> in the scaffold's `.env` — is only used when `RUST_LOG` is unset.

### Example

**config.toml:**

```toml
[logging]
level = "info"
format = "json"
file_enabled = true
file_path = "./logs"
```

**Environment:**

```bash
LT_LOG_LEVEL=info
# format, file_enabled and file_path have no environment variable —
# set them in config.toml or code.
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

| Variable               | Default       | Config File | Env Var                   | Code Builder             | Hot-Reload | Description                               |
| ---------------------- | ------------- | ----------- | ------------------------- | ------------------------ | ---------- | ----------------------------------------- |
| `data_dir`                  | `"./data"` |             | `LT_DATA_DIR`             | `.with_data_dir(String)` |            | Base directory for data storage                     |
| `snapshot_interval`         | `1000`     |             | -                         | -                        |            | Number of events before creating snapshot (no env var) |
| `compaction_enabled`        | `true`     |             | -                         | -                        |            | Enable automatic log compaction (no env var)        |
| `backup_enabled`            | `false`    |             | -                         | `.with_backup(bool)`     |            | Enable automatic backups (no env var)               |
| `schema_validation_enabled` | `true`     |             | `LT_SCHEMA_VALIDATION`    | -                        |            | Validate stored schema against models at startup    |
| `schema_migration_mode`     | `warn`     |             | `LT_SCHEMA_MIGRATION_MODE` | -                       |            | Schema migration behavior: `warn`, `strict` or `auto` |

### Example

**config.toml:**

```toml
[storage]
data_dir = "./data"
snapshot_interval = 1000
compaction_enabled = true
backup_enabled = true
schema_validation_enabled = true
```

**Environment:**

```bash
LT_DATA_DIR=./data
LT_SCHEMA_VALIDATION=true
LT_SCHEMA_MIGRATION_MODE=warn
# snapshot_interval, compaction_enabled and backup_enabled have no
# environment variable — set them in config.toml or code.
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

| Variable               | Default | Config File | Env Var                  | Code Builder        | Hot-Reload | Description                          |
| ---------------------- | ------- | ----------- | ------------------------ | ------------------- | ---------- | ------------------------------------ |
| `cache_enabled` | `true` |             | `LT_CACHE_ENABLED` | `.with_cache(bool)` |            | Enable in-memory caching                          |
| `cache_size`    | `1000` |             | -                  | -                   |            | Maximum number of cached items (no env var)       |
| `cache_ttl`     | `300`  |             | -                  | -                   |            | Cache TTL in seconds (5 min default) (no env var) |
| `batch_size`    | `100`  |             | -                  | -                   |            | Default batch size for operations (no env var)    |

### Example

**config.toml:**

```toml
[performance]
cache_enabled = true
cache_size = 1000
cache_ttl = 300
batch_size = 100
```

**Environment:**

```bash
LT_CACHE_ENABLED=true
# cache_size, cache_ttl and batch_size have no environment variable —
# set them in config.toml.
```

**Code:**

```rust
LithairServer::new()
    .with_cache(true)
```

---

## Environment-Only Variables

These variables are read **directly from the environment** and cannot be set
via `config.toml`. Most of them bypass the builder too; the two exceptions —
`LT_MULTI_FILE` and `LT_HTTP_ACCESS_LOG` — are OR-combined with the equivalent
config/builder flag (either side can enable the feature). Because a process
cannot receive new environment variables after it starts, they all require a
restart to change. Boolean flags accept `1` or `true` (any case) unless noted
otherwise.

### Engine & persistence

| Variable                | Default          | Description                                                                  |
| ----------------------- | ---------------- | ---------------------------------------------------------------------------- |
| `LT_ENABLE_BINARY`      | off              | Binary (bincode) event log format instead of JSON lines                      |
| `LT_MULTI_FILE`         | off              | Multi-file event store backend (OR'd with the config flag)                   |
| `LT_OPT_PERSIST`        | off              | Optimized async event writer path                                            |
| `LT_BUFFER_SIZE`        | writer default   | Write buffer size in bytes (only used with `LT_OPT_PERSIST`)                 |
| `LT_MAX_EVENTS_BUFFER`  | writer default   | Max buffered events before forced flush (only used with `LT_OPT_PERSIST`)    |
| `LT_FLUSH_INTERVAL_MS`  | `100`            | Background batch flusher interval in milliseconds                            |
| `LT_FSYNC_ON_APPEND`    | off              | fsync after each event append                                                |
| `LT_EVENT_MAX_BATCH`    | `16384`          | Max batch size for event store batching                                      |
| `LT_DEDUP_PERSIST`      | on               | Persisted dedup state — set `0`/`false` to disable                           |
| `LT_DISABLE_HASH_CHAIN` | chain on         | Set to disable the tamper-evident event hash chain                           |
| `LT_DISABLE_INDEX`      | index on         | Set to disable in-memory event indexing                                      |
| `LT_DISABLE_CONSENSUS`  | off              | Skip Raft consensus during bulk create (local-only writes)                   |

### HTTP & firewall

| Variable                        | Default   | Description                                                  |
| ------------------------------- | --------- | ------------------------------------------------------------ |
| `LT_FW_ENABLE`                  | off       | HTTP firewall master switch                                  |
| `LT_FW_IP_ALLOW`                | empty     | Firewall allowlist (comma-separated IPs/CIDRs)               |
| `LT_FW_IP_DENY`                 | empty     | Firewall denylist (comma-separated IPs/CIDRs)                |
| `LT_FW_RATE_GLOBAL_QPS`         | unlimited | Global queries-per-second rate limit                         |
| `LT_FW_RATE_PERIP_QPS`          | unlimited | Per-IP queries-per-second rate limit                         |
| `LT_HTTP_ACCESS_LOG`            | off       | In-memory HTTP access-log ring buffer (OR'd with builder flag) |
| `LT_HTTP_MAX_BODY_BYTES_SINGLE` | 2 MiB     | Max request body in bytes for single-item endpoints          |
| `LT_HTTP_MAX_BODY_BYTES_BULK`   | 12 MiB    | Max request body in bytes for bulk endpoints                 |

### Observability

| Variable               | Default     | Description                                                       |
| ---------------------- | ----------- | ------------------------------------------------------------------ |
| `RUST_LOG`             | unset       | Global tracing filter (full directive syntax); wins over `LT_LOG_LEVEL` |
| `LT_OTEL_ENDPOINT`     | unset       | OTLP/gRPC trace export endpoint — presence enables export         |
| `LT_OTEL_SERVICE_NAME` | `"lithair"` | Service name reported to the OTLP collector                       |
| `LT_VERBOSE`           | off         | Verbose logging in the declarative HTTP handler                   |

### Raft storage roots (legacy names)

| Variable               | Default    | Description                                            |
| ---------------------- | ---------- | ------------------------------------------------------- |
| `LITHAIR_DATA_DIR`     | `"./data"` | Base directory for the Raft WAL and snapshots           |
| `EXPERIMENT_DATA_BASE` | -          | Fallback for `LITHAIR_DATA_DIR` (used by the BDD harness) |

Resolution order: `LITHAIR_DATA_DIR` > `EXPERIMENT_DATA_BASE` > `./data`.

---

## Hot-Reload Support

### Hot-Reloadable (No Restart Required)

These settings can be changed at runtime via the admin API:

- **Server:** `cors_enabled`, `cors_origins`, `request_timeout`, `max_body_size`
- **Sessions:** `cleanup_interval`, `max_age`, `cookie_*` settings
- **RBAC:** `default_role`, `audit_enabled`, `rate_limit_enabled`, `max_login_attempts`
- **Replication:** `election_timeout`, `heartbeat_interval`, `snapshot_threshold`
- **Admin:** `enabled`, `path`, `auth_required`, `metrics_enabled`
- **Logging:** `level`, `format`, `file_*` settings
- **Storage:** `snapshot_interval`, `compaction_*`, `backup_*` settings
- **Performance:** All settings

### Requires Restart

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
use lithair_core::LithairServer;
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

## Frontend Configuration

Static asset serving settings.

| Variable     | Default | Config File | Env Var                  | Code Builder             | Hot-Reload | Description                          |
| ------------ | ------- | ----------- | ------------------------ | ------------------------ | ---------- | ------------------------------------ |
| `enabled`    | `false` |             | `LT_FRONTEND_ENABLED`    | `.with_frontend(String)` |            | Enable frontend asset serving        |
| `static_dir` | `None`  |             | `LT_FRONTEND_STATIC_DIR` | `.with_frontend(String)` |            | Directory containing static assets   |
| `watch`      | `false` |             | `LT_FRONTEND_WATCH`      | -                        |            | Watch frontend directory for changes |
| `compress`   | `true`  |             | `LT_FRONTEND_COMPRESS`   | -                        |            | Compress frontend assets             |

### Example

**config.toml:**

```toml
[frontend]
enabled = true
static_dir = "./frontend/dist"
watch = false
compress = true
```

**Environment:**

```bash
LT_FRONTEND_ENABLED=true
LT_FRONTEND_STATIC_DIR=./frontend/dist
LT_FRONTEND_WATCH=false
LT_FRONTEND_COMPRESS=true
```

**Code:**

```rust
LithairServer::new()
    .with_frontend("./frontend/dist")
```

---

## See Also

- [Getting Started Guide](./getting-started.md)
- [RBAC Guide](./rbac.md)
- [Session Management](./sessions.md)
- [Replication Guide](./replication.md)
- [Admin Panel](./admin-panel.md)
