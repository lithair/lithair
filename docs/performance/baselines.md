# Performance baselines (v1.0 gate G4)

Honest positioning, not winning charts. Lithair is memory-first and
event-sourced: **reads are cheap, durable writes are the cost.** This page
measures that against a conventional baseline so the trade-off is a number, not
a claim.

Reproduce everything with one command:

```bash
task bench:baselines          # or: ./benchmarks/run.sh
BENCH_TOTAL=50000 TIERS="64 256 1024" ./benchmarks/run.sh
```

## Methodology

- **Workload**: write-heavy CRUD — 85% `POST /api/products`, 15% `GET
  /api/products` — driven by `tools/loadgen` (`--mode random`), which reports
  throughput and p50/p95/p99 from per-request timings. Write-heavy because
  that's where event-sourcing vs SQL actually diverges.
- **Same harness, both servers.** loadgen drives identical requests against
  Lithair and the baseline; only the server differs.
- **Subjects**:
  - **Lithair** — single node, event-sourced, disk-durable (the
    `lithair-cluster-node` example, `--node-id 0`, no peers).
  - **Baseline** — Axum + SQLite, **file-backed**, `WAL` + `synchronous=NORMAL`
    (`benchmarks/baseline-axum-sqlite`). File-backed on purpose: Lithair fsyncs
    its event log, so an in-memory baseline would be an unfair write win. This
    is durable-vs-durable.
- **Per run**: a 2k-request warmup (discarded), then `BENCH_TOTAL` requests at
  each concurrency tier.
- **Honesty caveats** — read before quoting these numbers:
  - Single run per tier, **no stddev yet**. Treat as order-of-magnitude.
  - Measured on a **dev box under WSL2** (below), not isolated bench hardware,
    no CPU-governor pinning. Run it on your own hardware — that's the point of
    the one-command harness.
  - The READ path is "list **all** products", which grows during the run —
    pathological for both servers. A point-read (`GET /api/products/:id`) would
    look different; not yet measured.

### Measured on

| | |
|---|---|
| CPU | AMD Ryzen 7 5800X (16 threads) |
| RAM | 31 GiB |
| OS | WSL2, kernel 5.15 |
| toolchain | rustc 1.95.0, `--release` |
| date | 2026-06-23 |

## Results (BENCH_TOTAL=20000)

**CREATE (write) throughput, ops/s — higher is better**

| concurrency | Lithair | Axum+SQLite |
|---|---|---|
| 32  | 378  | 12,697 |
| 128 | 373  | 12,675 |
| 512 | 188  | 11,355 |

**CREATE latency p50 / p99 (ms) — lower is better**

| concurrency | Lithair p50 | Lithair p99 | SQLite p50 | SQLite p99 |
|---|---|---|---|---|
| 32  | 98   | 227   | 0.7  | 18  |
| 128 | 378  | 711   | 4.2  | 28  |
| 512 | 2701 | 8665  | 34   | 180 |

**READ latency p50 / p99 (ms) — lower is better**

| concurrency | Lithair p50 | Lithair p99 | SQLite p50 | SQLite p99 |
|---|---|---|---|---|
| 32  | 1.3 | 2.2   | 6.9  | 19  |
| 128 | 1.6 | 5.1   | 12   | 382 |
| 512 | 1.7 | 230   | 13   | 205 |

## When Lithair wins / loses

**Wins — reads.** In-memory reads are ~1.3–1.7 ms p50 and stay flat as
concurrency rises. The SQLite baseline's reads are ~7–13 ms p50 and degrade
under load (a growing table scan contends with writes on the single writer).
Memory-first is doing exactly what it's for.

**Loses — write throughput, by ~30–60×.** SQLite does ~12k durable inserts/s;
Lithair does ~200–380. Lithair pays for an event-sourced, single-leader durable
append per write, and that path serializes — at concurrency 512 throughput
*drops* (188 ops/s, p99 8.6 s). This is consistent with the documented
single-leader envelope (~210–240 ops/s, [cluster ops](../operations/cluster.md))
and is the headline trade-off: **choose Lithair when reads dominate and the
write rate fits the envelope; don't choose it for write-bound, high-ingest
workloads.** Raising the write ceiling is tracked in
[#130](https://github.com/lithair/lithair/issues/130).

## Not yet measured (deferred)

The gate is satisfied with one baseline and the CRUD dimension; these are named
here rather than half-done, and the harness makes each a small addition:

- **Actix + Postgres** — the conventional production stack (a second baseline).
- **Memory growth** over sustained writes, with and without `#[retention]`.
- **Cold-start replay** time vs event-log size, with and without snapshots.
- **Point-read** latency (`GET /api/products/:id`) vs the list path used here.
- **Variance discipline** — multiple runs + stddev, pinned CPU governor.

Tracked in [#126](https://github.com/lithair/lithair/issues/126).
