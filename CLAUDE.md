# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Lithair is a declarative memory-first web server framework in Rust. Core philosophy: "In Memory We Trust, In Data We Believe" - data models define infrastructure through declarative annotations.

**Key crates:**

- `lithair-core/` - Core framework with zero-dependency design
- `lithair-macros/` - Proc macros for `#[derive(DeclarativeModel)]`
- `examples/` - Production-ready examples (scc2_server_demo, raft_replication_demo, rbac_sso_demo, etc.)
- `cucumber-tests/` - BDD tests with Cucumber

## Common Commands

Uses [Taskfile](https://taskfile.dev). See all tasks: `task help`

### Development Cycle

```bash
task ci:full        # Fast CI (~2-3min): fmt + build + clippy + tests with -D warnings
task ci:github      # Complete validation (~10-15min): ci:full + smoke tests - run before push
```

### Build & Test

```bash
task build          # Debug build
task build:release  # Release build with LTO
task test           # Run all workspace tests
task lint           # Clippy with -D warnings
task fmt            # Format code
```

### Run Examples

```bash
task scc2:serve PORT=18321                    # Start SCC2 server
task scc2:demo                                # Full demo with benchmarks
task loadgen:json LEADER=http://127.0.0.1:18321 BYTES=65536 CONC=512  # Load test
task examples:rbac-session                    # RBAC session demo
```

### BDD Tests

```bash
task bdd:all         # All BDD suites
task bdd:performance # Performance tests
task bdd:security    # Security tests
```

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

```bash
# 1. Create feature branch from main
git checkout main && git pull origin main
git checkout -b feat/my-feature

# 2. Work, commit incrementally
#    Run CI before each push:
task ci:full
git add <files>
git commit -m "feat: description of change"

# 3. Push and create PR
git push -u origin feat/my-feature
gh pr create --title "feat: description" --body "## Summary\n- ...\n\n## Test plan\n- ..."

# 4. CI must pass, then merge via GitHub (squash merge recommended)
gh pr merge --squash --delete-branch
```

### Rules

- **Never push directly to `main`** -- always go through a PR
- **One concern per PR** -- keep PRs small and focused
- **CI must pass** before merge (`task ci:full` at minimum)
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

1. `task ci:full` passes (fmt + clippy -D warnings + tests)
2. Ensure new or modified behavior is covered by tests
3. `task ci:github` for final validation before requesting review

## Spec-Driven Development Workflow

The project uses slash commands for feature development:

1. `/specify <feature>` - Create specification and feature branch
2. `/plan <details>` - Generate implementation plan with artifacts
3. `/tasks <context>` - Break down plan into executable tasks

Templates are in `/templates/`, specs go in feature-specific directories.

## Key Documentation

- `docs/guides/getting-started.md` - Quick start guide
- `docs/guides/data-first-philosophy.md` - Core philosophy
- `docs/CI_WORKFLOW.md` - CI task breakdown
- `docs/development/ai-instructions.md` - Extended AI guidelines
- `docs/modules/` - Per-module documentation
