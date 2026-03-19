# 🔍 Lithair Examples - Status Audit

**Date:** 2026-03-07
**Scope:** public examples at repo root, advanced tools, and test/example split

## 📊 Summary

- Root learning examples in `examples/01-*` to `examples/15-*` are canonical.
- Advanced tools under `examples/advanced/*` are canonical.
- Core crate validation stays in `lithair-core` tests and modules.
- Internal fake examples in `lithair-core/examples` were removed.

---

## Canonical Catalog

### Root examples

The authoritative public catalog now lives in `examples/README.md` and includes:

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

### Advanced examples and validation tools

Advanced scenarios are kept in `examples/advanced/`:

- `datatable`
- `stress-test`
- `consistency-test`
- `playground`
- `http-firewall`
- `http-hardening`

These remain public examples, but they are intentionally more operational and
validation-oriented than the progressive learning path.

---

## Test and Example Separation

### What changed

The repository now follows a clearer split:

- `examples/` contains public, documented examples
- `examples/advanced/` contains advanced demos and validation tools
- `lithair-core` contains framework code and tests
- `cucumber-tests/` contains BDD coverage

### Removed internal artifacts

The following files were removed from `lithair-core/examples` because they were
internal demos or test-like artifacts, not public examples:

- `frontend_http_server.rs`
- `frontend_memory_demo.rs`
- `rbac_password_test.rs`

Coverage that was previously implicit in those files is now preserved by tests
added in frontend modules inside `lithair-core`.

---

## Taskfile Alignment

The Taskfile now points to the real root catalog instead of obsolete demo
names.

### Current entry points

```bash
task examples:list
task examples:test
task examples:hello-world
task examples:rbac-session
task examples:blog:serve
task examples:replication:firewall
task examples:replication:hardening
```

### Important consequence

Legacy names such as `scc2_server_demo`, `raft_replication_demo`,
`rbac_session_demo`, and `simplified_consensus_demo` are no longer considered
the public reference model for this repository.

---

## Recommended Reading Order

For new users:

1. `examples/01-hello-world`
2. `examples/03-rest-api`
3. `examples/04-blog`
4. `examples/06-auth-sessions`
5. `examples/09-replication`

For advanced validation:

1. `examples/advanced/http-firewall`
2. `examples/advanced/http-hardening`
3. `examples/advanced/stress-test`
4. `examples/advanced/consistency-test`
5. `examples/advanced/playground`

---

## Remaining Debt Outside This Audit

This cleanup pass focused on the examples inventory and legacy references.
Other historical warnings may still exist elsewhere in the repository, notably:

- older Markdown lint debt in unrelated docs
- legacy lint warnings in framework code unrelated to examples organization

Those issues are separate from the example/test boundary cleanup.

---

## Conclusion

The examples inventory is now coherent:

- root examples are the public source of truth
- advanced tools remain under `examples/advanced/`
- framework coverage stays in tests, not hidden demos
- internal fake examples have been removed from `lithair-core`

This document should be read as the post-cleanup status, not as a snapshot of
the old demo-based organization.
