# Lithair CI Workflow Guide

The single source of truth for CI is [`cidx.toml`](../../cidx.toml). GitHub
Actions (`.github/workflows/cidx.yml`) and local validation run the exact same
containerized phases via [cidx](https://github.com/cidx-org/cidx). The
Taskfile only delegates — it no longer duplicates pipeline definitions.

## Quick Reference

**Inner loop (seconds, host-native):**

```bash
task check      # cargo fmt --check + clippy -D warnings
task test       # all workspace tests
```

**Before push (containerized, GitHub parity):**

```bash
task ci         # cidx run ci — security + code + test + build
task pr         # cidx run pr — code + test only (faster)
```

**Functional demos (not part of the CI gate):**

```bash
task smoke      # firewall + hardening demos, example test suites (~5-10min)
```

## Task Breakdown

| Task | Runs | Time | When |
|------|------|------|------|
| `task check` | fmt --check + clippy -D warnings on the host | seconds | every edit cycle |
| `task test` | `cargo test --workspace --all-features` | ~1-2min | before commit |
| `task pr` | `cidx run pr` (code + test phases) | minutes | fast pre-push gate |
| `task ci` | `cidx run ci` (security + code + test + build) | ~10min | before opening a PR |
| `task smoke` | demo scripts + example suites | ~5-10min | when touching demos |

## Why cidx and not plain cargo?

Local `cargo fmt` / `cargo clippy` run whatever toolchain is on your machine.
rustfmt and clippy gain new behaviors between releases; CI runs a pinned
container image. `task ci` runs that same image locally — if it passes, the
GitHub `CIDX CI` workflow passes.

`rust-toolchain.toml` pins the host toolchain to the same version as the CI
image, so `task check` normally agrees with CI — the container run is the
guarantee.

## Environment Setup

`task setup` (or `./scripts/setup.sh` directly if `task` itself is missing)
bootstraps everything: rustup + pinned toolchain, go-task, cidx, probatum.
Docker is required for `task ci` / `task pr`.

## Common Issues

**Problem:** `task check` passes but CI fails on formatting/clippy
**Solution:** toolchain drift — re-run `task setup` (installs the pinned
toolchain from `rust-toolchain.toml`), or validate with `task ci`.

**Problem:** `task ci` fails with a Docker error
**Solution:** cidx needs a running Docker daemon. `task check` + `task test`
still work without it.
