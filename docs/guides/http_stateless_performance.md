# Lithair HTTP Stateless Performance & Hardening Guide

This guide explains how to enable and use the stateless performance endpoints, server-side gzip compression, per-route policies, environment overrides, and static file caching (ETag) in Lithair. All configuration is fully declarative and can be toggled per environment.

- Target files:
  - `lithair-core/src/http/declarative_server.rs`
  - `examples/raft_replication_demo/http_hardening_node.rs`
  - `examples/raft_replication_demo/bench_http_server_stateless.sh`

## Overview
- Stateless perf endpoints for HTTP-only benchmarking with zero persistence.
- Gzip compression (HTTP/1.1) with Accept-Encoding negotiation.
- Per-route policies (force gzip on/off, no-store, per-route min-bytes).
- Env overrides for maximum flexibility.
- Static files with strong caching:
  - ETag on all files.
  - `Cache-Control: public, max-age=31536000, immutable` for `assets/...`.
  - `Cache-Control: no-cache` for `index.html`.

---

## Declarative Performance Endpoints
The performance endpoints are off the model API path and never interact with persistence. They are intended to measure the HTTP stack end-to-end with controlled payload sizes.

- Endpoints (assuming base path `/perf`):
  - `POST /perf/echo` → Echoes request body (binary). Content-Type: `application/octet-stream`.
  - `GET  /perf/json?bytes=N` → JSON with a string payload of ~N bytes.
  - `GET  /perf/bytes?n=N` → Raw binary payload of N bytes.

- Maximum payload: controlled by env `LT_PERF_MAX_BYTES` (default: 2,000,000 bytes).

### Enable in code
```rust
use lithair_core::http::declarative_server::{DeclarativeServer, PerfEndpointsConfig};

let server = DeclarativeServer::<Product>::new("./data/product.events", 18320)?
    .with_perf_endpoints(PerfEndpointsConfig {
        enabled: true,
        base_path: "/perf".into(),
    });
```

### Environment overrides
- `LT_PERF_ENABLED=1` or `true` → Enable even if disabled in code.
- `LT_PERF_BASE=/bench` → Change base path without code change.
- `LT_PERF_MAX_BYTES=2000000` → Cap for `json`/`bytes` endpoints.

### Quick usage
```bash
# Echo 1MB (POST)
curl -X POST http://127.0.0.1:18320/perf/echo --data-binary @/path/to/1mb.bin -o /dev/null -s -w "%{http_code}\n"

# JSON 100KB
curl "http://127.0.0.1:18320/perf/json?bytes=102400" -s -o /dev/null -w "%{size_download}\n"

# Raw bytes 1MB
curl "http://127.0.0.1:18320/perf/bytes?n=1048576" -s -o /dev/null -w "%{size_download}\n"
```

---

## Gzip Compression (HTTP/1.1)
Gzip is negotiated via `Accept-Encoding: gzip`. When enabled, Lithair compresses eligible responses and adds `Vary: Accept-Encoding`.

- Code configuration:
```rust
use lithair_core::http::declarative_server::GzipConfig;

let server = server
    .with_gzip_config(GzipConfig { enabled: true, min_bytes: 1024 });
```

- Environment:
  - `LT_HTTP_GZIP=1` (or `true`) → enable globally.
  - `LT_HTTP_GZIP_MIN=1024` → minimum body size before compressing.

- Verification:
```bash
# Should return Content-Encoding: gzip when Accept-Encoding includes gzip
curl -sI -H 'Accept-Encoding: gzip' http://127.0.0.1:18320/status | grep -i '^content-encoding: gzip'
```

Notes:
- Small bodies below `min_bytes` are not compressed.
- Responses already having `Content-Encoding` are not recompressed.

---

## Per-route Policies
You can override gzip and caching for specific URI prefixes using `RoutePolicy`. The longest matching prefix wins.

- Data type:
```rust
#[derive(Clone, Debug, Default)]
pub struct RoutePolicy {
    pub gzip: Option<bool>,       // None = inherit, Some(true/false) = force enable/disable
    pub no_store: bool,           // Adds Cache-Control: no-store
    pub min_bytes: Option<usize>, // Overrides gzip min_bytes when gzip is enabled
}
```

- Usage:
```rust
use lithair_core::http::declarative_server::RoutePolicy;

let server = server
    // Force gzip under /perf, and mark payloads as non-cacheable
    .with_route_policy(
        "/perf",
        RoutePolicy { gzip: Some(true), no_store: true, min_bytes: Some(1024) }
    );
```

Behavior:
- `gzip: Some(true)`
  - Forces gzip even if global gzip is disabled.
- `gzip: Some(false)`
  - Disables gzip for that prefix even if global gzip is enabled.
- `no_store: true`
  - Adds `Cache-Control: no-store` to responses under that prefix.
- `min_bytes`
  - Overrides gzip threshold for that prefix only when gzip is enabled.

---

## Static Files: ETag and Cache-Control
Static file serving is enabled by setting `LT_STATIC_DIR` to a directory containing `index.html` and an `assets/` folder.

- Endpoints:
  - `/` or `/index.html` → serves `index.html`
  - `/assets/...` → serves files under `assets/`

- Caching policy:
  - All files include an `ETag` header (SHA-256 of file content).
  - `index.html`: `Cache-Control: no-cache`
  - `assets/...`: `Cache-Control: public, max-age=31536000, immutable`

- Conditional requests using ETag:
```bash
# Get ETag for index.html
ETAG=$(curl -sI http://127.0.0.1:18320/index.html | awk -F": " '/^ETag/ {print $2}' | tr -d '\r')

# Ask with If-None-Match – should return 304 if unchanged
curl -sI -H "If-None-Match: $ETAG" http://127.0.0.1:18320/index.html | head -n 1
```

---

## Load Generation & Benchmarking
There is a dedicated load generator and a ready-to-run benchmarking script.

### Load generator (modes)
`examples/raft_replication_demo/http_loadgen_demo.rs` supports:
- `perf-status`: GET a configured status/perf path.
- `perf-json`: GET `{base}?bytes=N`.
- `perf-bytes`: GET `{base}?n=N` (or `bytes=` as alias).
- `perf-echo`: POST body of N bytes to `{base}`.

Examples:
```bash
# 10k requests, concurrency 512, JSON 1KB
cargo run --release -p raft_replication_demo --bin http_loadgen_demo -- \
  --leader http://127.0.0.1:18320 --total 10000 --concurrency 512 \
  --mode perf-json --perf-path /perf/json --perf-bytes 1024
```

### Benchmark script
`examples/raft_replication_demo/bench_http_server_stateless.sh` runs a suite and writes a Markdown report under `baseline_results/`.

Environment knobs:
- `PORT`, `CONCURRENCY`, `TIMEOUT_S`
- Totals per scenario (examples): `TOTAL_STATUS`, `TOTAL_JSON_1KB`, `TOTAL_BYTES_1KB`, `TOTAL_ECHO_1KB`, ...
- `LT_HTTP_GZIP`, `LT_HTTP_GZIP_MIN` for compression
- `LT_PERF_MAX_BYTES` for perf endpoints size caps

Run:
```bash
# Without gzip
tbash examples/raft_replication_demo/bench_http_server_stateless.sh

# With gzip forced
tLT_HTTP_GZIP=1 LT_HTTP_GZIP_MIN=1024 bash examples/raft_replication_demo/bench_http_server_stateless.sh
```

The report includes throughput and latency percentiles (p50, p95, p99) per operation type.

---

## CI Smoke Tests
The workflow `/.github/workflows/ci.yml` includes:
- A fast stateless perf smoke run with moderate concurrency and capped payload sizes.
- A gzip negotiation step that verifies `Content-Encoding: gzip` on `/status` and `/perf/json?bytes=1024`.

These checks help detect regressions early without running heavy benchmarks.

---

## Troubleshooting
- Payload rejected / truncated
  - Ensure `LT_PERF_MAX_BYTES` is large enough for your test.
- No gzip in response
  - Include `Accept-Encoding: gzip`, and check the size vs `min_bytes`.
  - Verify per-route policies (a route policy may disable gzip).
- Static files not served
  - Set `LT_STATIC_DIR` to the correct directory and include `index.html` and `assets/`.
- ETag mismatch
  - ETag values are quoted. Ensure the `If-None-Match` header includes quotes.

---

## Security Considerations
- The stateless perf endpoints are for benchmarking; disable them in production or protect them via the Firewall config.
- Use `RoutePolicy { no_store: true }` on routes that must never be cached by intermediaries or clients.
- All responses include CORS and security headers by default (e.g., `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`).

---

## Full example (excerpt)
Below is the minimal example showing all features together in `examples/raft_replication_demo/http_hardening_node.rs`:

```rust
use lithair_core::http::declarative_server::{
    DeclarativeServer, PerfEndpointsConfig, GzipConfig, RoutePolicy
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let port = 18320;
    std::fs::create_dir_all("./data").ok();

    let perf_cfg = PerfEndpointsConfig { enabled: true, base_path: "/perf".into() };

    DeclarativeServer::<Product>::new("./data/product.events", port)?
        .with_perf_endpoints(perf_cfg)
        .with_gzip_config(GzipConfig { enabled: true, min_bytes: 1024 })
        .with_route_policy("/perf", RoutePolicy { gzip: Some(true), no_store: true, min_bytes: Some(1024) })
        .serve()
        .await
}
```

You can override behavior per environment with:
```bash
export LT_PERF_ENABLED=1
export LT_PERF_BASE=/bench
export LT_HTTP_GZIP=1
export LT_HTTP_GZIP_MIN=512
export LT_PERF_MAX_BYTES=2000000
export LT_STATIC_DIR=./tests/frontend_benchmark/dist
```

If you need additional examples (e.g., disabling gzip under a specific prefix or raising `min_bytes` per route), let us know.
