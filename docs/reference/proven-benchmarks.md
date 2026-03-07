# 🔥 Lithair Benchmark Notes

## Executive Summary

This page documents benchmark-oriented observations from the **current public
catalog**, not from a deleted historical demo.

The main public reference for distributed benchmarking is now
`examples/09-replication` together with:

- `replication-declarative-node`
- `replication-loadgen`
- `bench_1000_crud_parallel.sh`
- `bench_http_server_stateless.sh`

Benchmark numbers in Lithair should be read as **scenario-specific results**.
They depend on workload mix, storage profile, read path, concurrency, and node
topology.

---

## What the Current Benchmark Surface Demonstrates

The current replication package is useful because it exercises several concerns
together from a single public example surface:

- declarative model exposure over HTTP
- event-sourced persistence
- multi-node replication
- benchmark scripting and load generation
- operational checks such as `/status` and lightweight read endpoints

This is enough to evaluate the framework under realistic CRUD and replication
scenarios without relying on old internal or deleted demos.

---

## Representative Findings

Typical benchmark runs in the replication example help answer questions like:

- how much read path choice affects throughput and latency
- how write-heavy workloads behave under different storage profiles
- how replication tails evolve as concurrency rises
- how much simpler operational benchmarking becomes when the example already
  exposes the right endpoints and scripts

The exact numbers are expected to move over time as Lithair evolves.

---

## Why This Matters for the Data-First Model

The value of these benchmarks is not that one fixed number "proves" the whole
framework.

The useful part is that a single declarative model can drive multiple layers at
once:

- persistence behavior
- HTTP exposure
- validation
- replication participation
- permission rules

That reduces the amount of coordination work needed between separate schema,
API, and infrastructure layers.

---

## Current Runnable Entry Points

### Distributed replication example

```bash
cd examples/09-replication
cargo run -p replication --bin replication-declarative-node -- --node-id 1 --port 8080
```

### Load generator

```bash
cargo run --release -p replication --bin replication-loadgen -- \
  --leader http://127.0.0.1:8080 \
  --total 3000 \
  --concurrency 256 \
  --mode random
```

### Scripted CRUD benchmark

```bash
bash examples/09-replication/bench_1000_crud_parallel.sh 3000
```

### Stateless HTTP benchmark

```bash
bash examples/09-replication/bench_http_server_stateless.sh
```

---

## Reading Results Correctly

When you evaluate Lithair benchmark output, prefer this interpretation:

- **Throughput** tells you how a given workload shape behaves under a given
  configuration
- **Latency percentiles** tell you whether tail behavior is still acceptable
- **Consistency checks** confirm whether replicated state converges as expected
- **Scenario scripts** make regressions reproducible over time

That is more useful than treating a single old benchmark as a timeless reference
artifact.

---

## Related Documents

- `./http-loadgen.md` – CLI and benchmark guidance
- `../guides/data-first-philosophy.md` – conceptual framing
- `../../examples/09-replication/README.md` – current scenario entry point
- `../guides/performance.md` – benchmark entry points and validation workflow

---

## Conclusion

Lithair still has a meaningful benchmark story, but it now lives in the
**current runnable catalog** rather than in deleted historical demos.

If you want to benchmark Lithair today, use `examples/09-replication` and its
scripts as the reference surface.
