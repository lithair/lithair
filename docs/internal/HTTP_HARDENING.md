# Lithair HTTP Hardening Guide

This document summarizes the production-oriented defaults and knobs available in the Pure Declarative HTTP server.

## Overview (What’s enabled by default)

- CORS + Security headers are applied on all responses
- OPTIONS preflight is supported on any path (204 No Content)
- Per-request timeout for API routes (default 10s)
- JSON Content-Type validation for POST/PUT
- Request body size limits (single/bulk)
- Graceful shutdown on Ctrl-C / SIGTERM
- Consistent error semantics and status codes

## Configuration (Environment Variables)

- LT_HTTP_TIMEOUT_MS
  - Default: `10000` (10 seconds)
  - Applies a per-request timeout to API paths (`/api/{model}...`). On timeout: 504.

- LT_HTTP_MAX_BODY_BYTES_SINGLE
  - Default: `2097152` (2 MiB)
  - Max request size for single-item POST/PUT (`/api/{model}`, `/api/{model}/{id}`)

- LT_HTTP_MAX_BODY_BYTES_BULK
  - Default: `12582912` (12 MiB)
  - Max request size for bulk POST (`/api/{model}/_bulk`)

## CORS & Security Headers

Applied to every response:

- Access-Control-Allow-Origin: `*`
- Access-Control-Allow-Methods: `GET, POST, PUT, DELETE, OPTIONS`
- Access-Control-Allow-Headers: `Content-Type, Authorization`
- X-Content-Type-Options: `nosniff`
- X-Frame-Options: `DENY`
- Referrer-Policy: `no-referrer`
- Content-Security-Policy: `default-src 'none'; frame-ancestors 'none'; base-uri 'none'`

Preflight handling:

- Any path accepts `OPTIONS` and responds `204 No Content` with the headers above.

## Timeouts

- Configured via `LT_HTTP_TIMEOUT_MS` (default 10000 ms)
- If a request exceeds this time budget on an API route, the server returns:

```json
{"error":"request timeout"}
```

with HTTP `504 Gateway Timeout`.

## Request Body Size Limits

- Single-item endpoints (POST/PUT): `LT_HTTP_MAX_BODY_BYTES_SINGLE` (2 MiB by default)
- Bulk ingestion (POST /_bulk): `LT_HTTP_MAX_BODY_BYTES_BULK` (12 MiB by default)
- Exceeding a limit yields HTTP `413 Payload Too Large`.

## Content-Type Validation

- For POST/PUT, the server expects `Content-Type: application/json`.
- Otherwise, it returns HTTP `415 Unsupported Media Type`.

## Status Codes & Error Shape

- 201 Created – create and bulk create
- 200 OK – fetches; 204 No Content – delete
- 400 Bad Request – malformed JSON or validation errors
- 404 Not Found – resource missing
- 405 Method Not Allowed – includes `Allow` header
- 413 Payload Too Large – body over the configured limit
- 415 Unsupported Media Type – missing or wrong Content-Type
- 500 Internal Server Error – unexpected failure
- 503 Service Unavailable – consensus failure (when applicable)
- 504 Gateway Timeout – request exceeded `LT_HTTP_TIMEOUT_MS`

JSON error shape (representative):

```json
{"error":"bad_request","message":"invalid json"}
```

Some endpoints may return specialized `error` codes (e.g. `rbac_denied`, `unsupported_media_type`, `payload_too_large`, `request timeout`).

## TLS Termination

Lithair supports native TLS via rustls. When `LT_TLS_CERT` and `LT_TLS_KEY` are set, the server:

1. Loads the PEM certificate chain and private key at startup
2. Logs the leaf certificate SHA-256 fingerprint
3. Accepts TLS connections with a 10-second handshake timeout (slow/stalled handshakes are dropped)
4. Adds `Strict-Transport-Security: max-age=31536000; includeSubDomains` to all responses

HSTS is only sent when TLS is active. Plain HTTP responses never include the header.

See `docs/guides/tls.md` for setup instructions.

## Graceful Shutdown

- The server drains connections when receiving Ctrl-C / SIGTERM, reducing in-flight request loss during restarts.

## Recommended Defaults

- For latency-sensitive APIs, leave the default 10s timeout and adjust per route via upstream proxy if needed.
- Keep single-item body limit at 2 MiB; increase bulk limit only when your ingestion batches require it (observe 413s under load tests to tune).
- Prefer light reads (`/status`, `/api/{model}/count`) when benchmarking write paths to avoid serialization overhead of large lists.

## Related Documents

- `docs/guides/tls.md` – TLS setup, certificates, and HSTS.
- `docs/API_REFERENCE.md` – high-level reference; includes a summary under "HTTP Hardening & Error Semantics".
- `docs/HTTP_LOADGEN.md` – benchmarking & scenarios.
- `examples/raft_replication_demo/README.md` – demo scenario guidance.

## Roadmap

- Firewall middleware (IP allow/deny, per-IP and per-route rate limiting) – to be introduced in a subsequent PR.
- Unified error helper usage across all modules (RBAC, replication internals) – gradual refactor to ensure consistent error shapes everywhere.
