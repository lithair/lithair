# Contributing to Lithair

Thanks for considering a contribution. Lithair is a small project with a solo
maintainer; the conventions below exist so patches land quickly and CI stays
green.

## Development environment

- **Rust toolchain**: pinned to `1.97.1` via [`rust-toolchain.toml`](./rust-toolchain.toml).
  `rustup` will pick this up automatically the first time you run `cargo` in
  the repo. Don't override the channel — CI uses the same version.
- **Components**: `rustfmt`, `clippy`, `rust-analyzer` (declared in
  `rust-toolchain.toml`).
- **Optional project helpers**: [Task](https://taskfile.dev) runs examples, demos,
  benchmarks and documentation tools. Install with `./scripts/setup.sh --with-task`
  and run `task help`; the CI/PR workflow does not require Task.
- **CI parity tool**: [cidx](https://github.com/cidx-org/cidx) runs the same
  containerized phases (rustfmt, clippy, cargo-audit, gitleaks, trivy,
  workspace test/build) that GitHub Actions runs. See
  [`.github/workflows/cidx.yml`](./.github/workflows/cidx.yml) for the CI
  configuration.

## Pre-commit validation (mandatory)

**Run `cidx run code` before every commit.** This is the authoritative gate.

```bash
cidx run code       # rustfmt + clippy in the Rust 1.97.1 image used by CI
cidx run security   # cargo-audit + gitleaks + trivy
cidx run test       # unit, integration, macro and behavior BDD gate
cidx run ci         # full pipeline
```

Local `cargo fmt` / `cargo clippy` is not sufficient. rustfmt and clippy gain
new behaviors between releases, and CI runs the pinned 1.97.1 image — running
cidx locally is what guarantees your push won't bounce on formatting drift.

Use `cidx run code` during development and `cidx run test` for the test gate.
Run `cidx run ci` for full validation before requesting review. Formatting edits
can use `task fmt` (or `cargo fmt`), but they do not replace the cidx code gate.
Bootstrap without Task using `./scripts/setup.sh`.

See [`cidx.toml`](./cidx.toml) for the phase definitions and the
[CI workflow guide](docs/internal/CI_WORKFLOW.md) for the Task command migration.

## Pull request workflow

Lithair uses [trunk-based development](https://trunkbaseddevelopment.com/).
`main` is the protected trunk; all changes land via short-lived feature
branches and squash-merged PRs.

### Branch naming

```text
feat/<short-description>      # new features
fix/<short-description>       # bug fixes
chore/<short-description>     # maintenance, deps, CI
docs/<short-description>      # documentation only
refactor/<short-description>  # code restructuring with no behavior change
```

### Flow

Open a **draft PR before implementation**, using cidx from a clean working tree.
It creates and pushes the branch and initial commit. Omit `--issue` for work
without an issue; issue-linked branches use cidx's `issue-NUMBER` naming.

```bash
# 1. Create the branch and draft PR before writing the fix
cidx repo pr create --issue NUMBER 'fix: describe the change'

# 2. Implement, validate and publish incremental commits
cidx run test
cidx repo cpw -m 'fix: describe the change'  # code gate + commit + push + CI watch

# 3. Keep the description current and verify checks/reviews
cidx repo pr edit --title 'fix: final description' --body 'Behavior and validation'
cidx repo pr status
cidx repo pr watch
cidx run ci

# 4. Once checks pass and review findings are addressed
cidx repo pr ready
cidx repo pr merge --method squash
```

Never bypass validation with `--no-verify` or `--skip-checks`. Read all review
comments before merging; cidx status summarizes reviews but does not show their
full text, which is available on GitHub.

### Rules

- **Never push directly to `main`** — always through a PR.
- **One concern per PR** — keep diffs small and focused.
- **CI must pass** before merge.
- **Squash merge** — keeps `main` linear.
- **Delete the branch** after merge.

### Commit messages

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```text
feat: add native TLS termination
fix: correct session expiry calculation
chore: bump tokio to 1.36
docs: document trunk-based workflow
refactor: extract PEM loading helpers
```

The commit subject becomes the squash-merge title, so write it for the
changelog.

## Tests

Unit and integration tests live next to the code they cover and run under
`cidx run test` (the complete per-PR test gate).

### BDD suite

End-to-end behavioral tests live in [`cucumber-tests/features/`](./cucumber-tests/features/).
Each sub-directory is a suite that maps to a `task bdd:*` target:

```bash
task bdd:setup        # install Cucumber dependencies (first run)
task bdd:all          # run every suite
task bdd:sessions     # session cookie journey
task bdd:persistence  # event sourcing + hash chain
task bdd:performance  # performance + durability benchmarks
task bdd:scaffolding  # CLI scaffolding
task bdd:distribution # cluster replication
```

New runtime behavior should come with a `.feature` scenario in the relevant
suite. See [`cucumber-tests/features/persistence/retention.feature`](./cucumber-tests/features/persistence/retention.feature)
for the current style.

## Questions and bug reports

- **Bugs, feature requests, design discussions**: open a GitHub issue at
  <https://github.com/lithair/lithair/issues>.
- **Security disclosures**: do not use public issues. See
  [`SECURITY.md`](./SECURITY.md).

## Review SLA

Honest version: the maintainer is solo and reviews are best-effort. Expect a
first response within **1–7 days**. Smaller, well-scoped PRs that pass
`cidx run ci` locally land faster. If a PR sits for more than a week
without a response, a polite ping on the PR is welcome.

## Code style and conventions

[`CLAUDE.md`](./CLAUDE.md) at the repo root documents the project's Rust
conventions (`if let` over `unwrap`, `Default` where reasonable, HTTP type
aliases, etc.). The same conventions apply to human contributors — clippy
with `-D warnings` enforces most of them automatically.

## Code of conduct

This project follows community standards for respectful collaboration. See
[`CODE_OF_CONDUCT.md`](./CODE_OF_CONDUCT.md) for the full text.
