# HTTP Firewall Demo (v1)

This example demonstrates the in-process HTTP firewall integrated into the Pure Declarative server.

It validates:

- IP deny and allow lists
- Global QPS rate limiting
- Per-IP QPS rate limiting
- OPTIONS preflight exempt (CORS-friendly)

## Prerequisites

- Rust toolchain
- This repo built once (first run will build automatically)

## Quick start

From repo root:

```bash
bash examples/http_firewall_demo/run_demo.sh
```

The script will:

- Free port 8080 if needed
- Build `pure_declarative_node`
- Run several scenarios and assert HTTP statuses:
  - Baseline (firewall disabled): GET /status → 200
  - Deny localhost: GET /status → 403
  - Allow list mismatch: GET /status → 403
  - Allow localhost: GET /status → 200
  - Global QPS limit: flood → some 429
  - Per-IP QPS limit: flood → some 429

Logs are written to `examples/http_firewall_demo/node_demo.log`.

## Environment variables

- `RS_FW_ENABLE` = `1` to enable firewall
- `RS_FW_IP_DENY` = CSV of IPs to deny (exact match)
- `RS_FW_IP_ALLOW` = CSV of IPs to allow (exact match). If set, only these are allowed.
- `RS_FW_RATE_GLOBAL_QPS` = global requests per second limit
- `RS_FW_RATE_PERIP_QPS` = per-IP requests per second limit

See `docs/HTTP_FIREWALL.md` for details and roadmap.
