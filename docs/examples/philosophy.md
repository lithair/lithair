# 🎯 Lithair Examples Philosophy

**Date:** 2026-03-07
**Vision:** clear public examples at the repo root, framework validation in tests,
and advanced operational scenarios kept discoverable without mixing roles

---

## Core Principle

> **Examples should teach runnable patterns. Tests should validate framework
> behavior.**

Lithair now separates three concerns more explicitly:

1. **Root examples** teach the framework progressively
2. **Advanced examples** exercise operational and validation scenarios
3. **Framework tests** verify internal behavior inside `lithair-core`

This avoids mixing product-facing examples with internal experiments or
test-like artifacts.

---

## What Counts as a Public Example

Public examples are runnable, documented, and part of the catalog users should
discover first.

### Progressive learning path

The root `examples/` directory is the main learning surface:

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

These examples are intentionally diverse. Some are minimal, some are fuller
applications, but they all belong to the public catalog because they are useful
entry points for users.

### Advanced operational scenarios

`examples/advanced/` contains scenarios that are still public, but more focused
on validation, experimentation, or operational behavior:

- `datatable`
- `stress-test`
- `consistency-test`
- `playground`
- `http-firewall`
- `http-hardening`

They are examples too, but they are not the default learning path.

---

## What Should Not Live as an Example

The repository should avoid treating these as public examples:

- internal debugging artifacts
- coverage stand-ins for missing tests
- fake demos that only exist to exercise framework internals
- obsolete demo names kept only for historical reasons

That is why `lithair-core/examples` was removed during the cleanup. The files in
that folder were closer to internal validation than to user-facing examples.

---

## Role of Tests

Tests are now the right place for framework validation.

### In practice

- frontend asset serving is covered by tests in `lithair-core`
- admin asset handling is covered by tests in `lithair-core`
- BDD coverage stays in `cucumber-tests/`

This keeps examples focused on showing how to use Lithair, instead of silently
carrying framework regression coverage.

---

## How to Choose the Right Example

There is no single “reference demo” anymore.

Choose the example that matches your goal:

- **First server setup** → `01-hello-world`
- **Declarative CRUD** → `03-rest-api`
- **Sessions, RBAC, frontend assets** → `04-blog`
- **Auth flows** → `06-auth-sessions`
- **Replication and clustering** → `09-replication`
- **Operational hardening** → `advanced/http-hardening`
- **Firewall behavior** → `advanced/http-firewall`
- **Stress and consistency work** → `advanced/stress-test` and
  `advanced/consistency-test`

---

## Guidance for Contributors

### Add something to `examples/` when

- it is useful to users directly
- it has a clear README or obvious entry point
- it demonstrates a pattern worth reusing
- it can be part of the public catalog without extra explanation

### Add something to `examples/advanced/` when

- it is still public and runnable
- it targets operational behavior, validation, or experimentation
- it is valuable, but not ideal as a first-step learning example

### Add something to tests instead when

- the goal is framework regression coverage
- the artifact only validates internal behavior
- the scenario would confuse users if presented as a public example

---

## Why This Matters

This organization improves the repository in several ways.

### For users

- the catalog is easier to navigate
- example names map to real, runnable directories
- learning paths are clearer

### For maintainers

- CI can target the actual public catalog
- framework coverage lives where it belongs
- docs do not need to preserve dead historical aliases

### For documentation

- `examples/README.md` is the authoritative index
- docs can link to stable, real paths
- advanced scenarios remain visible without becoming the default narrative

---

## Current Working Mental Model

Think of the repository like this:

- `examples/` = public catalog and learning path
- `examples/advanced/` = advanced demos and validation tools
- `lithair-core` = framework implementation and tests
- `cucumber-tests/` = behavior-level integration coverage

That model is simpler than the older demo-centric structure and matches how the
project is now organized in practice.

---

## Conclusion

The examples philosophy is now straightforward:

- examples teach usage
- advanced examples validate richer scenarios
- tests protect framework behavior

If a file helps users discover Lithair, it belongs in the public examples
catalog. If it mainly protects the framework from regressions, it belongs in
tests.
