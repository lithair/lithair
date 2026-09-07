# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Lithair is a declarative memory-first web server framework in Rust. Core philosophy: "In Memory We Trust, In Data We Believe" - data models define infrastructure through declarative annotations.

**Published on crates.io** (`lithair-core`, `lithair-macros`, `lithair-cli`).

**Key crates:**

- `lithair-core/` - Core framework
- `lithair-macros/` - Proc macros for `#[derive(DeclarativeModel)]`
- `lithair-cli/` - CLI scaffolding tool (`lithair new`)
- `examples/` - 11 progressive examples (hello-world through distributed clusters)
- `cucumber-tests/` - BDD tests with Cucumber (performance, durability, clustering)

## Common Commands

Use **cidx for validation and the PR workflow**, as required by `AGENTS.md`.
`cidx.toml` defines the phases used locally and by `.github/workflows/cidx.yml`.

```bash
./scripts/setup.sh       # Bootstrap Rust, cidx and probatum
cidx run code            # CI rustfmt + clippy; mandatory before each commit
cidx run test            # Unit + integration + macro + behavior BDD gate
cidx run security        # cargo-audit + gitleaks + trivy
cidx run build           # Workspace release build
cidx run pr              # Code + test phases
cidx run ci              # Full pipeline before review
```

Task is optional for project helpers. Install it with
`./scripts/setup.sh --with-task`; see `task help` for the available commands.
Its generic CI/check/test wrappers have been removed.

```bash
task fmt                         # Edit Rust formatting; validate with cidx
task build                       # Build hello-world + loadgen (debug)
task build:release               # Build hello-world + loadgen (release)
task examples:hello-world        # Run the minimal server
task examples:rbac-session       # Run the session example
task examples:blog:serve PORT=3000
task bench:host-router           # Dedicated benchmark
task bdd:performance             # Long-running performance suite
task bdd:distribution            # Dedicated clustering suite
```

See `docs/internal/CI_WORKFLOW.md` for command migration and
`docs/TESTING.md` for the test pyramid and dedicated suites.

## Architecture

### Core Modules (`lithair-core/src/`)

| Module | Purpose |
|--------|---------|
| `engine/` | SCC2 lock-free concurrent engine, event sourcing |
| `http/` | Hyper-based HTTP server, router, firewall |
| `rbac/` | Role-based access control with field-level permissions |
| `session/` | Session management with state engine |
| `consensus/`, `raft/` | OpenRaft integration for distributed clustering |
| `frontend/` | Memory-first static file serving |
| `schema/` | Auto-generated database schema |
| `lifecycle/` | Audit trails, history tracking |
| `security/` | Authentication, validation, JWT support |

### Declarative Model Pattern

One struct generates complete backend infrastructure:

```rust
#[derive(DeclarativeModel)]
pub struct Product {
    #[db(primary_key, indexed)]           // Database constraints
    #[http(expose)]                       // REST API endpoint
    #[permission(read = "Public")]        // RBAC security
    #[persistence(replicate)]             // Distributed replication
    #[lifecycle(audited)]                 // Audit trail
    pub id: Uuid,
}
```

Annotations automatically generate: REST endpoints, database schema, validation, RBAC, event sourcing, and replication.

## Rust Coding Standards

### Patterns to Follow

- Use `if let`, `match`, or combinators instead of `unwrap()` after `is_some`/`is_ok`
- Derive `Default` when type has reasonable empty state
- Use `rsplit(delim).next()` instead of `split().last()`
- Use `or_default()` instead of `or_insert_with(HashMap::new)`
- Use `strip_prefix()` instead of manual `s[1..]` after `starts_with`
- Prefer clear `if/else` over `condition.then(..).unwrap_or(..)`
- Box large error variants in Results

### HTTP/Hyper Conventions

```rust
type RespBody = BoxBody<Bytes, Infallible>;
type Resp = Response<RespBody>;
type RespErr = Box<Response<BoxBody<Bytes, Infallible>>>;
```

## Git Workflow (Trunk-Based Development)

`main` is the protected trunk. All changes go through short-lived feature branches and Pull Requests.

### Branch Naming

```
feat/<short-description>    # New features
fix/<short-description>     # Bug fixes
chore/<short-description>   # Maintenance, deps, CI
docs/<short-description>    # Documentation only
refactor/<short-description> # Code restructuring
```

### Development Flow

Create the draft PR **before implementation**. cidx creates an `issue-NUMBER`
branch when linking an issue; omit `--issue` for work without one. Start with a
clean working tree and preserve any unrelated changes.

```bash
cidx repo pr create --issue NUMBER 'fix: describe the correction'
# Implement the change and its regression coverage on the created branch.
cidx run test
cidx repo cpw -m 'fix: describe the correction'  # code gate, commit, push, watch
cidx repo pr edit --title 'fix: final title' --body 'Behavior and validation'
cidx run ci
cidx repo pr status
cidx repo pr watch
# Only once checks pass and review findings are addressed:
cidx repo pr ready
cidx repo pr merge --method squash
```

Do not use `--no-verify` or `--skip-checks` to bypass the gates. Use cidx for PR
creation, updates, status, readiness and merge. Read review text on GitHub where
cidx only exposes summary status.

### Rules

- **Never push directly to `main`** -- always go through a PR
- **One concern per PR** -- keep PRs small and focused
- **CI must pass** before merge (the cidx code, test, security and build gates)
- **Short-lived branches** -- merge within hours/days, not weeks
- **Squash merge** -- keeps `main` history clean and linear
- **Delete branch after merge** -- no stale branches

### Commit Messages

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```
feat: add native TLS termination
fix: correct session expiry calculation
chore: bump tokio to 1.36
docs: document trunk-based workflow
refactor: extract PEM loading helpers
```

### Pre-Push Checklist

1. `cidx run code` and `cidx run test` pass (formatting, clippy and tests)
2. Ensure new or modified behavior is covered by tests
3. `cidx run ci` (full pipeline) for final validation before requesting review

### Pre-Merge Checklist

Before merging any PR, always:

1. **Read all review comments** — automated (CodeRabbit, Gemini) and human
2. **Check CI pipeline status** — all checks must pass
3. **Address critical/major findings** — don't merge with unresolved issues
4. **Fix nits in a follow-up or same PR** — don't ignore them

## Multi-Agent & Release Discipline

Rules encoded after the v1.6.0 release incident, where a resumed sub-agent
handed out instructions based on stale state (told the user to publish an
already-published release, then theorized a phantom publisher):

- **The main session owns terminal state operations** — `cargo publish`, tag
  pushes, GitHub secrets. Sub-agents prepare and report; they must never
  instruct the user to run state-changing commands themselves.
- **Stop agents once their mission is complete.** A resumable agent's
  knowledge freezes at its last transcript entry; every later resume replays
  that stale state as if it were current.
- **Resuming an agent? Open with a state snapshot** of everything that
  changed since its last report.
- **Verify before relaying**: any agent claim about external state (CI
  status, secrets, published versions) gets checked at the source first
  (`gh secret list`, the crates.io index, `gh pr checks`).
- **Surprising external state has a boring explanation first** — check the
  main session's own recent actions before theorizing about phantom
  automation or advising credential revocation.
- **Releases**: the tag-triggered `publish` job in `release.yml` is the only
  automated publish path; it is idempotent (already-published versions are
  skipped). Manual publishes, when needed, run from a clean checkout of the
  tag — never from a working tree.

## Spec-Driven Development Workflow

The project uses slash commands for feature development:

1. `/specify <feature>` - Create specification and feature branch
2. `/plan <details>` - Generate implementation plan with artifacts
3. `/tasks <context>` - Break down plan into executable tasks

Templates are in `/templates/`, specs go in feature-specific directories.

## Key Documentation

- `docs/guides/getting-started.md` - Quick start guide
- `docs/guides/data-first-philosophy.md` - Core philosophy
- `docs/internal/CI_WORKFLOW.md` - CI task breakdown
- `docs/TESTING.md` - Test pyramid: what goes where, the per-PR gate, BDD workflow
- `docs/internal/development/ai-instructions.md` - Extended AI guidelines
- `docs/modules/` - Per-module documentation
