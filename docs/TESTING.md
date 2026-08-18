# Testing — the pyramid, what goes where, and why

Lithair is also a demonstration of how far a test workflow can be pushed
while staying understandable. This page is the map: every tier, who runs it,
what belongs in it, and the hard-won rules — each one traces back to a real
bug this suite caught.

## The pyramid

| Tier | What it proves | Where it lives | Runs |
|---|---|---|---|
| **Unit** | internal invariants (~400 tests) | `#[cfg(test)]` modules in `lithair-core/src/` | every PR (CI) |
| **Integration** | the real API: server boot + HTTP + assertions (23 suites) | `lithair-core/tests/*.rs` | every PR (CI) |
| **Compile-fail** | the declarative macro surface: typos and wrong positions fail the build (gate G2) | `lithair-macros/tests/ui/*.rs` (trybuild) | every PR (CI) |
| **Behavior BDD** | user-visible promises as executable Gherkin specs | `cucumber-tests/features/{persistence,models,core/{scaffolding,sessions}}` | every PR (CI) |
| **Performance / durability BDD** | throughput, 1M-event stress, snapshot/durability drills | `cucumber-tests/features/performance/` | `task bdd:performance` (manual/nightly) |
| **Cluster** | real 3-node replication, leader election | `cucumber-tests/features/core/*cluster*` | `task bdd:distribution` (manual/nightly) |
| **Load / benchmarks** | published baselines vs Axum+SQLite | `benchmarks/run.sh`, `tools/loadgen` | manual ([baselines](performance/baselines.md)) |

The per-PR CI gate (the `test` phase in [`cidx.toml`](../cidx.toml)) chains
unit → integration → compile-fail → behavior BDD in one container — ~8 min
on GitHub runners. Long-running tiers stay out of the gate *by design*:
a slow gate stops being run.

## The workflow

**New feature** → write the Gherkin scenario first (red), implement until
green, open the PR: CI executes the whole behavior tier. The `.feature`
file is simultaneously the test and the living documentation.

**Bug** → write the reproduction first (an integration test, or a BDD
scenario if the bug breaks a user-visible promise), watch it fail, fix,
keep the repro as the regression test. The pre-merge checklist requires new
behavior to be covered — a fix without its repro is unfinished.

## Rules — each one is a scar

These are not style preferences; every rule below traces to a real bug
found in this repository the week the suites joined the CI gate (#177):

1. **Never assert wall-clock time.** Assert the *property*, not its shadow
   on a stopwatch. A "concurrent collection takes < 400 ms" test flaked on
   loaded runners for months; the fix asserts a peak-concurrency atomic —
   sequential = peak 1, concurrent = peak N, load-independent. Waiting for
   I/O? Poll with a deadline, never `sleep(50ms)`.
2. **Env vars are process-global — serialize suites that mutate them.**
   Cucumber runs scenarios concurrently by default. A retention scenario
   asserting limit 10 while a neighbour set 100 cost an afternoon (#175).
   Runners whose steps touch `std::env` use `max_concurrent_scenarios(1)`,
   and asserts print the env state on failure.
3. **A spec nobody runs is worse than no spec.** 10 orphan `.feature`
   files existed; investigating one surfaced a *data-loss bug* (#176 — the
   log "rotation" deleted event history). The `no_orphan_features`
   meta-test now fails CI when a feature file has no declared runner:
   adding a spec forces the "who runs it?" decision.
4. **Silently-ignored configuration is a bug, not a convenience.** Gate G2
   makes unknown attribute keys fail the build; the trybuild suite pins the
   diagnostics. The same pass found that a *known* attribute in the wrong
   position (`#[retention]` on a field) was silently ignored — a typo'd
   memory budget is an OOM in production, not a style issue.
5. **A test that only ever ran on the maintainer's machine is not a test.**
   The whole integration + BDD net was compiled — never executed — by CI
   before #177. If it's not in a gate or a named task, it doesn't exist.

## Commands

```bash
task check             # fmt + clippy -D warnings (fast local loop)
task test              # all workspace tests
cidx run test          # the exact per-PR CI gate, in the CI container
cargo test -p lithair-core --tests                 # integration tier only
cargo test -p lithair-macros --test compile_fail   # macro surface (trybuild)
cd cucumber-tests && cargo test --test cucumber_tests   # persistence BDD
cd cucumber-tests && cargo test --test sessions_test    # session cookie journey BDD
task bdd:performance   # long-running perf/durability suites
task bdd:distribution  # real-cluster suites
```

Refreshing trybuild snapshots after an intentional diagnostic change:
`TRYBUILD=overwrite cargo test -p lithair-macros --test compile_fail`.

## Debugging a red BDD scenario

1. Run it alone: `cd cucumber-tests && cargo test --test <runner> -- --name "<scenario name>"`.
2. Green alone, red in suite? Suspect shared state — env vars first
   (the failure message of retention asserts prints them; add the same to
   yours).
3. Need engine logs? `RUST_LOG=info cargo test --test <runner> -- --name …`
   (cucumber captures per-step output; it prints on failure).
4. `Step skipped` means *no step definition matched* — the scenario is a
   wishlist entry, not a passing test. Either implement the steps or ask
   whether the promise is already covered by an integration test.

## Adding a feature file

Create it under the directory that matches its tier, then run
`cargo test -p cucumber-tests --test no_orphan_features` — it fails until
you declare a runner for the new file in its `CLAIMED` map. That's the
point: every spec has an owner, per-PR or nightly, from day one.
