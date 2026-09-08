# Lithair CI Workflow Guide

[`cidx.toml`](../../cidx.toml) is the source of truth for validation. Local cidx
runs and `.github/workflows/cidx.yml` use the same containerized phases. Use
cidx directly for checks and the PR lifecycle; Task is optional for project
helpers.

## Validation commands

| Command | Purpose |
|---------|---------|
| `cidx validate` | Validate configuration and workflow invocations |
| `cidx run code` | Rustfmt check and clippy; required before every commit |
| `cidx run test` | Workspace unit, core integration, macro and behavior BDD tests |
| `cidx run security` | cargo-audit, gitleaks and trivy |
| `cidx run build` | Workspace release build |
| `cidx run pr` | Code and test phases |
| `cidx run ci` | Full security, code, test and build pipeline before review |

A local success does not override a remote failure: differences in caches,
network availability and test isolation can still surface in GitHub Actions.
Verify the remote checks and read review comments before merging.

## PR lifecycle

Open the draft PR **before implementation**, from a clean working tree. cidx
creates the branch and initial commit and pushes the draft. Issue-linked work
uses an `issue-NUMBER` branch; omit `--issue` for work without an issue.

```bash
cidx repo pr create --issue NUMBER 'fix: describe the correction'
# Implement and validate on the created branch.
cidx repo cpw -m 'fix: describe the correction'
cidx repo pr edit --title 'fix: final title' --body 'Behavior and validation'
cidx repo pr status
cidx repo pr watch
# After successful validation and resolution of review findings:
cidx repo pr ready
cidx repo pr merge --method squash
```

`cidx repo cpw` runs the code gate, commits, pushes and tracks CI. Do not bypass
checks with `--no-verify` or `--skip-checks`. Read review text on GitHub when cidx
provides only a summary. A failed gate keeps the PR in draft until resolved.

## Migrating from Task

The generic Task validation and pipeline targets have been removed. They no
longer provide an alternative host-toolchain path or wrappers around cidx.

| Removed Task command | Use directly |
|----------------------|--------------|
| `task check` / `task lint` | `cidx run code` |
| `task fmt:check` | `cidx run rustfmt` (or the full `cidx run code` gate) |
| `task test` | `cidx run test` |
| `task pr` | `cidx run pr` |
| `task ci` | `cidx run ci` |
| `task bdd:ci` | `cidx run test` (behavior tier); dedicated `task bdd:*` helpers for long suites |

The test gate is defined by `cidx.toml`; it is not an alias for the former
host-native `cargo test --workspace --all-features` command. The CI phase and
coverage definitions are unchanged by this migration.

Task still runs examples, demos, load generation, benchmarks, documentation
commands and dedicated BDD suites. `task fmt` is an editing helper, not a
validation gate. `task build` and `task build:release` build hello-world and
loadgen for demos; `cidx run build` builds the workspace in release mode.

```bash
task examples:hello-world
task smoke
task bench:host-router
task bdd:distribution
task docs:lint
task help
```

## Environment setup and troubleshooting

Run `./scripts/setup.sh` to bootstrap Rust, cidx and probatum. Add `--with-task`
if you want the optional project helpers. The pinned Rust toolchain lives in
`rust-toolchain.toml`; the code/build/test presets select the CI container.

A running Docker daemon and socket access are required for the default cidx
backend. Use `cidx doctor` to diagnose the environment. If Docker is unavailable,
fix the environment and rerun cidx rather than treating a host-native Cargo
check as a passing CI gate. `task fmt` can still edit files without Docker.
