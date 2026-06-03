# Contributing to Lithair

Thanks for considering a contribution. Lithair is a small project with a solo
maintainer; the conventions below exist so patches land quickly and CI stays
green.

## Development environment

- **Rust toolchain**: pinned to `1.95.0` via [`rust-toolchain.toml`](./rust-toolchain.toml).
  `rustup` will pick this up automatically the first time you run `cargo` in
  the repo. Don't override the channel — CI uses the same version.
- **Components**: `rustfmt`, `clippy`, `rust-analyzer` (declared in
  `rust-toolchain.toml`).
- **Task runner**: [Taskfile](https://taskfile.dev). Install `task` and run
  `task help` to see the full menu.
- **CI parity tool**: [cidx](https://github.com/cidx-org/cidx) runs the same
  containerized phases (rustfmt, clippy, cargo-audit, gitleaks, trivy,
  workspace test/build) that GitHub Actions runs. See `.github/workflows/cidx.yml`
  for the pinned version.

## Pre-commit validation (mandatory)

**Run `cidx run code` before every commit.** This is the authoritative gate.

```bash
cidx run code       # rustfmt + clippy in the Rust 1.95 image used by CI
cidx run security   # cargo-audit + gitleaks + trivy
cidx run test       # cargo test --workspace --lib --bins --release
cidx run ci         # full pipeline
```

Local `cargo fmt` / `cargo clippy` is not sufficient. rustfmt and clippy gain
new behaviors between releases, and CI runs the pinned 1.95 image — running
cidx locally is what guarantees your push won't bounce on formatting drift.

For tighter dev loops without a container, use the Taskfile entry points:

```bash
task ci:full        # ~2-3min: fmt + build + clippy + tests with -D warnings
task ci:github      # ~10-15min: ci:full + smoke tests — run before pushing
```

`task ci:full` is the minimum bar for a commit. `task ci:github` is the
recommended bar before opening a PR. See [`CLAUDE.md`](./CLAUDE.md) and
[`Taskfile.yml`](./Taskfile.yml) for the full command reference.

## Pull request workflow

Lithair uses [trunk-based development](https://trunkbaseddevelopment.com/).
`main` is the protected trunk; all changes land via short-lived feature
branches and squash-merged PRs.

### Branch naming

```
feat/<short-description>      # new features
fix/<short-description>       # bug fixes
chore/<short-description>     # maintenance, deps, CI
docs/<short-description>      # documentation only
refactor/<short-description>  # code restructuring with no behavior change
```

### Flow

```bash
# 1. Branch from a fresh main
git checkout main && git pull origin main
git checkout -b feat/my-change

# 2. Commit incrementally; run the gate before each push
cidx run code
git commit -m "feat: describe the change"

# 3. Push and open a PR
git push -u origin feat/my-change
gh pr create --title "feat: describe the change" \
             --body "## Summary\n- ...\n\n## Test plan\n- ..."

# 4. Wait for CI, address review comments, then squash-merge
gh pr merge --squash --delete-branch
```

### Rules

- **Never push directly to `main`** — always through a PR.
- **One concern per PR** — keep diffs small and focused.
- **CI must pass** before merge.
- **Squash merge** — keeps `main` linear.
- **Delete the branch** after merge.

### Commit messages

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```
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
`task test` (or `cargo test --workspace`).

### BDD suite

End-to-end behavioral tests live in `cucumber-tests/features/`. Each
sub-directory is a suite that maps to a `task bdd:*` target:

```bash
task bdd:setup        # install Cucumber dependencies (first run)
task bdd:all          # run every suite
task bdd:engine       # engine + basic server scenarios
task bdd:persistence  # event sourcing + hash chain
task bdd:performance  # performance + durability benchmarks
task bdd:scaffolding  # CLI scaffolding
task bdd:distribution # cluster replication
```

New runtime behavior should come with a `.feature` scenario in the relevant
suite. See `cucumber-tests/features/persistence/retention.feature` for the
current style.

## Questions and bug reports

- **Bugs, feature requests, design discussions**: open a GitHub issue at
  <https://github.com/lithair/lithair/issues>.
- **Security disclosures**: do not use public issues. See
  [`SECURITY.md`](./SECURITY.md).

## Review SLA

Honest version: the maintainer is solo and reviews are best-effort. Expect a
first response within **1–7 days**. Smaller, well-scoped PRs that pass
`task ci:github` locally land faster. If a PR sits for more than a week
without a response, a polite ping on the PR is welcome.

## Code style and conventions

`CLAUDE.md` at the repo root documents the project's Rust conventions
(`if let` over `unwrap`, `Default` where reasonable, HTTP type aliases,
logging via the `log` crate, `LT_` env-var prefix, etc.). The same
conventions apply to human contributors — clippy with `-D warnings` enforces
most of them automatically.

## Code of conduct

This project follows community standards for respectful collaboration. See
[`CODE_OF_CONDUCT.md`](./CODE_OF_CONDUCT.md) for the full text.
