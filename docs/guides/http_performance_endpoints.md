# HTTP Stateless Performance Endpoints

This guide documents the stateless performance endpoints designed to benchmark the HTTP stack in isolation, plus the built-in load generator and a ready-to-run benchmarking script. Use it together with the metrics endpoint to correlate throughput/latency with internal server metrics.

Related guides:
- Hardening, Gzip, Route Policies: see `docs/guides/http_hardening_gzip_firewall.md`

## What are Stateless Perf Endpoints?
Synthetic, non-persistent endpoints that return or echo data without touching storage or business logic. They are ideal to benchmark raw HTTP performance.

Endpoints (assuming base path `/perf`):
- `POST /perf/echo` — Echoes request body as-is (binary), `Content-Type: application/octet-stream`.
- `GET /perf/json?bytes=N` — Returns a JSON with a string payload of ~N bytes.
- `GET /perf/bytes?n=N` — Returns raw bytes of length N.

Limits:
- `RS_PERF_MAX_BYTES` caps requested sizes (default: 2,000,000 bytes).

## Enable in Code
```rust
use lithair_core::http::declarative_server::{DeclarativeServer, PerfEndpointsConfig};

let server = DeclarativeServer::<Product>::new("./data/product.events", 18320)?
    .with_perf_endpoints(PerfEndpointsConfig {
        enabled: true,
        base_path: "/perf".into(),
    });
```

## Environment Overrides
- `RS_PERF_ENABLED=1` (or `true`) — Enable without code change.
- `RS_PERF_BASE=/bench` — Change the base path.
- `RS_PERF_MAX_BYTES=2000000` — Cap the size for `/perf/json` and `/perf/bytes`.

## Using Metrics Alongside Benchmarks
For meaningful analysis, correlate your perf runs with live metrics. Lithair exposes Prometheus-compatible metrics at `/metrics` (see README Monitoring & Health Checks).

Examples:
```bash
# Start server (example port)
cargo run -p raft_replication_demo --bin http_hardening_node -- --port 18320

# Scrape metrics
curl -s http://127.0.0.1:18320/metrics | head
```
Collect metrics during a perf run to measure CPU/memory, request counters, latency histograms (if exported), etc.

## Quick Usage
```bash
# Echo 1MB (POST)
curl -X POST http://127.0.0.1:18320/perf/echo --data-binary @/path/to/1mb.bin -o /dev/null -s -w "%{http_code}\n"

# JSON 100KB
curl "http://127.0.0.1:18320/perf/json?bytes=102400" -s -o /dev/null -w "%{size_download}\n"

# Raw bytes 1MB
curl "http://127.0.0.1:18320/perf/bytes?n=1048576" -s -o /dev/null -w "%{size_download}\n"
```

## Load Generator
`examples/raft_replication_demo/http_loadgen_demo.rs` supports modes:
- `perf-status` — GET a configured path (e.g., `/status`).
- `perf-json` — GET `{base}?bytes=N`.
- `perf-bytes` — GET `{base}?n=N` (or `bytes` alias).
- `perf-echo` — POST a body of N bytes to `{base}`.

Example:
```bash
cargo run --release -p raft_replication_demo --bin http_loadgen_demo -- \
  --leader http://127.0.0.1:18320 --total 10000 --concurrency 512 \
  --mode perf-json --perf-path /perf/json --perf-bytes 1024
```

## Benchmark Script
`examples/raft_replication_demo/bench_http_server_stateless.sh` orchestrates runs and writes Markdown reports under `baseline_results/`.

Environment knobs:
- `PORT`, `CONCURRENCY`, `TIMEOUT_S`
- Totals per scenario: `TOTAL_STATUS`, `TOTAL_JSON_1KB`, `TOTAL_BYTES_1KB`, `TOTAL_ECHO_1KB`, etc.
- `RS_HTTP_GZIP`, `RS_HTTP_GZIP_MIN` (if testing gzip)
- `RS_PERF_MAX_BYTES` size caps

Run:
```bash
# Without gzip
bash examples/raft_replication_demo/bench_http_server_stateless.sh

# With gzip forced
RS_HTTP_GZIP=1 RS_HTTP_GZIP_MIN=1024 bash examples/raft_replication_demo/bench_http_server_stateless.sh
```

## Production Considerations
- Disable or strictly protect `/perf/*` in production. These endpoints are for benchmarking.
- Limit exposure via the Firewall to internal networks only (see the Hardening guide).
- Use `/metrics` to continuously observe production behavior; consider gating it as well.
