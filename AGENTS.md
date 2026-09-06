# Working on Lithair

Lithair is a declarative, memory-first Rust web framework. Models generate REST
APIs, validation, permissions and event-sourced persistence. Keep single-node
use straightforward; clustering and other optional capabilities remain opt-in.

## Repository map

- `lithair-core/`: framework runtime; `app/` composes the server, `engine/`
  provides SCC2 storage and event sourcing, `http/` handles requests, and
  `frontend/` serves in-memory assets. Authentication, sessions, RBAC, schema,
  lifecycle and clustering have their own modules.
- `lithair-macros/`: derive and attribute macros, with trybuild diagnostics tests.
- `lithair-cli/`: application scaffolding and CLI commands.
- `examples/`: progressive examples and advanced demonstrations. Consult the
  root `Cargo.toml` for current workspace members.
- `cucumber-tests/`: executable behavior, persistence, performance and cluster specs.
- `docs/`: guides, architecture, module references and operations runbooks.

## Before editing

Read the relevant implementation and tests, plus `CLAUDE.md`, `CONTRIBUTING.md`
and `docs/TESTING.md`. Verify commands against `Taskfile.yml` and `cidx.toml`;
older documentation can contain stale paths or commands. Inspect the working
tree and preserve unrelated changes. For an issue, read its acceptance criteria
and comments, then create the draft PR through cidx **before writing the
reproduction or changing the implementation** (see Git and review below).
Keep the reproduction as regression coverage.

## Implementation conventions

- Follow the existing Rust API and module conventions. Use the toolchain pinned
  in `rust-toolchain.toml`; do not override it to work around a failure.
- Prefer `match`, `if let` and error propagation over unchecked `unwrap()` in
  runtime code. Use `or_default()`, `strip_prefix()` and derived `Default` where
  appropriate. Box large error variants and follow existing Hyper body aliases.
- Persistence changes must preserve event compatibility and restart/replay
  behavior. A successful mutation must perform the promised operation; propagate
  storage errors. Check concurrency and host/model isolation where relevant.
- Keep request paths memory-first. Avoid introducing disk I/O or repeated full
  collection scans on reads.
- Preserve panic isolation: the release profile must not use `panic = "abort"`.
- Update public documentation/examples when their documented behavior changes.

## Validation

Use **cidx as the authoritative entry point for validation and the PR
workflow**, including local checks. Do not substitute Task or host-native Cargo
commands for the cidx gates:

```bash
cidx run code       # mandatory before every commit: CI rustfmt and clippy
cidx run test       # CI unit, integration, compile-fail and behavior gate
cidx run ci         # full containerized pipeline before review
```

Task remains useful for project-specific commands such as setup, examples,
demos, benchmarks and dedicated BDD suites; consult `task help`. Prefer cidx
over the redundant Task wrappers for CI, linting and tests. Removing Task or
its duplicate targets is a separate maintenance change, not part of a bug fix.
Report checks actually run and any blockers; do not equate compilation with
test execution or mark a PR ready while required gates are failing.

Place internal invariants in unit tests and API/restart regressions in
`lithair-core/tests/`. New runtime promises should have executable Gherkin
coverage in the relevant BDD suite. Register new feature files with the
`no_orphan_features` test. Use temporary data directories and isolated ports.
Assert properties rather than elapsed time; poll with a deadline instead of
fixed sleeps. Serialize tests that mutate process-wide environment variables.

## Git and review

**Open a draft PR before implementing a correction or feature.** Do not wait
until the work is finished, and do not leave an assigned issue as an uncommitted
local patch. Use cidx for branch/PR creation, commit/push and CI tracking:

```bash
cidx repo pr create --issue 227 'fix: describe the correction'
# Work on the branch created by cidx, then publish the changes:
cidx repo cpw -m 'fix: describe the correction'
cidx repo pr status
cidx repo pr watch
# Once validation passes and the description is current:
cidx repo pr ready
```

Replace the example issue number and title with the actual task. For work
without an issue, omit `--issue`. `cidx repo pr create` requires a clean working
tree and creates the initial commit, branch and draft PR; preserve existing
changes before invoking it and restore them onto the appropriate branch.
Read-only investigation may precede PR creation. `cidx repo cpw` runs the code
gate before committing; do not bypass it with `--no-verify`. Use
`cidx repo pr edit` to keep the PR title and description current.

Use cidx's generated issue branches or short-lived `feat/`, `fix/`, `chore/`,
`docs/` or `refactor/` branches, with Conventional Commits. Never push directly
to protected `main`. Keep each PR focused, describe the resulting behavior,
link its issue and report validation.
Before merging, read human and automated review comments, address findings and
verify CI passes. Squash merges keep trunk history linear.

The main session owns release operations, tag pushes and secrets. If agents are
used, give them bounded tasks and refresh their state before resuming; verify
external-state claims at the source. The tag-triggered `release.yml` publish job
is the automated release path; manual publishing uses a clean checkout of the tag.
