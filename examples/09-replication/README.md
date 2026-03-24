# 🚀 Lithair Distributed Replication Demo

Demo of a multi-node Lithair cluster with automatic data replication.

## 🎯 Objective

This example shows how to:
- Configure a distributed Lithair cluster
- Automatically replicate data between nodes
- Use the declarative model with persistence attributes
- Handle leader redirection and HTTP replication (full OpenRaft: WIP)

## 🏗️ Architecture

```
Node 1 (Leader)     Node 2 (Follower)    Node 3 (Follower)
┌──────────────┐    ┌────────────────┐    ┌────────────────┐
│ Port: 8080   │◄───┤ Port: 8081     │    │ Port: 8082     │
│ Data: node1  │    │ Data: node2    │◄───┤ Data: node3    │
└──────────────┘    └────────────────┘    └────────────────┘
        ▲                    ▲                       ▲
        └────────────────────┼───────────────────────┘
                         Raft Consensus
```

## 📋 Features

### Declarative Model with Replication
- **Product**: Product model with primary key, audited fields, and replication
- **Persistence attributes**: `#[persistence(replicate, track_history)]`

### Distributed Events
- User creation/modification
- Message creation/modification
- Per-node replication statistics

## 🚀 Usage

### Starting the Cluster

```bash
# Terminal 1: Start the leader (Node 1)
cargo run --release --bin replication-declarative-node -- \
  --node-id 1 \
  --port 8080 \
  --peers "8081,8082"

# Terminal 2: Start follower (Node 2)
cargo run --release --bin replication-declarative-node -- \
  --node-id 2 \
  --port 8081 \
  --peers "8080,8082"

# Terminal 3: Start follower (Node 3)
cargo run --release --bin replication-declarative-node -- \
  --node-id 3 \
  --port 8082 \
  --peers "8080,8081"
```

### Monitoring Replication

Each node displays its statistics every 10 seconds:
```
=== Node 1 Statistics ===
Users: 2 local, 6 total
Messages: 3 local, 9 total
Replications: 6 received, 12 sent
==============================
```

## 🔧 Configuration

### Declarative Persistence Attributes (simplified excerpt)

```rust
#[derive(DeclarativeModel)]
pub struct Product {
    #[db(primary_key, indexed)]
    #[lifecycle(immutable)]
    #[http(expose)]
    #[persistence(replicate, track_history)]
    pub id: Uuid,

    #[db(indexed, unique)]
    #[lifecycle(audited, retention = 90)]
    #[http(expose, validate = "non_empty")]
    #[persistence(replicate, track_history)]
    pub name: String,
}
```

### Available Options
- `replicate`: Replicate across all cluster nodes
- `track_history`: Retain complete modification history
- `memory_only`: Local data only (no persistence/replication)
- `auto_persist`: Automatic write persistence
- `no_replication`: Exclude from replication even if persisted

## 📊 Monitoring

### Per-Node Metrics
- **users_created**: Users created locally
- **messages_created**: Messages created locally
- **replications_received**: Events received from other nodes
- **replications_sent**: Events sent to other nodes

### Persistence
- Events persisted in a local EventStore per node (`.raftlog` files)
- Periodic snapshots to speed up recovery (if enabled)

## 🧪 Replication Tests

### Tested Scenarios
1. **Distributed creation**: Each node creates users/messages
2. **Unique constraints**: Cross-node duplicate verification
3. **Foreign keys**: Cross-entity relationship consistency
4. **Recovery**: Node restart and catch-up

### Test Execution Order
1. Start all nodes
2. Wait for cluster formation
3. Execute operations in parallel on each node
4. Verify replicated data consistency

## 🔮 Roadmap

- [ ] Full OpenRaft integration (strong consensus)
- [ ] Network partition handling
- [ ] Performance tests under heavy load
- [ ] Web-based cluster monitoring interface

## 🎛️ Command-Line Arguments

```bash
--node-id <ID>              # Unique node ID (required)
--port <PORT>               # Listening port (default: 8080)
--peers "<PORT1>,<PORT2>"   # Other nodes: peer ports on localhost
```

## 💡 Implementation Notes

- HTTP server based on Hyper (HTTP/1.1)
- Automatic write redirection to the leader
- Data replication via HTTP between nodes
- Events serialized as JSON for network transport

## 🧪 Benchmarks

A script is provided to run a distributed CRUD benchmark:

```bash
./bench_1000_crud_parallel.sh 1000
```

See `baseline_results/` at the repo root for representative measurements.

## 🔐 HTTP Hardening Demo (stateless perf + firewall)

The `replication-hardening-node` binary starts a minimal declarative HTTP server to demonstrate:

- Stateless performance endpoints (`/perf/echo`, `/perf/json`, `/perf/bytes`)
- Gzip (content negotiation via `Accept-Encoding`, configurable threshold)
- Per-prefix policies (e.g., force gzip / `no-store` on `/perf`)
- Firewall (allow/deny IP, CIDR, macros `internal`, `loopback`, etc.)

By default, this server starts with a production-like posture:

- `/perf/*` and `/metrics` protected by firewall
- `/status` and `/health` exempted
- `allow` includes the `internal` macro (private IPv4 + ULA IPv6 networks)

To open it locally (disable the default firewall posture):

```bash
cargo run -p replication --bin replication-hardening-node -- --port 18320 --open
```

You can also compile the example in "open by default" mode using a feature flag:

```bash
cargo run -p replication --features open_by_default --bin replication-hardening-node -- --port 18320
```

The stateless bench script automatically starts the server with `--open`:

```bash
bash examples/09-replication/bench_http_server_stateless.sh
```

### Single-Node Mode (Engine/Persistence Isolation)

To isolate network/consensus overhead and measure only the HTTP + engine + persistence cost, you can run the benchmark in **single-node** mode:

```bash
SINGLE_NODE=1 ./bench_1000_crud_parallel.sh 10000
```

Tip: combine with `LT_` variables to compare JSON vs Binary, async on/off:

```bash
# Async JSON (Stage A)
SINGLE_NODE=1 LT_OPT_PERSIST=1 LT_ENABLE_BINARY=0 ./bench_1000_crud_parallel.sh 10000

# Binary (Stage B)
SINGLE_NODE=1 LT_OPT_PERSIST=1 LT_ENABLE_BINARY=1 ./bench_1000_crud_parallel.sh 10000
```

## ⚙️ Runtime (Persistence & Performance)

For realistic high-throughput benchmarks, the demo supports `LT_` environment variables that control EventStore persistence:

- `LT_OPT_PERSIST` (1/0) -- Enables optimized asynchronous writes (writer thread) for events (enabled by default in the bench script).
- `LT_BUFFER_SIZE` (bytes) -- Write buffer size (default 1,048,576 = 1 MB).
- `LT_MAX_EVENTS_BUFFER` -- Number of events to buffer before flushing (default 2000).
- `LT_FLUSH_INTERVAL_MS` -- Periodic flush interval (default 5 ms for benchmarks).
- `LT_FSYNC_ON_APPEND` (1/0) -- fsync on every append (0 recommended for throughput benchmarks).
- `LT_EVENT_MAX_BATCH` -- Internal batch size on the EventStore side (default 65536 in the bench script).
- `LT_ENABLE_BINARY` (1/0) -- Enables binary mode (Stage B): event envelopes are serialized via bincode and written line by line (separated by `\n`). Replay/restore remains compatible: the engine converts back to JSON on read.
- `LT_DISABLE_INDEX` (1/0) -- Disables the `aggregate_id -> offset` index to avoid extra writes during benchmarks.
- `LT_DEDUP_PERSIST` (1/0) -- Controls idempotency ID persistence. Set to `0` for ephemeral benchmarks (no exactly-once cross-restart guarantee needed).

Example manual run with optimized persistence and binary mode:

```bash
export LT_OPT_PERSIST=1
export LT_BUFFER_SIZE=1048576
export LT_MAX_EVENTS_BUFFER=5000
export LT_FLUSH_INTERVAL_MS=2
export LT_FSYNC_ON_APPEND=0
export LT_ENABLE_BINARY=1

./bench_1000_crud_parallel.sh 10000
```

Notes:

- The `bench_1000_crud_parallel.sh` script already exports default values tuned for throughput, including `LT_OPT_PERSIST=1`.
- Binary mode (`LT_ENABLE_BINARY=1`) maximizes append speed (3-5x vs JSON depending on workload) while retaining JSON snapshots.

### Pre-built Storage Profiles (STORAGE_PROFILE)

The bench script supports ready-to-use profiles (selected via `STORAGE_PROFILE=<name>`):

- `high_throughput` (default)
  - Goal: Maximum throughput (benchmarks). Async writer ON, binary ON, index/dedup OFF, large buffers, fsync OFF, widely spaced snapshots.
  - Example:
    ```bash
    STORAGE_PROFILE=high_throughput LOADGEN_MODE=bulk LOADGEN_BULK_SIZE=500 \
    ./bench_1000_crud_parallel.sh 10000
    ```

- `balanced`
  - Goal: Throughput/reliability trade-off. Async ON, binary ON, index/dedup ON, medium buffers, fsync OFF.
  - Example:
    ```bash
    STORAGE_PROFILE=balanced LOADGEN_MODE=bulk LOADGEN_BULK_SIZE=500 \
    ./bench_1000_crud_parallel.sh 10000
    ```

- `durable_security`
  - Goal: Durability and audit trail. Async ON, binary OFF (human-readable), index/dedup ON, fsync ON, frequent snapshots.
  - Example:
    ```bash
    STORAGE_PROFILE=durable_security LOADGEN_MODE=bulk LOADGEN_BULK_SIZE=200 \
    ./bench_1000_crud_parallel.sh 10000
    ```

Each profile automatically configures the appropriate `LT_` variables (buffers, flush, fsync, index, dedup, snapshots) to adapt the engine to the application's needs.

### Data Path (EXPERIMENT_DATA_BASE)

By default, the bench script configures the example database in:

```
EXPERIMENT_DATA_BASE=examples/09-replication/data
```

This path is passed to the engine via the `EXPERIMENT_DATA_BASE` environment variable and overrides `EngineConfig.event_log_path` at startup. You can:

- Leave the default behavior (`.raftlog`/snapshot files are written to the example directory)
- Or override the path:

```bash
EXPERIMENT_DATA_BASE=/tmp/lithair_bench \
STORAGE_PROFILE=high_throughput LOADGEN_MODE=bulk LOADGEN_BULK_SIZE=1000 \
./bench_1000_crud_parallel.sh 100000
```

The script explicitly prints the path used and lists the persisted files at the end of each run.

## 🔦 Lightweight Reads (LIGHT_READS)

To avoid the JSON serialization cost of the full list (`GET /api/products`), the bench supports configurable "lightweight" reads via `LIGHT_READS`:

- `LIGHT_READS=0` (default) -- `GET /api/products` (full list, heavy read)
- `LIGHT_READS=1`, `true`, or `status` -- `GET /status` (very lightweight)
- `LIGHT_READS=count` -- `GET /api/products/count` (lightweight, returns `{ "count": N }`)

Endpoints added by the declarative server (`lithair-core/src/http/declarative.rs`):

- `GET /api/{model}/count` -- Returns only the element count
- `GET /api/{model}/random-id` -- Returns an existing `id` (useful for pre-filling UPDATE targets without listing everything)

### A/B Test: Heavy vs Light

Example after pre-seeding (5,000 objects per node):

```bash
# Heavy read: full list
LIGHT_READS=0 PRESEED_PER_NODE=5000 CREATE_PERCENTAGE=0 READ_PERCENTAGE=100 UPDATE_PERCENTAGE=0 \
  ./bench_1000_crud_parallel.sh 3000

# Light read: counter
LIGHT_READS=count PRESEED_PER_NODE=5000 CREATE_PERCENTAGE=0 READ_PERCENTAGE=100 UPDATE_PERCENTAGE=0 \
  ./bench_1000_crud_parallel.sh 3000
```

Recent observations (3 nodes, PRESEED_PER_NODE=50000, concurrency=256, read-only 3000 ops):

- Heavy read (full list): ~38.6 ops/s, p50 ~6.1 s, p95 ~10 s
- Light read (count): ~10.3k-15.3k ops/s, p50 ~2-3 ms, p95 ~115-128 ms
- Status: ~15.1k-24.6k ops/s, p50 ~1-2 ms, p95 ~80-170 ms

Recommendations:
- Avoid `GET /api/products` for performance benchmarks; use `/count` or `/status` instead.
- Profile `high_throughput`: default `LOADGEN_CONCURRENCY=256` provides the best throughput/tail-latency trade-off.
- Profiles `balanced` and `durable_security`: stay at 512 or below to keep write tail latencies in check.
- The `BENCH_SUITE=durability_profiles` suite restarts the cluster for each profile to correctly apply storage parameters.

Tip: for workloads with a high UPDATE proportion, the loadgen now uses `GET /api/products/random-id` to fetch a lightweight `id` when the ID pool is empty (instead of `GET /api/products`).
