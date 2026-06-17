# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **`lithair_core::prelude`** — the recommended one-line application import
  (`use lithair_core::prelude::*;`). Re-exports the stable user-facing
  surface: `LithairServer`, the derive/attribute macros, the custom-route
  types (`Method`, `RouteRequest`, `RouteResponse`, `StatusCode`,
  `response`), and the core RBAC types. This is the canonical stable v1.0
  application surface (gate G3 groundwork).

### Fixed

- **`lithair new` generated a project that did not compile.** The scaffold
  templates referenced `lithair_core::prelude::*` (which did not exist),
  pinned `lithair-core = "0.1"` (the current line is `0.14`), and emitted a
  custom-route handler written against an obsolete signature plus imports
  (`http`, `hyper`, `Full<Bytes>`) for crates the scaffold did not depend
  on. The templates now use the new prelude, pin `lithair-core` to this
  CLI's own version (`CARGO_PKG_VERSION`, so scaffolds track the release),
  carry only the dependencies they use, and emit a handler matching the
  current `with_route(RouteRequest) -> Result<RouteResponse>` contract. A
  new test (`scaffold_targets_current_version_and_api`) guards against this
  rot, and a freshly generated project was verified to compile end-to-end.

## [0.14.0] - 2026-06-16

This release closes the three "lifecycle of an installed product" gates on
the [road to v1.0](docs/roadmap/v1.0.md) — backup (G7), frontend updates
(G8), and cluster (G1, documented envelope) — so an operator can update
content, update the frontend, and trust backups without restarting Lithair
except on a binary change.

### Added

- **Frontend lifecycle admin API** (issue #134, PR #138). New
  `/_admin/frontend/*` endpoints (gated by `with_data_admin()`): `GET
  /_admin/frontend` lists every configured frontend with a comparable
  `sha256:` version; `GET /_admin/frontend/{key}` inspects one frontend's
  config, version and asset manifest; `POST /_admin/frontend/{key}/reload`
  reloads a single vhost (or `POST /_admin/frontend/reload` for all)
  atomically in memory. Reloads build the new asset set and swap it under a
  write lock, are node-local (correctly exempt from Raft write-redirection
  so a follower reloads its own assets), and touch neither the event store
  nor other vhosts. Covers both the path-prefix (`with_frontend_at`) and
  per-vhost (`with_vhost`) storage paths. The version is O(1) cached and
  recomputed only at load/reload. This turns "redeploy = restart everything"
  into "redeploy = one API call."

- **`lithair verify <data-dir>` CLI** (issue #133, PR #136). Opens an event
  store offline and runs the hash-chain verification an operator needs to
  check a restored backup by hand: exit `0` (valid), `1` (chain broken),
  `2` (could not open), with the full verification report on stdout.

- **Backup/restore end-to-end drill** (issue #133, PR #136).
  `backup_restore_drill_test.rs` backs up, wipes, restores to a fresh state,
  and asserts field-level identity, a valid hash chain, and a
  chain-continuing write after restore — in both JSON and binary log modes,
  plus the torn-tail case. A documented backup that has never been restored
  is a guess; this makes it a tested path.

### Fixed

- **On-disk format — binary log mode (`LT_ENABLE_BINARY`) is now usable**
  (PR #136). Proving the backup drill surfaced two latent bugs that made
  binary log mode non-functional in ≤0.13 (the default JSON mode was never
  affected):
  - The genesis event (whose `previous_hash` is `None`) was dropped on
    bincode replay because `EventEnvelope`'s hash fields used
    `skip_serializing_if`, which bincode encodes as an absent field rather
    than `None`. Removed `skip_serializing_if` (kept `serde(default)` so
    JSON stores written by earlier versions still read back unchanged).
  - Reopening a binary log failed because `EventStore::new` applied the
    JSON line reader to bincode frames: the `LT_ENABLE_BINARY` env var was
    merged into the struct field but not into the *initial* read path. Both
    the initial reads and the stored mode now use one resolved
    `effective_binary_mode`.

  Because binary mode could not produce a round-trippable store before
  v0.14, there is no functional pre-0.14 binary log to break; JSON-mode
  stores are byte-compatible. Operators relying on the on-disk format
  promise should note these are format-sensitive fixes.

### Docs

- **Cluster operations runbook + G1 envelope** (issue #104, PR #127). The
  cluster operating envelope (single-leader writes at a measured
  ~210–240 ops/s ceiling, static lowest-ID election, fixed membership) and
  its procedures are documented in `docs/operations/cluster.md`; G1 marked
  resolved (production-stable within that envelope).
- **v1.0 roadmap, deprecation policy, governance** (PR #125): `docs/roadmap/v1.0.md`
  (the three compatibility surfaces and the v1.0 gates), the
  deprecate-in-N / remove-in-N+2 [deprecation policy](docs/policy/deprecation.md),
  and `CODE_OF_CONDUCT.md`.
- Roadmap gates G7 (backup proven) and G8 (frontend lifecycle API) added
  and marked resolved (PRs #135/#137/#139).

## [0.13.0] - 2026-06-11

### Added

- **Graceful shutdown hook** (issue #112, PRs #114). New
  `LithairServer::serve_with_graceful_shutdown(shutdown: impl Future)`
  (also on the builder): the accept loop selects on the shutdown future
  (`biased` — a pending connection in the backlog can no longer win over
  a ready shutdown signal), the listening socket is closed before a 5s
  drain window for in-flight connections, then the call returns so the
  application can join its own background workers. `serve()` delegates
  with `std::future::pending()` — existing callers are byte-for-byte
  unchanged. Precise per-connection joins and internal-task draining are
  tracked in #115.

- **Tracing foundation** (issue #107 phase 1, PR #118). The default
  logger init is now a `tracing-subscriber` registry (an `EnvFilter`
  honoring `RUST_LOG`, same `error` fallback as the previous env_logger)
  plus a `tracing-log` bridge: all ~550 existing `log::*` call sites
  flow through unchanged, and a custom logger installed first (e.g.
  `RaftstoneLogger`) still wins — same try-semantics as before.
  Surgical spans on the critical paths: `http_request` (method, path,
  request_id), `event_append`, `snapshot_save`/`snapshot_load`,
  `retention_evict`, `event_replay`.

- **`X-Request-ID` correlation** (issue #107 phase 1, PR #118). Every
  response carries an `X-Request-ID` header: inbound values are honored
  when they are 1–128 bytes of visible ASCII (anything else is replaced
  — header-injection hygiene), otherwise a UUID v4 is minted. The id is
  recorded on the `http_request` span for log/trace correlation.

- **Opt-in OpenTelemetry OTLP exporter** (issue #107 phase 2, PR #119).
  New `otel` cargo feature (off by default — default builds are
  unchanged and compile none of it). With the feature and
  `LT_OTEL_ENDPOINT` set (e.g. `http://collector:4317`), spans export
  over OTLP/gRPC; `LT_OTEL_SERVICE_NAME` sets the resource name
  (default `lithair`). The batch exporter is flushed during graceful
  shutdown (bounded). Setting the env without the feature logs an
  explicit warning instead of failing silently. See
  `docs/operations/observability.md`.

- **Operations runbooks** (issue #106, PRs #110/#111/#113/#116/#117).
  `Dockerfile` + `docker-compose.yml` (bridge networking, named volume,
  non-root, healthcheck), systemd unit (`StateDirectory`, hardening) and
  Kubernetes manifests (single-replica + `Recreate` by design,
  startupProbe sized for event-store replay), plus four guides under
  `docs/operations/`: capacity planning (RAM/disk/CPU model incl. the
  retention-bounded formula), backup/restore/PITR (logical export vs
  physical event-store copy, torn-tail semantics for both JSON and
  binary log modes), and the version-upgrade playbook (additive vs
  breaking changes, `#[lithair_model]` requirement for `db(default)`
  compat, rollback window).

- **Production-realistic BDD coverage** (issue #105, PR #120). Four new
  models (`BddInvoice`, `BddDocument`, `BddUser`, `BddOrder`) exercise
  `rust_decimal` money, >100KB blobs under byte-budget retention,
  timezone-aware timestamps, serde enums, `Option` fields, nested
  structs and FK auto-join end-to-end — 25 scenarios. This coverage
  found both bugs fixed below.

- **Governance docs** (issue #108, PR #109): `CONTRIBUTING.md`
  (dev setup, `cidx run code` gate, trunk-based PR flow) and
  `SECURITY.md` (private disclosure via GitHub Security Advisories,
  supported-versions policy).

- **Example: configurable bind** (PR #110). `examples/01-hello-world`
  reads `HOST`/`PORT` from env (defaults unchanged: `127.0.0.1:8080`) —
  the pattern container deployments need.

### Fixed

- **`#[retention(max_mb = N)]` or `memory = "30d"` alone were silently
  ignored** (issue #121, PR #123). Both retention gates
  (`DeclarativeHttpHandler::new` and `Scc2Engine::enable_retention`)
  only activated the retention layer when a count was configured, so a
  budget-only or duration-only annotation capped nothing — a silent
  misconfiguration for v0.12 adopters. The gate predicate is now
  `RetentionConfig::is_configured()` (any of the three modes), defined
  in exactly one place and delegated to by `has_retention_limit()` and
  `RetentionLayer::is_active()`.

- **`#[db(fk = "table")]` was dropped by the macro parser** (issue
  #122, PR #124). `parse_db_attributes` walked TokenTrees one by one,
  so the pair-shaped `fk = "..."` never matched and `fk_collection` was
  `None` at runtime — the AutoJoiner could not expand the relation.
  Converted to the same string-level comma-split parsing as the issue
  #75 fix; flags and the `default = X` block are regression-pinned.
  Third silently-dropped-attribute bug in this parser — making unknown
  tokens a compile error is a candidate follow-up.

- **Warm-entry permission bypass through the public list seam**
  (PR #120 review). Extracting `list_response_json` from `handle_list`
  made it callable with explicit `user_perms` while the warm-entry gate
  only checked handler-configured extractors — a direct caller could
  get hot items permission-filtered but evicted pinned data appended
  unfiltered. Explicit perms now imply filtering is active.

### Changed

- `lithair-macros` bumped to 0.13 (the `#[db(fk)]` parser fix changes
  generated specs — every model deriving `DeclarativeModel` must be
  recompiled, which a `cargo update`/version bump does naturally).
- `env_logger` removed from `lithair-core` dependencies (replaced by
  the tracing stack; `RUST_LOG` behavior preserved, `RUST_LOG_STYLE`
  no longer honored — color follows tty detection).
- `.serena/` and `.cidx/` tool runtime state untracked (PR #103).

## [0.12.0] - 2026-05-29

### Added

- **`#[retention(memory = N)]` and `#[pinned]` annotations for memory/disk
  tiering** (issues #96, #97, #98). Lithair is memory-first but not
  memory-only. The new declarative annotations let a model declare how many
  items stay fully projected in RAM and which fields survive eviction:

  ```rust
  #[derive(DeclarativeModel)]
  #[retention(memory = 1000)]      // keep last 1000 items fully in memory
  pub struct Email {
      #[pinned] pub from: String,   // always in RAM, even after eviction
      #[pinned] pub subject: String,
      pub body: String,             // evicted with the rest; reloaded on demand
  }
  ```

  Three retention dimensions are supported and can be combined:

  - `memory = N` — count-based: cap on items fully in memory
  - `memory = "30d"` — duration-based: evict items older than the cutoff
    (s/m/h/d/w/y suffixes)
  - `max_mb = 512` — budget-based: evict oldest until total serialized
    hot-storage size ≤ budget

  Evicted items keep their pinned fields in a lightweight warm map for
  fast listing and filtering; non-pinned fields are reloaded from the event
  store on demand via reverse-scan with `aggregate_id` short-circuit (no
  full payload deserialization for mismatched events). The system stays
  100% event-sourced — no external DB, no separate CRUD paradigm.

- **Runtime retention overrides via environment variables**. Each
  dimension can be tuned at deploy time without recompiling:

  - `LT_<MODEL>_MEMORY_RETENTION=<count>`
  - `LT_<MODEL>_MEMORY_DURATION=<duration>` (e.g. `30d`)
  - `LT_<MODEL>_MEMORY_MAX_MB=<megabytes>`

  Model name is the last segment of `std::any::type_name::<T>()`,
  sanitized to alphanumeric + underscore and uppercased.

- **`/metrics` endpoint** for Prometheus-compatible monitoring, exposing
  per-model storage stats (item count, `.raftlog` size, snapshot size).

- **BDD coverage for the retention system**. New
  `cucumber-tests/features/persistence/retention.feature` with 22 core
  scenarios + step definitions in `cucumber-tests/src/features/steps/retention_steps.rs`.
  Three prioritized scenarios pass end-to-end via direct
  `DeclarativeHttpHandler` invocation (eviction-on-overflow, on-demand load
  from event store, env override wins over annotation).

### Fixed

- **Update of an evicted item no longer duplicates it** (PR #101 review,
  Gemini critical). When a PUT/PATCH/replication targets an item currently
  in the warm map, the warm entry is now cleared BEFORE the hot insert,
  preserving the exactly-one-place invariant. Previously the item briefly
  existed in both maps and `handle_list` emitted it twice.

- **`Scc2Engine::update_entry_volatile` no longer desyncs warm/hot under
  contention** (PR #101 review, CodeRabbit critical). `promote_from_warm`
  was called before `try_entry`, so a failed acquisition cleared the warm
  entry without registering anything in the hot map — silent data loss.
  The call now happens inside the success branches, paired with
  `maybe_evict`'s `track_insert` so the warm clear and hot registration
  are atomic from the caller's perspective.

- **`limit = 0` retention now correctly evicts the inserted item**
  (PR #96 review, CodeRabbit major). The previous `oldest == key`
  short-circuit kept exactly one item even at zero capacity.

- **Macro now declares `retention` and `pinned` as derive helper
  attributes** (PR #99 macro regression test). Without this, the
  compiler silently ignored both attributes on user models — the
  generated `RetentionAware::retention_config()` returned defaults
  regardless of what the user wrote.

### Changed

- `lithair-macros` bumped to 0.12 (helper-attribute declaration fix
  requires recompilation of every model deriving `DeclarativeModel`).

## [0.11.0] - 2026-05-24

### Fixed

- **SSE-over-HTTP now streams incrementally instead of buffering** (issue #93).
  The route dispatch infrastructure (`app/builder.rs` `with_handler` routes and
  `app/mod.rs` `handle_model_request`) previously converted every handler's
  `BoxBody` response into `Full<Bytes>` via `body.collect().await`, which
  fully buffered the response before sending it to hyper. For infinite SSE
  streams, this meant a JS `EventSource` subscriber on `/api/{model}/stream`
  received zero events until the connection closed. The `RouteResponse` type
  has been migrated from `Response<Full<Bytes>>` to
  `Response<BoxBody<Bytes, Infallible>>`, and the `body.collect()` calls have
  been removed. The SSE handler's `StreamBody` now passes through to hyper
  directly, delivering each event incrementally. Non-SSE consumers see
  identical behavior (their `Full<Bytes>` bodies are boxed transparently).

## [0.10.0] - 2026-05-23

### Added

- **`LithairServerBuilder::with_handler` now wires the builder-level SSE
  broadcaster onto the externally-constructed handler at `serve()` time**
  (issue #91). Pre-fix, only the `with_model` factory path called
  `set_sse_broadcaster` (`app/mod.rs:776`); handlers registered through
  `with_handler` started with no broadcaster regardless of whether
  `.with_sse(true)` was called on the builder. Net effect on the v0.9.0
  ergonomic path: the issue #89 fix (`apply_replicated_*` broadcasts) was
  a no-op for `with_model_ref`-registered models, and `GET /api/{model}/stream`
  returned `404 SSE not enabled`. The wiring now uses the same deferred-
  applier mechanism as the issue #86 session gate
  (`external_handler_sse_wirings`), so registration ordering between
  `with_handler` / `with_sse` is irrelevant. Same opt-in semantics:
  consumers who never call `.with_sse(true)` see byte-for-byte identical
  behavior to v0.9.1 — no broadcaster wired, `/stream` still 404s.

  Because `with_model_ref` delegates to `with_handler`, this also closes
  the LensMail Phase 4 gap end-to-end on the v0.9.0 documented path:

  ```rust
  let (builder, mail_handler) = LithairServer::new()
      .with_sse(true)
      .with_sessions(sm)
      .with_models_require_session(true)
      .with_model_ref::<Mail>("./data/mails", "/api/mails")
      .await?;
  // mail_handler.apply_replicated_item(...) now broadcasts on
  // /api/mails/stream as expected.
  ```

  Regression tests:
  `lithair-core/tests/issue_91_with_handler_sse_wiring_test.rs` — covers
  the wiring for `with_handler`, the same path via `with_model_ref`,
  the HTTP `/stream` route surface (no longer 404), and a backward-compat
  guard for SSE-off.

- **`DeclarativeHttpHandler::sse_broadcaster()`** — read-only accessor
  returning the installed `Arc<SseEventBroadcaster>` if any (`None` until
  wired). Useful for in-process consumers holding a programmatic handle
  (via `with_model_ref` or `with_handler`) that want to subscribe to the
  same per-model channel the framework's `/api/{model}/stream` route reads
  from, without going through HTTP.

### Changed

- **`DeclarativeHttpHandler<T>::sse_broadcaster` field** is now
  `OnceLock<Arc<SseEventBroadcaster>>` instead of
  `Option<Arc<SseEventBroadcaster>>` (issue #91 — interior mutability).
  Field is `pub(crate)`; only crate-internal call sites observe the type
  change. `with_sse_broadcaster(self, ...)` keeps the same public
  signature (returns `Self`) — only the internal storage primitive
  differs. First-call-wins semantics: subsequent installs silently no-op,
  matching the production lifecycle (one broadcaster per server,
  installed at `serve()` time, never replaced).

## [0.9.1] - 2026-05-23

### Fixed

- **`apply_replicated_item` / `apply_replicated_update` / `apply_replicated_delete`
  now broadcast SSE events** (issue #89). Pre-fix, the three programmatic /
  replicated apply methods on `DeclarativeHttpHandler<T>` updated storage
  and the event store but never called `broadcast_sse(...)` — so a
  subscriber on `/api/{model}/stream` only saw the initial `connected`
  event plus heartbeats, never the actual change events when writes came
  from the `with_model_ref` handle (added in v0.9.0) or from the cluster
  replication path. The methods now emit `"create"`, `"update"`, and
  `"delete"` respectively — the same operation names the HTTP CRUD path
  uses (`handle_create`, `handle_put`, `handle_delete`) so consumers can't
  tell write origin apart from the stream. The idempotent no-op branch
  in `apply_replicated_delete` (key not present) does NOT broadcast.

  Regression test: `lithair-core/tests/issue_89_replicated_sse_broadcast_test.rs`.

  Behavior is additive; no breaking API changes. Surfaced by the LensMail v2 IMAP
  sync worker which writes mails through `apply_replicated_item` from a
  background task and expected `/api/mails/stream` subscribers to see the
  inserts in real time.

### Added

- **`SseEventBroadcaster::subscribe(model_name)`** — public method
  returning a `tokio::sync::broadcast::Receiver<ModelChangeEvent>` for a
  given model channel. Used internally by the `/api/{model}/stream` SSE
  route and exposed publicly so consumers (and tests) can wire non-HTTP
  subscribers to the same channel. Channel capacity matches the
  existing route (1000 events; slow consumers receive `Lagged(n)` on
  `recv()`).

## [0.9.0] - 2026-05-21

### Added

- **`LithairServerBuilder::with_model_ref::<T>(data_path, base_path)`**
  (issue #85). A new builder method that registers a model for
  auto-generated CRUD **and** returns the `Arc<DeclarativeHttpHandler<T>>`
  to the caller, so background workers / OAuth callbacks / scheduled jobs
  can drive the model programmatically (via `apply_replicated_item`,
  `apply_replicated_update`, `apply_replicated_delete`) **without** giving
  up the session gate that `with_models_require_session(true)` provides.

  This is the missing "both at once" path between `with_model::<T>(...)`
  (gated CRUD, no handle) and `with_handler(arc, ...)` (handle, no
  auto-wired session store). Unlike `with_handler`, the builder
  constructs the handler internally, so the builder-level session store
  is wired onto the handler automatically — positive-path session
  validation (valid token → 200) works out of the box.

  The method is `async` (because `DeclarativeHttpHandler::new_with_replay`
  performs event-log replay I/O) and returns a tuple, so the chain
  breaks at this method:

  ```rust
  let (builder, mail_handler) = LithairServer::new()
      .with_sessions(session_manager)
      .with_models_require_session(true)
      .with_model_ref::<Mail>("./data/mails", "/api/mails")
      .await?;

  // mail_handler.apply_replicated_item(...).await — usable anywhere
  builder.serve().await?;
  ```

  Motivating use case: LensMail's Gmail OAuth + IMAP sync worker (see
  issue #85). Pre-#85, there was no API that gave both
  (1) session-gated auto-CRUD and (2) a programmatic handle for writes.

  Regression coverage: `lithair-core/tests/with_model_ref_test.rs` —
  4 tests covering programmatic write visibility through gated read,
  gate fires on unauthenticated read, programmatic writes do not
  bypass the read-side gate, and backward-compat with the flag off.

### Fixed

- **`with_handler` now respects `with_models_require_session(true)`** (issue
  #86, PR A of the #85/#86 pair). Pre-fix, switching a model registration
  from `with_model::<T>(...)` to `with_handler(handler, base_path)` to gain
  programmatic access to the handler silently bypassed the session gate
  even when the operator had explicitly opted into
  `with_models_require_session(true)`. Net effect was a security regression
  on the easiest "graduate to programmatic access" path:
  `GET /api/{model}` without an Authorization header returned 200 with the
  full collection instead of 401.

  Root cause: `with_handler` registered its CRUD routes via raw
  `with_route(...)` calls and never went through the `model_infos` pipeline
  in `LithairServer::serve()` where the `require_session` flag is applied.

  Fix: `with_handler` now records a deferred gate-applier closure with a
  cloned `Arc<DeclarativeHttpHandler<T>>`. At `serve()` time, after every
  builder method has run, the closures are invoked and flip the flag
  through interior mutability. To support this without breaking the Arc
  semantics that `with_handler`'s callers rely on,
  `DeclarativeHttpHandler::require_session` was changed from `bool` to
  `AtomicBool` and `set_require_session` now takes `&self` instead of
  `&mut self`. This is an internal change — public API is unchanged.

  Note: this fix does NOT auto-wire the builder-level session store onto
  the externally-constructed handler — that responsibility stays with the
  caller (call `handler.with_session_store(...)` before passing to
  `with_handler`). Without a session store, the gate fails closed (every
  request returns 401), which is the safe direction.

  Regression coverage: `lithair-core/tests/with_handler_session_gate_test.rs`
  pins the issue body's exact repro (`with_sessions` +
  `with_models_require_session(true)` + `with_handler` → 401 without auth)
  plus backward-compat (flag off → 200) and a POST gate test.

## [0.8.0] - 2026-05-19

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
  that periodically checks the event count and calls the new
  `ModelHandler::compact()` method. This method is responsible for
  atomically creating a state snapshot before truncating the event log,
  preventing data loss. The underlying compaction primitives in
  `SnapshotStore` and `EventStore` are unchanged — this is a thin opt-in
  driver on top. Lifecycle matches the existing background-flusher pattern
  (`DeclarativeHttpHandler::new`): spawned and forgotten, aborted on
  runtime shutdown.

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

[Unreleased]: https://github.com/lithair/lithair/compare/v0.14.0...HEAD
[0.14.0]: https://github.com/lithair/lithair/compare/v0.13.0...v0.14.0
[0.1.3]: https://github.com/lithair/lithair/compare/v0.1.1...v0.1.3
[0.1.1]: https://github.com/lithair/lithair/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/lithair/lithair/releases/tag/v0.1.0
