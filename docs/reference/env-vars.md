# Environment Variables Reference

This page lists the core environment variables supported by Lithair.

## Core

- `RUST_LOG`
  - Controls logging verbosity (e.g., `error`, `warn`, `info`, `debug`).
  - Recommended: `info` for development, `warn` for production.

## Admin & Frontend Reload

- `RS_ADMIN_PATH`
  - Custom admin base path. Defaults to `/admin`.
  - Special value `random` generates a random path (e.g., `/secure-XXXXXX`) and persists it to `.admin-path`.

- `RS_DEV_RELOAD_TOKEN` (development only)
  - Enables simplified hot reload for hybrid mode via header `X-Reload-Token`.
  - Example: `RS_DEV_RELOAD_TOKEN=mytoken cargo run -- --hybrid`
  - Security: NEVER use in production. The server prints a warning when enabled.

- `RS_DOCS_PATH`
  - Path to Lithair documentation directory for blog/doc servers.
  - Defaults to `../Lithair/docs` (relative to blog project).
  - Used by Lithair-Blog to load and serve framework documentation.

## Benchmarks & Demos (examples)

- `PORT`, `HOST`
  - Network configuration used by examples and demos.

- `ACCEPT_ENCODING`
  - Used by performance/loadgen scripts to test gzip negotiation.

- `STORAGE_PROFILE`, `SINGLE_NODE`
  - Tuning flags used by benchmark suites (durability profiles, single-node runs).

## See also

- `docs/guides/serving-modes.md` for Dev/Prod/Hybrid modes and reload endpoint
- `docs/guides/admin-protection.md` for firewall and admin endpoints
