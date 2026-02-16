# HTTP Hardening, Gzip & Firewall

This guide explains production hardening for Lithair HTTP servers:
- Server-side gzip negotiation (global and per-route)
- Per-route policies (force gzip on/off, no-store, per-route thresholds)
- Web Firewall configuration (IP allow/deny, rate limiting, route scoping)
- Protecting sensitive endpoints like `/perf/*` and `/metrics`

Related guide:
- Stateless perf endpoints & benchmarking: `docs/guides/http_performance_endpoints.md`

---

## Gzip Compression
Gzip is negotiated with `Accept-Encoding: gzip`. When enabled, Lithair adds `Content-Encoding: gzip` and `Vary: Accept-Encoding`.

### Global configuration
```rust
use lithair_core::http::declarative_server::GzipConfig;

let server = server
    .with_gzip_config(GzipConfig { enabled: true, min_bytes: 1024 });
```

### Per-route overrides
Use `RoutePolicy` to force gzip on/off and override `min_bytes` for specific URI prefixes.

```rust
use lithair_core::http::declarative_server::RoutePolicy;

let server = server
    // Force gzip for all /perf/* responses and mark as non-cacheable
    .with_route_policy(
        "/perf",
        RoutePolicy { gzip: Some(true), no_store: true, min_bytes: Some(1024) }
    )
    // Disable gzip for a particular admin path
    .with_route_policy(
        "/admin",
        RoutePolicy { gzip: Some(false), no_store: false, min_bytes: None }
    );
```

### Environment overrides
- `LT_HTTP_GZIP=1` or `true` → Enable globally.
- `LT_HTTP_GZIP_MIN=1024` → Minimum body size to start compressing.

Notes:
- Small bodies below `min_bytes` are not compressed.
- Responses that already set `Content-Encoding` are not recompressed.

---

## Web Firewall (IP allow/deny, rate limiting)
The built-in Firewall secures HTTP endpoints with IP filtering, request rate limiting and route scoping. It’s enforced at the start of request processing.

Data types:
```rust
use lithair_core::http::FirewallConfig;

#[derive(Clone, Debug)]
pub struct FirewallConfig {
    pub enabled: bool,
    pub allow: std::collections::HashSet<String>,
    pub deny: std::collections::HashSet<String>,
    pub global_qps: Option<u64>,
    pub per_ip_qps: Option<u64>,
    pub protected_prefixes: Vec<String>,
    pub exempt_prefixes: Vec<String>,
}
```

Important:
- `allow` and `deny` now support:
  - Exact IP addresses (e.g., `10.0.0.10`, `::1`)
  - CIDR subnets via `ipnet` (e.g., `192.168.0.0/16`, `fc00::/7`)
  - Macro shortcuts (case-insensitive):
    - `private_v4` → `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`
    - `private`, `internal`, `internal_private`, `internal_private_ip` → all private IPv4 ranges + IPv6 ULA `fc00::/7`
    - `loopback` → `127.0.0.0/8`, `::1/128`
    - `link_local` → `169.254.0.0/16`, `fe80::/10`
- `protected_prefixes` — if non-empty, the firewall applies only to these prefixes.
- `exempt_prefixes` — request paths under these prefixes bypass the firewall.

### Production example: Protect `/perf/*` and `/metrics`
```rust
use lithair_core::http::FirewallConfig;
use std::collections::{HashSet};

let mut allow = HashSet::new();
allow.insert("10.0.0.10".to_string()); // exact IP
allow.insert("192.168.0.0/16".to_string()); // CIDR subnet
allow.insert("internal".to_string()); // Macro: all private ranges (incl. IPv6 ULA)

let fw = FirewallConfig {
    enabled: true,
    allow,
    deny: HashSet::new(),
    global_qps: Some(1000),
    per_ip_qps: Some(50),
    // Apply firewall checks only to /perf and /metrics
    protected_prefixes: vec!["/perf".into(), "/metrics".into()],
    // Exempt health/status endpoints for liveness checks
    exempt_prefixes: vec!["/status".into(), "/health".into()],
};

let server = server.with_firewall_config(fw);
```

- Only clients whose IP matches the allow list (exact IPs, CIDR ranges, or macros like `internal`) can access `/perf/*` and `/metrics`.
- Other endpoints are unaffected (firewall checks do not run for them).
- Health endpoints remain reachable for probes.

### Environment configuration (no code change)
```bash
# Enable firewall and restrict IP access (comma-separated exact IPs)
export LT_FW_ENABLE=1
export LT_FW_IP_ALLOW="internal,10.0.0.10,192.168.0.0/16"
export LT_FW_IP_DENY=""  # optional

# Rate limits
export LT_FW_RATE_GLOBAL_QPS=1000
export LT_FW_RATE_PERIP_QPS=50
```

Then in code, rely on model or builder defaults; the final config is resolved with precedence `builder > model > env`.

Tip:
- You can also use macros in code by inserting their string names into `allow`/`deny`. They will be expanded internally the same way as env values.

---

## Protecting Metrics in Production
Metrics often leak operational details. Use the firewall to scope `/metrics` to trusted networks, or place Lithair behind a reverse proxy that gates access.

Recommendations:
- Gate `/metrics` to a Prometheus server IP list via `allow`.
- Keep `/health` and `/status` exempt for liveness/readiness checks.
- Enable a moderate `global_qps` and `per_ip_qps` on protected prefixes to avoid abuse.

Example server defaults:
- The demo server `http_hardening_node` starts with a production-like firewall posture by default (protects `/perf` and `/metrics`, exempts `/status` and `/health`, `allow` includes the `internal` macro).
- To run it open for local testing, pass the CLI flag `--open`.

---

## Disable or Gate Stateless Perf Endpoints in Production
The perf endpoints are intended for benchmarking. Disable them in production or gate them with the firewall.

Options:
- Disable in code:
```rust
.with_perf_endpoints(PerfEndpointsConfig { enabled: false, base_path: "/perf".into() })
```
- Disable via env:
```bash
export LT_PERF_ENABLED=0
```
- Or gate with `FirewallConfig` as shown above.

---

## Full Example (Server Builder)
```rust
use lithair_core::http::declarative_server::{
    DeclarativeServer, PerfEndpointsConfig, GzipConfig, RoutePolicy
};
use lithair_core::http::FirewallConfig;
use std::collections::HashSet;

let port = 18320;
let perf = PerfEndpointsConfig { enabled: true, base_path: "/perf".into() };
let gzip = GzipConfig { enabled: true, min_bytes: 1024 };

let mut allow = HashSet::new();
allow.insert("10.0.0.10".into());
allow.insert("10.0.0.11".into());

let fw = FirewallConfig { 
    enabled: true,
    allow,
    deny: HashSet::new(),
    global_qps: Some(1000),
    per_ip_qps: Some(50),
    protected_prefixes: vec!["/perf".into(), "/metrics".into()],
    exempt_prefixes: vec!["/status".into(), "/health".into()],
};

DeclarativeServer::<Product>::new("./data/product.events", port)?
    .with_perf_endpoints(perf)
    .with_gzip_config(gzip)
    .with_route_policy("/perf", RoutePolicy { gzip: Some(true), no_store: true, min_bytes: Some(1024) })
    .with_firewall_config(fw)
    .serve()
    .await?;
```

Deploy with env overrides for a fully locked-down production posture.
