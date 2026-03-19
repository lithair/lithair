# 📊 Lithair Examples - Executive Summary

**Date:** 2026-03-07
**Status:** ✅ Catalog clarified and public references consolidated

---

## Executive Summary

Lithair now uses a simpler public model for examples:

- root examples in `examples/` are the main user-facing catalog
- advanced demos and validation tools live in `examples/advanced/`
- framework behavior is validated by tests, not by hidden demos inside
  `lithair-core`

The older demo-centric naming scheme is no longer the source of truth.

---

## Current Public Catalog

### Progressive examples

The main learning path lives at the repository root:

- `01-hello-world`
- `02-static-site`
- `03-rest-api`
- `04-blog`
- `05-ecommerce`
- `06-auth-sessions`
- `07-auth-rbac-mfa`
- `08-schema-migration`
- `09-replication`
- `10-blog-distributed`
- `11-react` to `15-astro`

### Advanced examples

Operational and validation-oriented scenarios live in `examples/advanced/`:

- `datatable`
- `stress-test`
- `consistency-test`
- `playground`
- `http-firewall`
- `http-hardening`

---

## Recommended Entry Points

There is no longer a single “reference demo” that should dominate all other
examples. Instead, the best entry point depends on the goal:

- `01-hello-world` for the smallest server setup
- `03-rest-api` for declarative CRUD
- `04-blog` for a fuller application with sessions and frontend assets
- `06-auth-sessions` for auth and RBAC flows
- `09-replication` for clustering and replication

---

## Taskfile Shortcuts

The Taskfile is aligned with the current catalog:

```bash
task examples:list
task examples:test
task examples:hello-world
task examples:rbac-session
task examples:blog:serve
task examples:replication:firewall
task examples:replication:hardening
```

For the full list, see `Taskfile.yml` and `examples/README.md`.

---

## What Was Cleaned Up

This documentation pass completed the example/test boundary cleanup:

- legacy internal demos under `lithair-core/examples` were removed
- frontend coverage was preserved by adding focused tests in `lithair-core`
- docs and Taskfile entries were realigned to the root examples catalog
- obsolete names such as `scc2_server_demo`, `raft_replication_demo`,
  `rbac_session_demo`, and `simplified_consensus_demo` were removed from the
  public narrative

---

## Practical Guidance

### If you want to learn Lithair progressively

Start here:

1. `01-hello-world`
2. `03-rest-api`
3. `04-blog`
4. `06-auth-sessions`
5. `09-replication`

### If you want validation or operational scenarios

Start here:

1. `advanced/http-firewall`
2. `advanced/http-hardening`
3. `advanced/stress-test`
4. `advanced/consistency-test`
5. `advanced/playground`

---

## Conclusion

The examples story is now simpler and more maintainable:

- public examples live at the root
- advanced tools remain discoverable under `examples/advanced/`
- test coverage stays in test code
- documentation now points to the actual catalog users can run today
