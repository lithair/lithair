# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- **Auto-compaction data-loss bug** (PR #84 follow-up, Gemini review).
  The initial #69 implementation called `EventStore::truncate_events()`
  from the server-side spawn loop without first writing a state
  snapshot — in an event-sourced system, the events ARE the state, so
  truncating without a snapshot means the next restart sees an empty
  log and reconstructs empty storage. Permanent data loss for any model
  that crossed its threshold. Three changes:

  - New `ModelHandler::compact()` trait method (default: no-op). The
    `DeclarativeModelHandler` impl delegates to a new
    `DeclarativeHttpHandler::compact()` that serializes the storage
    `HashMap` to JSON, writes it via `EventStore::save_snapshot`, then
    truncates the events log — atomically under the event-store write
    lock.
  - `DeclarativeHttpHandler::replay_events()` now loads any persisted
    snapshot before replaying events, so a restart after compaction
    reconstructs full state.
  - The server-side auto-compaction task in `LithairServer::serve()`
    now calls `handler.compact().await` instead of touching
    `truncate_events()` directly. `EventStore::truncate_events()`
    remains public but should never be called without a prior snapshot
    — the framework no longer does so internally.

  Also addressed in the same review:

  - `env_logger::try_init()` now runs **before** the auto-compaction
    spawn loop. Previously any `log::info!` emitted by the spawned
    tasks before the logger was initialized routed to the fallback.
  - The auto-compaction ticker now uses
    `MissedTickBehavior::Skip` (was the default `Burst`). Under load,
    missed check intervals no longer cause a burst of back-to-back
    compaction checks.

  Regression coverage: `lithair-core/tests/auto_compaction_test.rs`
  gains `compact_then_reopen_preserves_state` (writes 12 items,
  compacts, drops the handler, reopens against the same data dir,
  asserts all 12 items are visible) and
  `compact_then_append_then_reopen_replays_both` (snapshot + post-
  snapshot events are both replayed correctly).

### Added

- **Per-model storage and memory stats** (closes #72). Two new surfaces give
  operators a read-only view of each registered model's footprint, motivated
  by LensMail v2 capacity planning:

  - `GET /_admin/data/models/{name}/_stats` — JSON with `item_count`,
    `approx_ram_bytes`, `raftlog_size_bytes`, and two compaction fields
    currently returning `null` (gated on #69 wiring). 404 when the model
    name doesn't exist. Requires `with_data_admin()` like the sibling
    `/_admin/data/models/*` endpoints.
  - `GET /metrics` — extended with three new Prometheus gauges per
    registered model: `lithair_model_items{model="..."}`,
    `lithair_model_ram_bytes{model="..."}`,
    `lithair_model_raftlog_bytes{model="..."}`.

  `approx_ram_bytes` is a sample-based estimate (up to 16 items
  JSON-serialized, averaged, multiplied by live count) — useful for order-
  of-magnitude capacity planning, not billing. See `ModelStats` docs for
  the methodology and biases. Custom `ModelHandler` impls can override
  `get_stats` with a cheaper sizing primitive if they have one.

- **Builder-driven auto-compaction of `.raftlog`** (closes #69). New opt-in
  `LithairServerBuilder` flag that periodically truncates each registered
  model's event log when its event count crosses a configurable threshold.
  Motivated by LensMail v2: every full-snapshot mutation (e.g. mark-as-read
  toggles) appends a full object dump to `.raftlog`, accumulating storage
  consumers had to manage with their own cron + monitoring loop.

  ```rust
  use std::time::Duration;
  LithairServer::new()
      .with_model::<Mail>("./data/mails", "/api/mails")
      .with_auto_compaction(10_000, Duration::from_secs(300))
      .serve()
      .await?;
  ```

  Behind the scenes, `serve()` spawns one tokio task per registered model
  that wraps the existing `EventStore::event_count()` + `truncate_events()`
  primitives. The compaction APIs in `SnapshotStore` and `EventStore` are
  unchanged — this is a thin opt-in driver on top. Lifecycle matches the
  existing background-flusher pattern (`DeclarativeHttpHandler::new`):
  spawned and forgotten, aborted on runtime shutdown.

  New public API: `engine::AutoCompactionConfig`, constants
  `DEFAULT_AUTO_COMPACTION_CHECK_INTERVAL`, `DEFAULT_SNAPSHOT_THRESHOLD`
  (re-exported from `engine::snapshot`), and `ModelHandler::event_store_arc()`
  trait method (default impl returns `None`; `DeclarativeModelHandler`
  returns the underlying store). Default is **disabled** — calling
  `with_auto_compaction(...)` (or `with_auto_compaction_config(...)`) opts
  in; not calling it preserves current behavior for every existing consumer.

  A threshold of `0` is rejected at builder time —
  `with_auto_compaction(0, ...)` panics, and `AutoCompactionConfig::new`
  returns `None`. Custom `ModelHandler` impls that don't back themselves
  with an `EventStore` are silently skipped (the trait method's `None`
  default).

## [0.7.1] - 2026-05-17

### Fixed

- **Double-Arc footgun in `with_models_require_session(true)`** (closes #80).
  Surfaced by LensMail hours after v0.7.0 published. When a consumer wrote
  the obvious `SessionManager::new(Arc::new(store))`, the compiler accepted
  it (because `impl SessionStore for Arc<T>` is a blanket), producing a
  `SessionManager<Arc<PersistentSessionStore>>` whose stored shape didn't
  match either `Arc` variant tried by `has_valid_session`. The gate
  silently rejected 100% of authenticated requests with HTTP 401 — even
  ones with valid tokens that `/auth/me` accepted. Two-part fix (PR #81):

  - **A — split `SessionManager` constructor.** `new(S)` keeps its
    by-value semantics; new `from_arc(Arc<S>)` is the explicit "I already
    have an Arc" path. Same split for `with_config` / `from_arc_with_config`.
    Consumer code that previously wrote `SessionManager::new(arc_store)`
    should switch to `SessionManager::from_arc(arc_store)`.
  - **B — fail-fast at boot.** `LithairServer::serve()` now does a
    one-time downcast of the registered session store when
    `models_require_session = true && any simple-CRUD model is registered`.
    If the shape is unrecognized — or if the flag is on with no store
    registered at all — `serve()` returns an error with a diagnostic
    naming the actual `TypeId` and pointing at `from_arc`, instead of
    silently 401-ing every request.

  Defense-in-depth: A makes the bad shape construction explicit (and
  the new doc-comments reference #80 directly), B catches anything that
  still slips through (e.g., a future store type added without updating
  the downcast).

- **Example `examples/06-auth-sessions` was carrying the buggy pattern.**
  Switched from `SessionManager::new(arc_store)` to
  `SessionManager::from_arc(arc_store)` with an explanatory comment.
  LensMail likely learned the bad pattern from this example — fixing the
  example removes the pedagogical regression vector.

### Changed (internal, no API change)

- New `pub(crate) RecognizedSessionStore` enum (`lithair-core/src/session/mod.rs`)
  centralizes the set of session-store shapes the framework recognizes.
  Used by both the gate's runtime check (`has_valid_session`) and the
  boot-time fail-fast in `serve()`. Single source of truth — adding a
  new shape requires one edit, not two. Suggested by CodeRabbit review
  on #79, applied as a follow-up commit on #81.

## [0.7.0] - 2026-05-17

### Added

- **`LithairServer::with_models_require_session(bool)`** — new builder method
  that gates all auto-generated `/api/{model}` endpoints. When set to `true`,
  any request without a valid active session is rejected with **HTTP 401**.
  Default is `false` (fully backward-compatible). Closes #78 (consumer-driven
  request from LensMail).

  ```rust
  LithairServer::new()
      .with_sessions(SessionManager::new(store))
      .with_models_require_session(true)   // ← new
      .with_model::<Account>("./data/accounts", "/api/accounts")
      .serve()
      .await?;
  ```

  Targets the binary "session required" policy without the ceremony of full
  RBAC (`with_model_full` + per-field `#[permission]` annotations). The flag
  intentionally exempts `with_model_full` registrations — they already have
  RBAC and don't need this. Session lookup handles both `Bearer <token>`
  headers (case-insensitive) and `session_token=` cookies; `OPTIONS`
  preflight requests are exempted.

- **`lithair_macros::__private` module** with `#[doc(hidden)] pub use` for
  `serde_json`, `clap`, `tokio`, `anyhow`. Consumers using `#[derive(DeclarativeModel)]`
  no longer need to declare these as direct dependencies in their own
  `Cargo.toml`. Closes #66.

### Fixed

- **`#[http(validate = "non_empty")]` and other field validators are now
  enforced on POST.** Closes #75. Root cause was a macro parser bug:
  `parse_http_attributes` walked `tokens.into_iter()` `TokenTree` by
  `TokenTree`, matching on the `validate` `Ident` alone — `extract_string_value`
  then returned `None` because the single token had no quotes, so
  `attrs.validation` stayed empty for every field and the generated
  `HttpExposable::validate()` body was effectively a no-op. Fixed by
  rewriting `parse_http_attributes` to use `tokens.to_string() + split(',')`,
  matching the pattern already used by `parse_firewall_attributes`,
  `parse_model_http_attributes`, and `parse_server_attributes` in the same
  file (PR #76). `#[db(unique)]` was unaffected (single-token, no key=value
  to reassemble) — which is why uniqueness worked while validators didn't.
  7 behavior tests + 1 token-level regression test added.

- **`with_sessions(...)` + `with_model::<T>(...)` now actually threads the
  session store into the auto-generated handler.** Pre-existing latent gap:
  only `with_model_full` propagated the session store via
  `set_session_store_any`; the plain `with_model` path produced a handler
  with `session_store: None`. Surfaced and fixed while implementing #78.

- **Session-store downcast accepts both shapes.** Pre-existing
  `extract_role_from_request` only downcast to `Arc<PersistentSessionStore>`,
  but `with_sessions(...)` stores `Arc<SessionManager<S>>`. RBAC was
  silently broken for `with_sessions` users. The new `has_valid_session`
  helper accepts both shapes (PR #79 bonus fix).

### Documentation

- README: `Distributed consensus` now states it's opt-in via the `cluster`
  feature; single-node deployments don't pay for it (PR #74).
- README: `/observe/metrics` reframed as planned — the route is not yet
  registered, only placeholder utilities exist (PR #74).

### CI / Infrastructure

- `.github/workflows/cidx.yml` regenerated with Node 24-compatible action
  versions (`actions/checkout@v6`, `actions/setup-go@v6`,
  `actions/upload-artifact@v7`, `actions/download-artifact@v8`,
  `go-version: 1.26`). Closes downstream impact of cidx-org/cidx#138.
  Resolves GitHub's Node 20 deprecation deadline (2026-06-02).

## [0.6.1] - 2026-05-13

### Security

- Resolved 11 transitive `cargo-audit` advisories surfaced by `cidx run
  security` (closes #54). All fixed via `cargo update`; no direct-dep
  Cargo.toml changes required:
  - `aws-lc-sys` 0.35.0 → 0.41.0 — RUSTSEC-2026-0044/0045/0046/0047/0048
  - `bytes` 1.11.0 → 1.11.1 — RUSTSEC-2026-0007
  - `quinn-proto` 0.11.13 → 0.11.14 — RUSTSEC-2026-0037
  - `rustls-webpki` 0.103.8 → 0.103.13 — RUSTSEC-2026-0049/0098/0099/0104
- Added `.gitleaks.toml` with an allowlist for `jwt_token_*_authenticated`
  documentation placeholders in API examples (10 false positives, all in
  `docs/guides/crud-integration.md` and `docs/reference/api-reference.md`).
- Re-enabled the `security` phase in the cidx CI pipeline (`cidx.toml` and
  regenerated `.github/workflows/cidx.yml`). Two transitive unmaintained
  warnings remain (`bincode` 2.x and `rustls-pemfile` 2.x) with no
  published replacement — kept visible in `cargo audit` output without
  blocking CI.

## [0.6.0] - 2026-05-12

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
