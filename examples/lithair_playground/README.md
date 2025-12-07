# Lithair Playground

Interactive showcase demonstrating ALL Lithair capabilities in a single demo.

## Features

- **Live Replication Visualization** - Watch data replicate across nodes in real-time
- **Integrated Benchmarks** - Run write/read/mixed benchmarks with live progress
- **Cluster Control** - Monitor node health, see leader elections
- **Data Explorer** - CRUD operations with instant replication feedback
- **Performance Metrics** - Ops/sec, latency histograms, WAL stats

## Quick Start

```bash
# Start 3-node cluster
./run_playground.sh start

# Open the Playground UI
open http://localhost:8080

# Stop cluster
./run_playground.sh stop

# Clean and restart
./run_playground.sh restart
```

## Manual Start

```bash
# Terminal 1 - Node 0 (initial leader)
cargo run --bin playground_node -- --node-id 0 --port 8080 --peers 8081,8082

# Terminal 2 - Node 1
cargo run --bin playground_node -- --node-id 1 --port 8081 --peers 8080,8082

# Terminal 3 - Node 2
cargo run --bin playground_node -- --node-id 2 --port 8082 --peers 8080,8081
```

## Endpoints

### Playground API
| Endpoint | Method | Description |
|----------|--------|-------------|
| `/_playground/cluster/status` | GET | Full cluster status |
| `/_playground/benchmark/start` | POST | Start benchmark |
| `/_playground/benchmark/status` | GET | Benchmark progress |
| `/_playground/benchmark/stop` | POST | Stop benchmark |
| `/_playground/events/replication` | GET | SSE replication events |
| `/_playground/events/cluster` | GET | SSE cluster events |
| `/_playground/events/benchmark` | GET | SSE benchmark events |

### Data API
| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/items` | GET | List all items |
| `/api/items` | POST | Create item (replicated) |
| `/api/items/:id` | GET | Get item |
| `/api/items/:id` | PUT | Update item (replicated) |
| `/api/items/:id` | DELETE | Delete item (replicated) |

### Admin
| Endpoint | Method | Description |
|----------|--------|-------------|
| `/_admin` | GET | Admin UI dashboard |
| `/_raft/health` | GET | Raft cluster health |

## Benchmark Configuration

```json
{
  "benchmark_type": "write",  // "write", "read", or "mixed"
  "concurrency": 10,          // Number of concurrent workers
  "duration_secs": 10,        // Test duration
  "payload_size": 256         // Payload size in bytes
}
```

## Testing Failover

1. Start the cluster with `./run_playground.sh start`
2. Open http://localhost:8080 (Leader)
3. Note which node is the leader
4. Kill the leader: `kill $(cat data/node_0.pid)`
5. Watch the UI update as a new leader is elected
6. Restart the node: start it manually or use `./run_playground.sh restart`

## Architecture

```
lithair_playground/
├── Cargo.toml                 # Dependencies
├── PLAN.md                    # Detailed implementation plan
├── README.md                  # This file
├── run_playground.sh          # Cluster management script
├── src/
│   ├── main.rs                # Entry point, server setup
│   ├── models.rs              # PlaygroundItem model
│   ├── playground_api.rs      # API handlers
│   ├── benchmark.rs           # Benchmark engine
│   └── sse_events.rs          # SSE broadcasting
└── frontend/
    └── index.html             # Single-page UI
```

## Lithair Features Demonstrated

| Feature | Demo |
|---------|------|
| Raft Consensus | Leader election, automatic failover |
| Log Replication | Live commit index, term tracking |
| SCC2 Engine | Lock-free concurrent operations |
| DeclarativeModel | Automatic CRUD generation |
| Event Sourcing | WAL persistence |
| Admin UI | Data browser, cluster monitoring |
| HTTP Server | Hyper-based routing |

## Development

```bash
# Build
cargo build --bin playground_node

# Run with debug logging
RUST_LOG=debug cargo run --bin playground_node -- --node-id 0 --port 8080

# Run tests
cargo test -p lithair_playground
```

## See Also

- [PLAN.md](./PLAN.md) - Detailed implementation plan
- [blog_replicated_demo](../blog_replicated_demo/) - Blog with replication
- [scc2_server_demo](../scc2_server_demo/) - Pure performance demo
