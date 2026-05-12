# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `lithair_core::app::request` module with body-reading helpers for custom
  routes (#63):
  - `read_body(req) -> Result<Vec<u8>>` — drain request body into bytes
  - `read_body_with_limit(req, max_bytes) -> Result<Vec<u8>>` — bounded read
    with a `Content-Length` pre-check plus a streaming `http_body_util::Limited`
    enforcement that aborts the read as soon as the cap is exceeded (closes
    the DoS path on chunked / unknown-length bodies — flagged by Gemini)
  - `read_body_as_string(req) -> Result<String>` — drain + UTF-8 decode
  - `read_body_json::<T>(req) -> Result<T>` — drain + `serde_json::from_slice`

  Closes the last consumer-side leak below the Lithair abstraction (after
  #59 closed handler signatures and #61 closed the response builder).
  Consumers like kovre serving `PUT /api/config` no longer need to re-add
  `http-body-util` and `bytes` as direct dependencies just to call
  `BodyExt::collect()` and walk the resulting `Bytes`.

## [0.5.0] - 2026-05-12

### Added

- `response::builder()` — chained `ResponseBuilder` returning `RouteResponse`,
  supporting `.status()`, `.header()`, `.body()`, and `.json_value()`. Lets
  consumers (e.g. kovre serving static assets with `Cache-Control: immutable`)
  drop direct `bytes` / `http-body-util` / `hyper` deps for the custom-header
  case that `response::json` / `text` / `html` can't cover (those hard-code
  `Content-Type` and nothing else). `body(...)` accepts anything
  `Into<Bytes>` — `&'static str`, `String`, `Vec<u8>`, `Bytes` — so the same
  shape covers static-asset and dynamic-payload callers (#61).
- `LithairServerBuilder::with_not_found_handler_async` — async-closure variant
  of `with_not_found_handler` that applies `Box::pin` internally. Mirrors
  `with_route_async` (v0.4.0) for symmetry; the existing sync-pinned variant
  remains for handlers that need explicit pinning control (#61).

## [0.4.0] - 2026-05-12

### Added

- `RouteRequest` and `RouteResponse` type aliases in `lithair_core::app` for
  consumer ergonomics. Custom-route consumers can now drop direct deps on
  `bytes`, `http`, `http-body-util`, and `hyper` from their `Cargo.toml` when
  they only need to type the handler signature — the four crates remain
  transitive deps of `lithair-core`, but no longer have to be tracked in
  lock-step by every downstream `Cargo.toml`. `http::Method` and
  `http::StatusCode` are also re-exported from `lithair_core::app` for the
  same reason (#59).
- `LithairServerBuilder::with_route_async` — convenience builder that accepts an
  async-closure handler and applies `Box::pin` internally. Mirrors the common
  `|req| async move { ... }` ergonomic while leaving the existing
  `with_route` (manual `Pin<Box<dyn Future<...>>>`) available for handlers
  that need explicit pinning control or that compose pre-built futures (#59).

## [0.3.0] - 2026-05-12

### Added

- `response::json_value(status, &serde_json::Value)` for typed JSON
  responses without `.to_string()` boilerplate (#47).
- `response::json_serialize<T: Serialize>(status, &T)` for typed JSON
  responses serialized directly from a `Serialize` value (#47).
- `query::param(query, key)` for single-key extraction from a query
  string, percent-decoded, bypassing the filter-spec semantics of
  `parse_query_params` (#48). Values like `>foo` are returned as the
  literal string instead of being parsed as a `Gt` filter. First
  occurrence wins on duplicate keys; empty decoded values map to `None`.

### Fixed

- **BDD cluster test isolation** (#29): Raft WAL/snapshot paths were hard-coded
  to `./data/raft/node_{N}/...` and ignored the test harness's
  `EXPERIMENT_DATA_BASE` tempdir, causing stale entry replay across runs and
  apparent leader connection drops (real cause: leader hang on apply-wait-loop
  for phantom entry indices). New `raft_base_dir()` helper resolves
  `LITHAIR_DATA_DIR` > `EXPERIMENT_DATA_BASE` > `./data`. Closed 4/11 → 11/11
  on `real_cluster_test.feature`.
- **`HttpServer::serve()` panic on std::thread workers** (#52): worker threads
  called `Handle::current()` from a `std::thread::spawn` context — panics
  silently, clients see `Empty reply from server`. Latent since the initial
  threaded-server implementation; surfaced only on in-process mock cluster
  paths. `serve()` now owns an explicit `tokio::Handle` (reuse from caller or
  build dedicated multi-thread); per-connection workers use it directly.
  Closed 0/14 → 14/14 on `distribution_clustering.feature`.
- **HTTP 404 status on successful static file responses** (#56): static-file
  dispatch gated on `method == GET`, so HEAD requests (used by `curl -I`,
  SEO bots, and monitoring probes) fell through to the default 404 JSON
  handler — body was correct on GET so browsers never noticed, but headers
  lied for HEAD. Static dispatch now accepts both GET and HEAD; HEAD emits
  `Content-Length` and an empty body per HTTP spec.
- `detect_mime_type` extended for `.xml`, `.rss`, `.atom`, `.woff2`,
  `.webmanifest`, `.ico`, `.wasm`, `.pdf`, `.webp`, `.txt`, `.md` (#56) —
  `/rss.xml` was previously served as `application/octet-stream`.

### Infrastructure

- Switched CI to [cidx](https://github.com/cidx-org/cidx) v1.7.0 (#53). The
  workflow `.github/workflows/cidx.yml` replaces the legacy `ci.yml` +
  `ci-fast.yml`. Developers and sub-agents can now run `cidx run code` (~6s)
  locally instead of waiting on GitHub Actions wall-clock time.
- BDD test harness now captures spawned-node stderr/stdout to a deterministic
  temp dir, and the cluster startup probe replaces a 500 ms sleep with an
  active `/health` poll (named-and-causal error message on timeout). Surfaces
  the next cluster-layer regression in seconds rather than minutes.

## [0.1.4] - 2026-04-29

### Added

- `LithairServer::with_vhost(host, builder)` and `with_default_vhost(builder)`
  — host-header-based routing primitive enabling multi-site hosting from a
  single binary without an external proxy (#30, #31). O(1) lookup, 23 tests.
- `LithairServer::with_redirect(from_host, to_host, ...)` — built-in 301
  redirect primitive (#36). Preserves path + query, applies to all HTTP
  methods, self-redirect-loop guard included.
- Blog post: ["The Layer I Stopped Choosing"](https://arcker.org/blog/2026-04-24-lithair-vhost-routing/).

### Fixed

- Clippy 1.95 compatibility fixes across macros and examples.
- `host_id` collision fix in the SCC2 store (per-vhost SCC2 derivation now
  collision-free).
- `LithairServer` no longer leaks host-agnostic frontends into a matched
  empty vhost.

## [0.2.0] - 2026-05-05

### Removed (breaking)

- `lithair_core::http::DeclarativeServer<T>` — the legacy single-model
  Hyper-direct server is gone. Use `LithairServer::new()` with
  `.with_port(...)`, `.with_declarative_model::<T>(path, base_path)`,
  and `.serve()` instead. Tracking: #42 (phase 4 + 5).
- `lithair_core::http::{GzipConfig, ObserveConfig, PerfEndpointsConfig,
  ReadinessConfig, RoutePolicy}` — these helper config types were defined
  alongside `DeclarativeServer` and only used through it. They were not
  referenced by any in-tree consumer outside `declarative_server.rs`.
- `lithair_core::http::DeclarativeServe` re-export — the `DeclarativeServe`
  convenience trait (`MyModel::serve_on_port(port).await?`) survives at
  its new public path **`lithair_core::app::DeclarativeServe`**. Default
  impls now delegate to `LithairServer` (behavior-preserving). The trait
  bound now also requires `HasSchemaSpec`; every type produced by
  `#[derive(DeclarativeModel)]` already implements it, so in-tree macro
  users are unaffected.

### Changed

- Macro-generated `main()` (single-node and distributed variants of
  `#[server(main, ...)]`) builds on `LithairServer` end-to-end —
  no remaining reference to `DeclarativeServer` in emitted code.

### Added

- `LithairServer` now serves built-in `/health`, `/ready`, `/info` ops
  endpoints out of the box (#40, #41) — previously only `DeclarativeServer`
  did. User-registered `.with_route()` overrides take precedence.

## [0.1.3] - 2026-04-01

### Added

- **Cluster Hardening**
  - Leader heartbeat (empty AppendEntries every election_timeout/3) to prevent unnecessary elections
  - WAL replay on startup — restores ConsensusLog state from disk after node restart
  - WAL compaction after snapshot — crash-safe (write to temp file, atomic rename)
  - Corruption guard in WAL reader (max 256MB per entry, graceful stop on bad data)
  - Conservative WAL replay: entries restored but not marked committed (cluster re-establishes consensus)
  - Per-follower match_index tracking in background catch-up task
  - 20 new cluster unit tests covering concurrency, corruption, edge cases

- **Write Path Optimization**
  - Write path sends single entry to followers instead of full log history (O(1) vs O(n))
  - Commit notifications send commit index only, background task handles lagging followers

- **HTTP & API**
  - PATCH endpoint for partial JSON merge updates
  - `#[db(unique)]` constraint enforcement on create, update, and patch
  - `#[lifecycle(immutable)]` field enforcement on update
  - `#[http(base_path = "custom")]` to override auto-generated REST path
  - `#[schema(version = N)]` for configurable schema version
  - Compile-time field validation in DeclarativeModel
  - 409 Conflict responses for unique constraint violations
  - PATCH added to CORS allowed methods + OPTIONS preflight handling
  - Audited field change logging on update and patch
  - URL percent-decoding for query filters and path IDs
  - OpenAPI 3.1 auto-generation from DeclarativeModel (#10)
  - SSE real-time subscriptions for model changes (#12)
  - Query, filter, and pagination on collection endpoints (#11)
  - System metrics module: CPU, RAM, load, RSS, request stats (#13)
  - In-memory AccessLogBuffer for zero-spawn log reads (#8)
  - TestHandler for integration testing DeclarativeModels

- **Infrastructure**
  - Feature-gate TLS, MFA, and cluster dependencies (#20)
  - Access logging for LithairServer (#6)
  - Real client IP in access logs behind reverse proxy (#7)
  - Pre-Merge Checklist added to CLAUDE.md

### Changed

- Renamed cluster binaries: `pure_declarative_node` → `lithair-cluster-node`, `blog_replicated_node` → `blog-cluster-node`
- Data directories renamed: `pure_node_{id}` → `node_{id}`
- Deprecated `DeclarativeConsensus`, `ConsensusConfig`, `HyperReplicationCoordinator` in favor of `LithairServer::with_raft_cluster()`
- BDD Taskfile tasks aligned with actual feature file locations

### Removed

- 3 dead cluster modules: `raft_replication.rs`, `optimized_replication.rs`, `simple_replication.rs` (-1310 lines)
- Dead modules and unused dependencies (-4185 lines, #19)
- Legacy code, old prelude, obsolete tools (#17)

### Fixed

- Firewall middleware configuration in LithairServer (#18)
- BDD test references to nonexistent binary names and paths
- Heartbeat and commit notifications now include correct `prev_log_index`/`prev_log_term`
- WAL corruption test writing to wrong file path
- Duplicate `node_{id}` path in stress test event directory construction

## [0.1.1] - 2025-02-07

### Added

- `lithair new` scaffolding CLI with BDD tests
- Native TLS termination with certificate fingerprint logging (#5)
- LithairServer hardening for self-sufficient deployment
- Custom handler DX improvements: error wrapping, 404 handler, response helpers
- Frontend framework integration examples (React, Angular, Vue, Svelte, Astro)
- Trunk-based development workflow documentation (#4)

### Changed

- Migrated `RS_*` env vars to `LT_*` prefix (#3)
- Reorganized examples with numbered progression (01-hello-world through 11-frontend-integrations)

### Fixed

- Security hardening: fail-closed guards, symlink safety, stale asset cleanup
- Eliminated risky `unwrap()`, fixed doc warnings, removed dead `println!()`
- All clippy warnings resolved across workspace
- CI pipeline optimizations (Alpine builds, disk space, release profiles)

## [0.1.0] - 2025-01-20

### Added

- **Core Framework**
  - Declarative model pattern with `#[derive(DeclarativeModel)]` macro
  - Memory-first architecture with SCC2 lock-free concurrent engine
  - Event sourcing with Write-Ahead Log (WAL) for durability
  - Hyper-based HTTP server with automatic REST API generation

- **Security**
  - Role-Based Access Control (RBAC) with field-level permissions
  - Session management with state engine
  - JWT authentication support
  - Input validation and security hardening

- **Clustering**
  - OpenRaft integration for distributed consensus
  - Automatic node discovery and leader election
  - Data replication across cluster nodes

- **Schema Management**
  - Auto-generated database schema from declarative models
  - Manual migration mode with approval workflow
  - Disk persistence for schema changes

- **Developer Experience**
  - Comprehensive mdBook documentation
  - Production-ready examples (SCC2 server, Raft replication, RBAC SSO)
  - BDD testing with Cucumber
  - Taskfile-based build system

### Dependencies

- Upgraded reqwest from 0.12 to 0.13

[Unreleased]: https://github.com/lithair/lithair/compare/v0.1.3...HEAD
[0.1.3]: https://github.com/lithair/lithair/compare/v0.1.1...v0.1.3
[0.1.1]: https://github.com/lithair/lithair/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/lithair/lithair/releases/tag/v0.1.0
