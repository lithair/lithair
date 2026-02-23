# Environment Variables Reference

This page lists the core environment variables supported by Lithair.

## Core

- `RUST_LOG`
  - Controls logging verbosity (e.g., `error`, `warn`, `info`, `debug`).
  - Recommended: `info` for development, `warn` for production.

## Admin & Frontend Reload

- `LT_ADMIN_PATH`
  - Custom admin base path. Defaults to `/admin`.
  - Special value `random` generates a random path (e.g., `/secure-XXXXXX`) and persists it to `.admin-path`.

- `LT_DEV_RELOAD_TOKEN` (development only)
  - Enables simplified hot reload for hybrid mode via header `X-Reload-Token`.
  - Example: `LT_DEV_RELOAD_TOKEN=mytoken cargo run -- --hybrid`
  - Security: NEVER use in production. The server prints a warning when enabled.

- `LT_DOCS_PATH`
  - Path to Lithair documentation directory for blog/doc servers.
  - Defaults to `../Lithair/docs` (relative to blog project).
  - Used by Lithair-Blog to load and serve framework documentation.

## Server

- `LT_PORT`
  - Server listening port. Default: `8080`.

- `LT_HOST`
  - Server listening address. Default: `127.0.0.1`.

- `LT_WORKERS`
  - Number of Tokio worker threads. Default: auto-detect (num CPUs).

- `LT_REQUEST_TIMEOUT`
  - Request timeout in seconds. Default: `30`.

- `LT_MAX_BODY_SIZE`
  - Maximum request body size in bytes. Default: `10485760` (10 MB).

## TLS

- `LT_TLS_CERT`
  - Path to TLS certificate PEM file. When set with `LT_TLS_KEY`, the server starts in HTTPS mode with HSTS enabled.
  - Both `LT_TLS_CERT` and `LT_TLS_KEY` must be set together.

- `LT_TLS_KEY`
  - Path to TLS private key PEM file.
  - Both `LT_TLS_CERT` and `LT_TLS_KEY` must be set together.

## CORS

- `LT_COLT_ENABLED`
  - Enable CORS support. Default: `false`.

- `LT_COLT_ORIGINS`
  - Allowed CORS origins (comma-separated). Default: `*`.

## Benchmarks & Demos (examples)

- `PORT`, `HOST`
  - Network configuration used by examples and demos.

- `ACCEPT_ENCODING`
  - Used by performance/loadgen scripts to test gzip negotiation.

- `STORAGE_PROFILE`, `SINGLE_NODE`
  - Tuning flags used by benchmark suites (durability profiles, single-node runs).

## See also

- `docs/guides/tls.md` for TLS setup and certificate management
- `docs/guides/serving-modes.md` for Dev/Prod/Hybrid modes and reload endpoint
- `docs/guides/admin-protection.md` for firewall and admin endpoints
