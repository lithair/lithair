# Lithair HTTP Load Generator (http_loadgen_demo)

Purpose-built, high-performance HTTP load generator used to validate Lithair end-to-end, in single-node and multi-node (3-node) settings. The tool evolves alongside the framework: each time we improve Lithair, we extend the load generator to exercise new features and prove robustness at scale.

## Key Goals

- End-to-end validation: from HTTP API to EventStore persistence and cluster replication
- Realistic workloads: create/read/update mixes, bulk ingestion, light vs heavy reads
- Repeatability: simple flags to reproduce scenarios and compare results over time
- Evolution: add features to the loadgen as Lithair gains capabilities (consensus, security, multi-model, etc.)

## Quick Start

```bash
# Build
cargo build --release -p raft_replication_demo --bin http_loadgen_demo

# Run single-node 10k CREATE (bulk mode)
./target/release/http_loadgen_demo \
  --leader http://127.0.0.1:8080 \
  --total 10000 \
  --concurrency 512 \
  --bulk-size 1000 \
  --mode bulk
```

Output summary line:
```
Loadgen (demo) completed: total=10000 dur=2.68s throughput=3729.10 ops/s mode=bulk bulk_size=1000 concurrency=512 leader=http://127.0.0.1:8080
```

## CLI Reference

Flags correspond to `examples/raft_replication_demo/http_loadgen_demo.rs` (`Args`):

- --leader <url>
  - Base URL of the leader, e.g. `http://127.0.0.1:8080`
- --total <n>
  - Total operations. In `single`/`random` mode this is per-request unit; in `bulk` mode this is total items (split into batches of `bulk-size`).
- --concurrency <n>
  - Number of in-flight requests. Concurrency applies to both single and bulk requests.
- --bulk-size <n>
  - Items per `_bulk` request (only meaningful in `bulk` mode). Each bulk request posts an array of size `bulk-size`.
- --mode <single|bulk|random>
  - `single`: N POST /api/products (one item per request)
  - `bulk`: ⌈N/bulk⌉ POST /api/products/_bulk (arrays)
  - `random`: mix of CREATE/READ/UPDATE controlled by percentages
- --create-pct <0..100>
- --read-pct <0..100>
- --update-pct <0..100>
- --delete-pct <0..100>
  - Only used when `--mode random`. Percentages are normalized; if they do not sum to 100, they are scaled proportionally.
- --read-targets <csv>
  - Comma‑separated target base URLs used by `READ` operations in `random` mode (e.g., `http://127.0.0.1:8080,http://127.0.0.1:8081,http://127.0.0.1:8082`).
- --read-path <path>
  - Path for `READ` operations. Examples: `/api/products` (heavy), `/status` (very light), `/api/products/count` (light).
- --timeout-s <secs>
  - Per-request timeout.

## Read Workloads: Heavy vs Light

The bench script (`examples/raft_replication_demo/bench_1000_crud_parallel.sh`) exposes `LIGHT_READS` to control `--read-path`:

- LIGHT_READS=0 → `/api/products` (heavy: full list JSON)
- LIGHT_READS=1|true|status → `/status` (very light)
- LIGHT_READS=count → `/api/products/count` (light)

Lightweight server endpoints are implemented in `lithair-core/src/http/declarative.rs`:

- `GET /api/{model}/count` → `{ "count": N }`
- `GET /api/{model}/random-id` → `{ "id": "..." }`

In update-heavy mixes, the loadgen uses the in-memory ID pool captured from CREATE responses. If empty, it falls back to `GET /api/products/random-id` (lightweight) rather than listing the whole collection.

On DELETE, an ID is selected from the pool (or fetched via `random-id`), the item is deleted, and the ID is removed from the pool on success to avoid repeated deletes.

## Common Scenarios (Cookbook)

- 100% CREATE, single mode
```bash
./target/release/http_loadgen_demo \
  --leader http://127.0.0.1:8080 \
  --total 10000 \
  --concurrency 1024 \
  --mode single
```

- 100% CREATE, bulk ingestion
```bash
./target/release/http_loadgen_demo \
  --leader http://127.0.0.1:8080 \
  --total 20000 \
  --concurrency 1024 \
  --bulk-size 1000 \
  --mode bulk
```

- Mixed workload (80% C / 15% R / 5% U) on 3 nodes with light reads
```bash
./target/release/http_loadgen_demo \
  --leader http://127.0.0.1:8080 \
  --total 3000 \
  --concurrency 1024 \
  --mode random \
  --create-pct 80 --read-pct 15 --update-pct 5 \
  --read-targets "http://127.0.0.1:8080,http://127.0.0.1:8081,http://127.0.0.1:8082" \
  --read-path /api/products/count
```

- READ A/B heavy vs light after pre-seed (via bench script)
```bash
# Heavy list
LIGHT_READS=0 PRESEED_PER_NODE=5000 CREATE_PERCENTAGE=0 READ_PERCENTAGE=100 UPDATE_PERCENTAGE=0 \
  ./examples/raft_replication_demo/bench_1000_crud_parallel.sh 3000
# Light count
LIGHT_READS=count PRESEED_PER_NODE=5000 CREATE_PERCENTAGE=0 READ_PERCENTAGE=100 UPDATE_PERCENTAGE=0 \
  ./examples/raft_replication_demo/bench_1000_crud_parallel.sh 3000
```

## Integration with Bench Script

The bench script orchestrates multi‑node clusters, readiness checks, and post‑run validations:

- Cleans data directories (including legacy), starts 1 or 3 nodes
- Polls `/status` until ready
- Runs `http_loadgen_demo` with arguments derived from env vars
- Validates convergence (counts + full list comparison) and persistence (.raftlog)

Useful env vars when using the script:

- `SINGLE_NODE=1` – run one node instead of 3
- `LOADGEN_MODE=single|bulk|random`
- `LOADGEN_BULK_SIZE=<n>`
- `LOADGEN_CONCURRENCY=<n>`
- `CREATE_PERCENTAGE|READ_PERCENTAGE|UPDATE_PERCENTAGE`
- `LIGHT_READS=0|1|status|count`
- `PRESEED_PER_NODE=<n>` – optional pre‑seed run (100% CREATE) before main workload

## Measurement & Output

At the end of a run, the tool prints a summary line with:

- total – number of items processed
- dur – wall time for the run
- throughput – operations per second (ops/s)
- mode – single/bulk/random
- bulk_size, concurrency, leader

Example:
```
Loadgen (demo) completed: total=15000 dur=3.42s throughput=4380.94 ops/s mode=bulk bulk_size=100 concurrency=1024 leader=http://127.0.0.1:8080
```

### Latency Metrics (p50/p95/p99)

The load generator also prints latency percentiles per operation type (CREATE/READ/UPDATE/DELETE) and the arithmetic mean. Values are in milliseconds. These are computed in-memory (no export yet) and help surface tail latency under load:

```
Latency percentiles (milliseconds):
    CREATE (count=256): p50=106.14ms p95=188.53ms p99=190.21ms mean=132.23ms
      READ (count=122): p50=109.40ms p95=187.54ms p99=189.22ms mean=136.23ms
    UPDATE (count=79):  p50=2.69ms   p95=72.47ms  p99=81.47ms  mean=10.60ms
    DELETE (count=7):   p50=6.34ms   p95=74.58ms  p99=77.93ms  mean=23.71ms
```

These metrics complement the throughput line and are key to tracking tail latency regressions.

## Best Practices

- Use light reads (`/status` or `/count`) when you want to isolate write/consensus/persistence cost
- Pre‑seed data (`PRESEED_PER_NODE`) before UPDATE‑only or READ‑only tests
- For ingestion ceilings, use `bulk` mode with large `bulk-size` (e.g., 500–1000)
- For durability testing, switch to `STORAGE_PROFILE=durable_security` in the bench script
- Compare against baselines and record runs in `baseline_results/`

## Recommended Defaults

- `STORAGE_PROFILE=high_throughput`: set `LOADGEN_CONCURRENCY=256` for a strong throughput vs p95/p99 tail balance on 3‑node clusters.
- `STORAGE_PROFILE=balanced` or `durable_security`: prefer lower concurrency (≤512) to avoid large CREATE tails (fsync/index/dedup, smaller buffers).
- When using `BENCH_SUITE=durability_profiles` in `examples/raft_replication_demo/bench_1000_crud_parallel.sh`, the script restarts the cluster per profile so each round truly runs with the intended settings.

## Heavy vs Light: Observations (latest)

Measured on a 3‑node cluster with `PRESEED_PER_NODE=50000`, `LOADGEN_CONCURRENCY=256`, read‑only workload (`CREATE=0 READ=100 UPDATE=0 DELETE=0`), 3000 ops per round:

- Heavy list (`LIGHT_READS=0` → `GET /api/products`)
  - Throughput ≈ 38.55–38.57 ops/s
  - READ latency: p50 ≈ 6.05–6.15 s, p95 ≈ 10.0 s, p99 ≈ 10.0 s
  - Guidance: extremely heavy. Avoid for performance benchmarks; only use to stress worst‑case read path.

- Light count (`LIGHT_READS=count` → `GET /api/products/count`)
  - Throughput ≈ 10.3k–15.3k ops/s
  - READ latency: p50 ≈ 2.15–2.89 ms, p95 ≈ 115–128 ms, p99 ≈ 125–139 ms
  - Guidance: recommended for read validation with realistic tails.

- Status (`LIGHT_READS=status` → `GET /status`)
  - Throughput ≈ 15.1k–24.6k ops/s
  - READ latency: p50 ≈ 1.20–2.33 ms, p95 ≈ 79–169 ms, p99 ≈ 90–175 ms
  - Guidance: the lightest endpoint; good for isolating write/consensus/persistence costs.

## Benchmark Suites (via bench script)

The orchestration script supports preset suites that run multiple rounds while the cluster is up. This is convenient for systematic comparisons. See `README_BENCHMARKS.md` for details. Quick usage:

- Concurrency scaling
```bash
BENCH_SUITE=concurrency_scaling \
LIGHT_READS=count CREATE_PERCENTAGE=60 READ_PERCENTAGE=25 UPDATE_PERCENTAGE=10 DELETE_PERCENTAGE=5 \
examples/raft_replication_demo/bench_1000_crud_parallel.sh 5000
# Customize concurrency list via CONC_LIST="128 256 512 1024 2048"
```

- Heavy vs light reads (requires pre-seed)
```bash
PRESEED_PER_NODE=50000 \
BENCH_SUITE=heavy_vs_light_reads \
CREATE_PERCENTAGE=0 READ_PERCENTAGE=100 UPDATE_PERCENTAGE=0 DELETE_PERCENTAGE=0 \
examples/raft_replication_demo/bench_1000_crud_parallel.sh 3000
```

- Bulk vs single ingestion
```bash
BENCH_SUITE=bulk_vs_single \
LOADGEN_BULK_SIZE=500 LOADGEN_MODE=bulk \
examples/raft_replication_demo/bench_1000_crud_parallel.sh 10000
```

## Roadmap (Extending http_loadgen)

- Latency metrics (p50/p95/p99), error rates, CSV/JSON export
- DELETE operations support and balanced CRUD mixes
- Failure injection (timeouts, broken connections) for robustness tests
- JWT/auth headers for secure endpoints (integration with RBAC demo)
- Multi‑model workloads (e.g., users, orders, comments) with cross‑entity sequences
- Distributed scenarios with leader changes, partitions, and recovery windows
- Long‑running soak tests and memory/CPU profiling integrations

This tool is part of our continuous performance and robustness validation strategy: each time Lithair improves, we update the load generator to cover the new surface area and keep proving the system end‑to‑end.
