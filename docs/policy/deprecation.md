# Deprecation policy

How Lithair removes or changes public behavior. "Public" means the
stable surfaces defined in the [v1.0 roadmap](../roadmap/v1.0.md):
the declarative annotations, the builder API, generated endpoint
shapes, operational endpoints, `LT_*` environment variables, and —
with the strongest guarantee — the on-disk event-store format.

## The 0.x line (history)

Before 1.0, SemVer pre-1.0 rules applied: **a minor release could break**. The
mitigations the project practiced (and still does):

- Every break is listed in [CHANGELOG.md](../../CHANGELOG.md) under the
  version that ships it, with the migration in the entry itself (see
  the 0.12.0 `lithair-macros` bump entry for the expected level of
  detail).
- Additive paths are preferred where they exist — e.g. adding a field
  with `#[db(default = X)]` under `#[lithair_model]` keeps old event
  stores replayable (see the
  [upgrade playbook](../operations/upgrade.md)).
- Security fixes target the latest minor only
  ([SECURITY.md](../../SECURITY.md)).
- The on-disk format is treated as if 1.0 rules already applied: no
  0.x release so far has required an event-store migration, and any
  that did would ship a migration path in the same release.

There was no deprecation *window* pre-1.0 — the CHANGELOG entry was the
notice. Pinning a 0.x minor (`lithair-core = "0.13"`) and reading the CHANGELOG
before bumping was the mitigation; the
[upgrade playbook](../operations/upgrade.md) is the procedure.

## 1.x — the active contract (since 1.0.0)

Now that 1.0 has shipped this is in force: **1.x minors are additive — they do
not break** the stable surface. Removals follow a window: **deprecate in minor
N, remove no earlier than minor N+2.** (To stay on one minor regardless, pin
with a tilde, e.g. `lithair-core = "~1.2"`; a plain `"1"` accepts every 1.x.)

1. **Deprecate (1.N)** — the item gets `#[deprecated(note = "use X;
   removed in 1.N+2 or later")]` (or for non-Rust surfaces: a startup
   `warn!` for env vars, a `Deprecation` response header for endpoints,
   a macro warning for annotations). The CHANGELOG entry names the
   replacement and the earliest removal version.
2. **Grace (1.N+1)** — both old and new paths work. The old path keeps
   its tests until removal so the grace period is real, not nominal.
3. **Remove (1.N+2 or later)** — the removal is its own CHANGELOG
   entry referencing the deprecation entry.

Accelerated removal is allowed only for security (per
[SECURITY.md](../../SECURITY.md))
or for behavior that is already broken in a way users cannot rely on —
both cases documented as such.

**The on-disk event-store format is excluded from this window**: it
does not get deprecated within 1.x at all. A format change means a
major version plus a shipped migration tool, full stop.

## Practical notes

- Deprecations are tracked with a `deprecation` label on the issue
  that introduces them, so the open set is one search away.
- `cargo build` surfacing `#[deprecated]` warnings is the intended
  upgrade experience: a clean build on 1.N+1 means 1.N+2 will not
  surprise you.
