# ✅ Lithair Examples - Validation Notes

**Date:** 2026-03-07
**Status:** current validation model documented

---

## Purpose of This Page

This page is no longer a snapshot of the old demo-based task surface.

Its role is to document how example validation is now organized after the
cleanup of legacy internal demos and historical example aliases.

---

## Current Validation Model

### Public examples

Public examples are validated through the root catalog and its workspace
packages:

- `examples/01-*` to `examples/15-*`
- `examples/advanced/*`
- `examples/README.md` as the authoritative index

### Framework behavior

Framework behavior is validated separately:

- focused tests inside `lithair-core`
- crate tests under `lithair-core/tests/`
- BDD coverage under `cucumber-tests/`

This means internal behavior is no longer “tested” by keeping fake demos in
`lithair-core/examples`.

---

## Current Taskfile Entry Points

The current public shortcuts are the ones aligned with the real catalog:

```bash
task examples:list
task examples:test
task examples:hello-world
task examples:rbac-session
task examples:blog:serve
task examples:blog:test
task examples:replication:firewall
task examples:replication:hardening
```

These replace the older task naming model based on obsolete demo names.

---

## What Was Validated During Cleanup

The cleanup pass focused on three validation goals.

### 1. Keep public examples at the repository root

Confirmed and documented:

- the root `examples/` directory is the user-facing catalog
- advanced scenarios remain public under `examples/advanced/`
- docs and Taskfile now point to these real paths

### 2. Remove fake examples from `lithair-core`

Completed:

- `frontend_http_server.rs` removed
- `frontend_memory_demo.rs` removed
- `rbac_password_test.rs` removed

### 3. Preserve coverage where those files were previously compensating

Completed:

- minimal frontend server tests added in `lithair-core/src/frontend/server.rs`
- minimal admin asset tests added in `lithair-core/src/frontend/admin.rs`

---

## What This Page No Longer Assumes

This page should not be read as evidence that these historical names are still
valid:

- `scc2_server_demo`
- `raft_replication_demo`
- `simplified_consensus_demo`
- `examples:scc2`
- `examples:firewall`
- `examples:hardening`

They belong to the repository's historical documentation layer, not to the
current public examples contract.

---

## Practical Validation Guidance

If you want to validate the catalog today:

1. use `task examples:list` to inspect the current public index
2. use `task examples:test` to compile the current workspace examples
3. use `task examples:hello-world` or `task examples:rbac-session` for quick
   smoke runs
4. use `task examples:blog:test` for a fuller end-to-end example script
5. use `task examples:replication:firewall` and
   `task examples:replication:hardening` for advanced operational scenarios

---

## Conclusion

The repository now validates examples and framework behavior through clearer,
separate channels:

- public examples are cataloged and run from the root
- advanced validation scenarios stay under `examples/advanced/`
- framework regressions are covered by tests, not hidden demos

That split is the main result this page should preserve.
