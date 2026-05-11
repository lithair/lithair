# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
