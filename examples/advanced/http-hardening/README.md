# HTTP Hardening Demo

This demo validates the HTTP hardening features of the Pure Declarative server:

- Global CORS + security headers
- OPTIONS preflight handling
- Per‑request timeout (LT_HTTP_TIMEOUT_MS)
- Request body size limits (single/bulk)
- JSON Content‑Type validation
- Coherent status codes (405/413/415/504)
- Graceful shutdown

## Prerequisites

- Rust toolchain installed
- `jq` and `curl` installed

## Quick Start

```bash
# From repo root
bash examples/http_hardening_demo/run_demo.sh
```

This script:
- Kills lingering nodes
- Builds the demo binaries
- Starts a single declarative node on :8080
- Runs deterministic curl tests and prints the observed codes and headers
- Shuts the node down

## What is exercised

- OPTIONS preflight → 204 + CORS/security headers
- CORS/security headers presence on /status and /api paths
- 415 Unsupported Media Type on POST/PUT without JSON content type
- 413 Payload Too Large on oversized bulk request (limit: LT_HTTP_MAX_BODY_BYTES_BULK)
- 405 Method Not Allowed with Allow header on wrong verb
- 504 Gateway Timeout (best‑effort) when LT_HTTP_TIMEOUT_MS is set very low

Notes:
- Timeouts depend on scheduling; setting `LT_HTTP_TIMEOUT_MS=1` ms typically yields 504 on commodity hosts, but is not guaranteed on very fast responses. Consider raising to a tiny value like 5–10ms if needed.
- You can tune size limits via `LT_HTTP_MAX_BODY_BYTES_SINGLE`/`LT_HTTP_MAX_BODY_BYTES_BULK` to make 413 more or less strict.

## Files

- `run_demo.sh` – orchestration and assertions

## Next Steps

- Integrate firewall middleware (IP allow/deny, rate limiting) once available.
- Extend with Prometheus `/metrics` checks (future PR).
