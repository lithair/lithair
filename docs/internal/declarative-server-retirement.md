# DeclarativeServer Retirement — Phase 1 Inventory

> Working spec for retiring `DeclarativeServer` in favor of `LithairServer`.
> Tracks issue [#42](https://github.com/lithair/lithair/issues/42).
> Authored 2026-05-04 as the deliverable of phase 1 (this PR).

## Scope and gating

This is the per-callsite inventory of `DeclarativeServer` references inside the framework, produced by re-reading each module on `main` and verifying claims against source (per project-tracker #6 protocol).

**Phase 1 gate**: this PR ships the inventory and the single simplest migration. It does **not** add `#[deprecated]` to the `DeclarativeServer` struct itself — that is gated on PR #41 merging plus parity verification (issue #42 acceptance criteria).

## Surprise: the 5 listed callsites are doc-only

Issue #42 lists 5 modules that "reference" `DeclarativeServer`:

- `lithair-core/src/rbac/mod.rs`
- `lithair-core/src/session/mod.rs`
- `lithair-core/src/consensus/mod.rs` (non-deprecated paths)
- `lithair-core/src/http/utils.rs`
- `lithair-core/src/app/builder.rs`

Re-reading each file on `main` (verified via `grep -nE "use .*DeclarativeServer|DeclarativeServer::|DeclarativeServer<"` across `lithair-core/src/` and `lithair-macros/src/`):

**Each of the 5 modules references `DeclarativeServer` in doc comments / module-level rustdoc only — never in code.** No `use` import, no method call, no type parameter. The actual code-level dependencies on `DeclarativeServer` live in two other places not listed in the issue body:

1. `lithair-core/src/http/declarative_server.rs` — the type definition itself (expected; this is what we're retiring)
2. `lithair-macros/src/declarative_simple.rs:1380` — generated code calls `lithair_core::http::DeclarativeServer::<#name>::new(...)?.serve()` (real code dependency, not in the issue's list)
3. `lithair-core/src/http/mod.rs:72` — public re-export `pub use declarative_server::{DeclarativeServer, ...}`

This means **issue #42's per-module migration plan does not need code rewrites in those 5 modules** — only stale doc strings to update. The real migration work for retiring `DeclarativeServer` is concentrated in:

- The macro-generated code path (`lithair-macros/src/declarative_simple.rs`)
- The public re-export (`lithair-core/src/http/mod.rs`)
- The `declarative_server.rs` file itself

This is a substantively different shape than the issue body suggested. Recommendation in the "What's left" section.

## Per-callsite breakdown

### 1. `lithair-core/src/rbac/mod.rs`

- **What it imports/uses from DeclarativeServer**: nothing in code. Single mention at line 11: `//! - Automatic middleware integration with DeclarativeServer`.
- **Why it depends on it**: it doesn't. The doc comment is stale — `LithairServer` now provides RBAC integration via `with_rbac_config()` (`lithair-core/src/app/builder.rs:433`).
- **Migration approach**: **C / docs-only** — replace `DeclarativeServer` with `LithairServer` in the rustdoc.
- **Migration size estimate**: **S** (1-line change in module rustdoc).
- **Blockers / dependencies on other modules**: none.
- **Recommended PR order**: 3 (after the macro migration, since rustdoc should describe the recommended path).

### 2. `lithair-core/src/session/mod.rs`

- **What it imports/uses from DeclarativeServer**: nothing in code. Single mention at line 13 inside a `no_run` example: `//! use lithair_core::http::DeclarativeServer;`.
- **Why it depends on it**: it doesn't. Doc-only example. Sessions are wired into `LithairServer` via `with_sessions()` (`lithair-core/src/app/builder.rs:218`).
- **Migration approach**: **C / docs-only** — rewrite the `no_run` example to use `LithairServer::new().with_sessions(...)`.
- **Migration size estimate**: **S** (~5-10 line example block).
- **Blockers / dependencies on other modules**: none. The example is `no_run` so it won't break compilation if it's slightly aspirational, but the replacement should still typecheck.
- **Recommended PR order**: 4.

### 3. `lithair-core/src/consensus/mod.rs`

- **What it imports/uses from DeclarativeServer**: nothing in code. Single mention at line 88: `/// Consensus configuration for DeclarativeServer` — and this line is the rustdoc for `ConsensusConfig`, which is itself **already `#[deprecated(since = "0.2.0", note = "Use LithairServer::with_raft_cluster() instead")]`** at line 89.
- **Why it depends on it**: same as rbac — stale doc comment on a struct that's already deprecated. The "non-deprecated paths" the issue mentions don't actually reference `DeclarativeServer` in this file (verified by grep on `main`).
- **Migration approach**: **C / docs-only** — adjust the doc comment to either reference `LithairServer` (since the deprecation note already points there) or just be neutral ("Legacy consensus configuration — see deprecation note").
- **Migration size estimate**: **S** (1-line change).
- **Blockers / dependencies on other modules**: none.
- **Recommended PR order**: 5 (lowest priority since the struct itself is already deprecated; the doc comment is downstream of that).

### 4. `lithair-core/src/http/utils.rs`

- **What it imports/uses from DeclarativeServer**: nothing in code. Single mention at line 286 in the rustdoc for `log_access` reads: `Used by both LithairServer and DeclarativeServer.`
- **Why it depends on it**: it doesn't — the function is reused by both server types because it's pure utility code (logs an HTTP access entry from a `Response` and headers). Once `DeclarativeServer` is retired, the comment becomes wrong.
- **Migration approach**: **C / docs-only** — drop the "and `DeclarativeServer`" half of the sentence, or rephrase to "Used by all Lithair HTTP server backends." For phase 1, simplest path is: keep the comment factually true today by saying "Used by `LithairServer` (and historically `DeclarativeServer`)."
- **Migration size estimate**: **S** (1-line change). **This is the simplest migration of the five.**
- **Blockers / dependencies on other modules**: none. `log_access` is a pure utility with existing call sites; doc-string change is zero-risk.
- **Recommended PR order**: **1 — chosen for this PR.**

### 5. `lithair-core/src/app/builder.rs`

- **What it imports/uses from DeclarativeServer**: nothing in code. Single mention at line 349 as a section header comment: `// HTTP FEATURES (from DeclarativeServer)`.
- **Why it depends on it**: it doesn't — this is a structural comment marking which builder methods were originally ported over from `DeclarativeServer`. Now that they live on `LithairServer`'s builder, the parenthetical is historical.
- **Migration approach**: **C / docs-only** — change the section header to `// HTTP FEATURES` (drop the parenthetical) or rephrase to `// HTTP FEATURES (formerly on DeclarativeServer)`.
- **Migration size estimate**: **S** (1-line change).
- **Blockers / dependencies on other modules**: none.
- **Recommended PR order**: 2 (right after the chosen phase-1 migration; trivial follow-up).

### 6. `examples/07-auth-rbac-mfa/test.sh`

- **What it imports/uses from DeclarativeServer**: nothing — it's a shell script. Single mention at line 3 as a comment: `# Tests DeclarativeServer with RBAC (password-based auth)`.
- **Why it depends on it**: it doesn't. The example's actual binary (`examples/07-auth-rbac-mfa/src/main.rs:13,240`) already uses `LithairServer::new()`. The shell comment is stale.
- **Migration approach**: **C / docs-only** — fix the comment to say `LithairServer`.
- **Migration size estimate**: **S** (1-line change).
- **Blockers / dependencies on other modules**: none.
- **Recommended PR order**: 6 (or merge with the rbac module change since both are RBAC-themed).

## What the issue body did not list (real code dependencies)

For completeness — these are the actual code-level dependencies that need real migration work later. They are out of scope for phase 1 but documented here so the next phases have a target:

### A. `lithair-macros/src/declarative_simple.rs:1380`

```rust
lithair_core::http::DeclarativeServer::<#name>::new(&event_store_path, args.port)?
```

This is generated code emitted by the `#[derive(DeclarativeModel)]` macro. Migrating this requires either:
- Generating `LithairServer::new().with_model::<#name>(...)` instead, OR
- Extracting a builder-agnostic trait that both servers implement

This is **M / L** sized work and is the actual blocker for retiring `DeclarativeServer`.

### B. `lithair-core/src/http/mod.rs:72` (re-export)

```rust
pub use declarative_server::{DeclarativeServe, DeclarativeServer, GzipConfig, ObserveConfig, PerfEndpointsConfig, ...};
```

Public re-export. Removing this is the final step (breaking change). **S** size mechanically, but coordinated with macro migration and a major release.

### C. `lithair-core/src/http/declarative_server.rs` (the file itself)

The 1700-LOC type definition. Internal `DeclarativeServer::<Self>::new(...)?.serve()` calls at lines 1643, 1653 are inside `impl` blocks for `DeclarativeModel` traits — these come along when the file is retired.

### D. `docs/` user-facing guides

User-facing docs still recommend `DeclarativeServer` in:
- `docs/guides/http_hardening_gzip_firewall.md`
- `docs/guides/http_performance_endpoints.md`
- `docs/guides/http_stateless_performance.md`
- `docs/modules/http-server/README.md`
- `docs/modules/firewall/{README.md,overview.md,attributes.md}`
- `docs/architecture/overview.md`
- `docs/internal/RBAC_IMPLEMENTATION_PLAN.md`

These should be updated as part of phase 2 once `LithairServer` parity is verified.

## Recommended PR sequence

1. **(this PR)** — Phase 1 inventory + smallest migration (`http/utils.rs:286` doc comment).
2. **PR #2** — Remaining 4 doc-only fixes batched together (`builder.rs:349`, `rbac/mod.rs:11`, `session/mod.rs:13` example block, `consensus/mod.rs:88`, `examples/07-auth-rbac-mfa/test.sh:3`). Single PR because they're all trivial doc edits with zero coupling. Total ~10 lines changed.
3. **PR #3** — User-facing docs sweep (`docs/guides/*`, `docs/modules/*`, `docs/architecture/*`). Pure docs PR, no code touched.
4. **PR #4** — `#[deprecated]` attribute on `DeclarativeServer` struct. Gated on parity verification (PR #41 ops endpoints + any other gaps). Will trigger compile warnings on internal callsites; fix or `#[allow(deprecated)]` per-site.
5. **PR #5 (M-L)** — Macro migration (`lithair-macros/src/declarative_simple.rs:1380`). The real work. May require a builder-agnostic trait abstraction, depending on what `LithairServer` exposes.
6. **PR #6** — Retire `lithair-core/src/http/declarative_server.rs` and remove the re-export. Breaking change, ships in v0.X.0 (probably v0.2.0 per the issue body).

## Source-citation log

All claims in this document are based on re-reading source on branch `feat/issue-42-decl-server-retirement-phase-1` (forked from `main` at HEAD, 2026-05-04). Specific line citations:

- `lithair-core/src/rbac/mod.rs:11` — single doc-comment mention.
- `lithair-core/src/session/mod.rs:13` — single doc-comment mention inside a `no_run` example block (lines 11-23).
- `lithair-core/src/consensus/mod.rs:88` — doc comment for `ConsensusConfig`, struct itself `#[deprecated]` at line 89.
- `lithair-core/src/http/utils.rs:286` — doc comment for `log_access` (defined at line 291).
- `lithair-core/src/app/builder.rs:349` — section-header comment.
- `examples/07-auth-rbac-mfa/test.sh:3` — shell script comment; the example binary at `examples/07-auth-rbac-mfa/src/main.rs:13,240` already uses `LithairServer::new()`.
- `lithair-macros/src/declarative_simple.rs:1380` — real code dependency (out of phase 1 scope).
- `lithair-core/src/http/mod.rs:72` — re-export (out of phase 1 scope).

`LithairServer` parity facts referenced above:
- `with_rbac_config` defined `lithair-core/src/app/builder.rs:433`.
- `with_sessions` defined `lithair-core/src/app/builder.rs:218`.
- RBAC handlers wired at `lithair-core/src/app/builder.rs:490, 517`.
