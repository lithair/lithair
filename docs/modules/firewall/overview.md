# HTTP Firewall (v1)

This firewall is a lightweight, in-process middleware applied before request handling in the Pure Declarative HTTP server.

It provides:

- IP allow/deny filtering
- Global QPS limit (per second)
- Per-IP QPS limit (per second)
- OPTIONS requests are exempt to preserve CORS preflight

Default: disabled, opt-in via environment.

## Configuration (ENV)

- LT_FW_ENABLE = `1`|`0` (default `0`)
- LT_FW_IP_ALLOW = CSV of IPs (exact match). Empty means allow all unless denied.
- LT_FW_IP_DENY = CSV of IPs (exact match). Deny takes precedence.
- LT_FW_RATE_GLOBAL_QPS = integer (e.g. `1000`)
- LT_FW_RATE_PERIP_QPS = integer (e.g. `100`)

Examples:

```bash
# Deny localhost entirely
LT_FW_ENABLE=1 LT_FW_IP_DENY=127.0.0.1 cargo run -p raft_replication_demo --bin pure_declarative_node -- --node-id 1 --port 8080

# Only allow a single IP
LT_FW_ENABLE=1 LT_FW_IP_ALLOW=192.168.1.50 cargo run -p raft_replication_demo --bin pure_declarative_node -- --node-id 1 --port 8080

# Rate limit globally and per IP
LT_FW_ENABLE=1 LT_FW_RATE_GLOBAL_QPS=500 LT_FW_RATE_PERIP_QPS=50 cargo run -p raft_replication_demo --bin pure_declarative_node -- --node-id 1 --port 8080
```

## Error semantics

- 403 Forbidden when IP is denied or not in allow list
- 429 Too Many Requests when global or per-IP QPS exceeded
- Error body (JSON):

```json
{"error":"forbidden","message":"IP not in allow list"}
```

or

```json
{"error":"rate_limited","message":"Global QPS limit exceeded"}
```

Per-IP overflow returns:

```json
{"error":"ip_rate_limited","message":"Per-IP QPS limit exceeded"}
```

## Notes

- Matching is currently exact by IP string (no CIDR yet in v1).
- State uses 1-second fixed windows. This is sufficient for first-line protection; a token bucket can be added later if needed.
- OPTIONS requests are bypassed by the firewall to not break CORS preflight.

## Roadmap

- CIDR support (allow/deny subnets)
- Per-endpoint rules (method+path selectors)
- Token bucket with burst capacity
- Config file and live reload

## Declarative configuration on the model

Besides ENV and CLI, you can configure the firewall directly on your model with a struct-level attribute parsed by the `DeclarativeModel` derive macro.

Precedence when multiple sources are present:

1. `DeclarativeServer::with_firewall_config(cfg)` (builder)
2. `#[firewall(...)]` attribute on the model
3. Environment variables (`LT_FW_*`)

Example:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
#[firewall(
  enabled = true,
  allow = "127.0.0.1",
  protected = "/api/products",
  exempt = "/status,/health",
  global_qps = 3,
  per_ip_qps = 2
)]
pub struct Product { /* ... */ }
```

Route scoping semantics:

- `exempt` prefixes bypass the firewall entirely (e.g., `/status`, `/health`).
- When `protected` is non-empty, firewall applies only to those prefixes.
- When `protected` is empty, firewall applies globally (except `exempt`).

## Example binaries and scripts

- Fully declarative example (model attribute only):
  - Binary: `examples/raft_replication_demo/http_firewall_declarative.rs`
  - Run: `cargo run -p raft_replication_demo --bin http_firewall_declarative -- --port 8081`
  - Script: `examples/http_firewall_demo/run_declarative_demo.sh`

- CLI-configurable example (flags override attribute/env):
  - Binary: `examples/raft_replication_demo/http_firewall_node.rs`
  - Script: `examples/http_firewall_demo/run_demo.sh`
  - Use `--fw-protected-prefixes "/api/products"` and `--fw-exempt-prefixes "/status,/health"` for route-scoped protection

## What you can run now (Quickstart)

Fully declarative (model attribute only):

```bash
bash examples/http_firewall_demo/run_declarative_demo.sh
```

Or manual:

```bash
cargo run -p raft_replication_demo --bin http_firewall_declarative -- --port 8081
curl http://127.0.0.1:8081/status
curl http://127.0.0.1:8081/api/products
```

CLI-configurable demo (flags):

```bash
bash examples/http_firewall_demo/run_demo.sh
```

Demonstrates deny/allow and rate limiting with route scoping.
