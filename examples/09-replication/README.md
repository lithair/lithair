# Lithair Distributed Replication Demo

Multi-node Lithair cluster with automatic Raft-based data replication.

## Architecture

```
Node 0 (Leader)     Node 1 (Follower)    Node 2 (Follower)
┌──────────────┐    ┌────────────────┐    ┌────────────────┐
│ Port: 8080   │◄──►│ Port: 8081     │◄──►│ Port: 8082     │
│ Data: node_0 │    │ Data: node_1   │    │ Data: node_2   │
└──────────────┘    └────────────────┘    └────────────────┘
                      Raft Consensus
```

## Quick Start

### Start a 3-node cluster

```bash
# Terminal 1 - Leader
cargo run --bin replication-declarative-node -- --node-id 0 --port 8080 --peers 8081,8082

# Terminal 2 - Follower
cargo run --bin replication-declarative-node -- --node-id 1 --port 8081 --peers 8080,8082

# Terminal 3 - Follower
cargo run --bin replication-declarative-node -- --node-id 2 --port 8082 --peers 8080,8081
```

Or use the helper script:

```bash
bash start-cluster.sh
```

### Test replication

```bash
# Create a product on the leader
curl -X POST http://localhost:8080/api/products \
  -H "Content-Type: application/json" \
  -d '{"name":"Laptop","price":999.99,"category":"Electronics"}'

# Read from any follower (data is replicated)
curl http://localhost:8081/api/products
curl http://localhost:8082/api/products

# Check cluster health
curl http://localhost:8080/_raft/health
```

### Run automated tests

```bash
./test.sh
```

## Binaries

| Binary | Purpose |
|--------|---------|
| `replication-declarative-node` | Cluster node (leader or follower) |
| `replication-loadgen` | HTTP load generator for benchmarking |

## CLI Arguments

```
--node-id <ID>              # Unique node ID (required)
--port <PORT>               # Listening port (default: 8080)
--peers "<PORT1>,<PORT2>"   # Peer ports on localhost
```

## Load Generator

```bash
# Status endpoint benchmark
cargo run --bin replication-loadgen -- --leader http://127.0.0.1:8080 --total 10000 --concurrency 256 --mode perf-status

# JSON payload benchmark
cargo run --bin replication-loadgen -- --leader http://127.0.0.1:8080 --total 10000 --concurrency 256 --mode perf-json --perf-bytes 1024
```

## Environment Variables

Persistence tuning via `LT_` variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `LT_OPT_PERSIST` | `0` | Enable async writer thread |
| `LT_BUFFER_SIZE` | `1048576` | Write buffer size (bytes) |
| `LT_MAX_EVENTS_BUFFER` | `2000` | Events to buffer before flush |
| `LT_FLUSH_INTERVAL_MS` | `5` | Periodic flush interval |
| `LT_FSYNC_ON_APPEND` | `0` | fsync on every append |
| `LT_ENABLE_BINARY` | `0` | Binary serialization mode |

## See Also

- [10-blog-distributed](../10-blog-distributed/) - Blog + replication combined
- [advanced/http-firewall](../advanced/http-firewall/) - IP filtering and rate limiting
- [advanced/http-hardening](../advanced/http-hardening/) - CORS, timeouts, security headers
- [advanced/consistency-test](../advanced/consistency-test/) - Automated consistency verification
