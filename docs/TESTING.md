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

The private OpenRaft log foundation is also in this gate. The dedicated
`openraft_storage_test` target explicitly enables `cluster,tls` and runs the upstream storage
suite plus crash/reopen and error-injection regressions. `openraft_storage_bdd`
owns `features/persistence/openraft_storage.feature`. These storage tests do not
qualify the current HTTP cluster; see the
[storage contract](internal/specs/OPENRAFT_STORAGE.md).

A bounded three-process OpenRaft subset also runs on every PR:
`openraft_consensus_test` exercises authenticated RPCs, repeated leader death,
restart, real TCP partitions and quorum rejection. `openraft_consensus_bdd` owns
`features/core/openraft_consensus.feature` and runs the crash/partition scenarios
with the same process fixture. These use a test-only state machine; they do not
qualify native/Turso/session replication. See the
[transport contract](internal/specs/OPENRAFT_TRANSPORT.md).

`openraft_checkpoint_test` and `openraft_checkpoint_bdd` cover snapshot publication,
physical journal compaction and process interruption. The consensus fixture also
checks snapshot transfer to a lagging follower and cold recovery after purge.
See the [checkpoint contract](internal/specs/OPENRAFT_CHECKPOINTS.md).


`openraft_identity_test` and `openraft_identity_bdd` cover durable node/cluster
binding and explicit single-use bootstrap, including interrupted claims and the
three-process cold restart before and after initialization. See the
[identity contract](internal/specs/OPENRAFT_IDENTITY.md).

`cluster_operator_test`, the opt-in CLI `cluster_ops` subprocess tests and
`cluster_operator_bdd` cover strict configuration/TLS validation, explicit
provisioning, nonmutating offline inspection and bootstrap retry after peer
unavailability. The cidx gates explicitly enable `cluster-ops` for the CLI and
BDD targets; the release build also compiles the optional CLI. See the
[operator contract](internal/specs/OPENRAFT_OPERATOR.md).

## Probatum and Docker Compose cluster gate

`cidx run cluster` compiles the private test node, then runs Probatum 0.10.0
against three separate Compose containers. Each node owns its data and credential
volumes; replication and test control use different private networks. The gate
kills the leader, disconnects a node from replication, checks majority/minority
behavior, requires actual snapshot transfer and cold-restarts all three nodes.
It also proves that Probatum rejects an intentionally incorrect expected value.

This phase runs in `cidx run ci` and the GitHub Cluster job. Evidence is saved
under `.probatum/runs/lithair-cluster-*/` and uploaded even after failure. See
[the fixture runbook](../tests/cluster/README.md) for prerequisites, cleanup and
failure-injection commands. These containers share one host and use a test state
machine; this is not three-VM or native/Turso/session qualification. The existing
Rust/Gherkin suites remain the lower-level regression gate.

## Native concurrent engine

`native_scc2_test` covers concurrent reads/mutations, uniqueness and secondary
indexes, event ordering across reopen, legacy events, retention and failed
persistence acknowledgements. `native_scc2_bdd` owns
`features/core/native_scc2.feature` and exercises the same public runtime
promises with declaratively defined models in the test gate. These are
single-process tests, not native model cluster qualification. See the
[engine contract](features/state-engine/scc2.md).

## The workflow

**New feature** → open a draft PR with `cidx repo pr create`, then write the
Gherkin scenario first (red), implement until
green, update the draft PR: CI executes the whole behavior tier. The `.feature`
file is simultaneously the test and the living documentation.

**Bug** → open the draft PR before coding, then write the reproduction first (an integration test, or a BDD
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
cidx run code          # formatting check + clippy in the CI container
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

## HTTP tests with ephemeral ports

Reserve the socket with `tokio::net::TcpListener::bind("127.0.0.1:0")`, read
`listener.local_addr()` for the client URL, and pass the listener to
`builder.build()?.serve_with_listener(listener, shutdown)`. Keep the socket
open throughout startup: probing a free port and binding it again later leaves
a race with other tests. The listener address overrides the server host/port.
The shutdown future has the same semantics as `serve_with_graceful_shutdown`;
signal it and await the server task before removing the test data directory.

## Experimental hybrid storage

The optional `lithair-turso` crate's unit tests and `tests/declarative.rs` HTTP
regressions run in the standard test gate. Macro compile-fail cases pin unsupported
storage declarations and malformed options.
`cucumber-tests/tests/turso_test.rs` owns `features/persistence/turso.feature`
and is included in `cidx run test`. It exercises SQL rollback/reopen and the
mixed HTTP example across graceful server restart, plus transactional schema
upgrade/rollback. `lithair-turso/tests/migrations.rs` covers legacy files, paged
upgrades, declaration drift, cancellation/panic recovery and HTTP startup. The native model path does
not acquire a Turso dependency. See [RFC 235](rfcs/235-hybrid-storage.md).
