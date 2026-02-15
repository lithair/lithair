# Replication Stress Test

Minimal Lithair example for testing and debugging replication issues.

## Purpose

This example isolates replication behavior from application complexity to help diagnose issues like:
- "Failed to reach majority" errors
- Inconsistencies between nodes after rolling upgrades
- Replication failures under load
- Node recovery after crash/restart

## Quick Start

### 1. Start the Cluster

```bash
# Using bombarder (recommended)
cargo run --bin bombarder -- cluster start

# Or manually in separate terminals:
# Terminal 1 (Leader)
cargo run --bin stress_node -- --node-id 0 --port 8080 --peers 8081,8082

# Terminal 2 (Follower 1)
cargo run --bin stress_node -- --node-id 1 --port 8081 --peers 8080,8082

# Terminal 3 (Follower 2)
cargo run --bin stress_node -- --node-id 2 --port 8082 --peers 8080,8081
```

### 2. Run Stress Tests

```bash
# Basic flood test (100 creates, 10 concurrent)
cargo run --bin bombarder -- flood --count 100 --concurrency 10

# Heavy load test
cargo run --bin bombarder -- flood --count 1000 --concurrency 50

# Storm test - random ops on random nodes (recommended for real testing)
cargo run --bin bombarder -- storm --count 500 --concurrency 50

# Storm with chaos - kills/restarts nodes during load
cargo run --bin bombarder -- storm --count 1000 --concurrency 50 --chaos --kill-interval 10
```

### 3. Verify Consistency

```bash
# Check if all nodes have the same data
cargo run --bin bombarder -- verify
```

### 4. Monitor Health

```bash
# Real-time cluster health monitoring
cargo run --bin bombarder -- watch

# One-time status check
cargo run --bin bombarder -- cluster status

# Detailed sync status per follower (lag, health, failures)
cargo run --bin bombarder -- cluster sync-status
```

## Bombarder Commands

### Cluster Management

| Command | Options | Description |
|---------|---------|-------------|
| `cluster start` | `--port N` | Start a 3-node cluster (default: 8080-8082) |
| `cluster stop` | | Stop all cluster nodes |
| `cluster status` | `--nodes URL1,URL2,URL3` | Show cluster health and entry counts |
| `cluster sync-status` | `--leader URL` | Detailed sync status per follower |
| `cluster resync` | `--leader URL --target ADDR` | Force snapshot resync to a follower |

### Stress Tests

| Command | Options | Description |
|---------|---------|-------------|
| `flood` | `--count N`, `--concurrency C` | Rapid concurrent creates to leader |
| `burst` | `--size N`, `--bursts B`, `--pause MS` | Batched writes with pauses |
| `mixed` | `--count N` | Mix of CRUD operations |
| `storm` | `--count N`, `--concurrency C`, `--read-pct P` | Random ops on random nodes |
| `storm --chaos` | `--kill-interval S` | Storm + random node kills/restarts |

### Diagnostics

| Command | Options | Description |
|---------|---------|-------------|
| `verify` | `--nodes URL1,URL2,URL3` | Check data consistency across nodes |
| `watch` | `--leader URL`, `--interval S` | Real-time health monitoring |

## API Endpoints

Each node exposes:

### Data Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/kv` | GET | List all KV entries |
| `/api/kv` | POST | Create entry (replicated) |
| `/api/kv/:id` | GET | Get entry by ID |
| `/api/kv/:id` | PUT | Update entry (replicated) |
| `/api/kv/:id` | DELETE | Delete entry (replicated) |
| `/status` | GET | Node status |

### Raft/Cluster Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/_raft/health` | GET | Cluster health (term, commit index, role) |
| `/_raft/sync-status` | GET | Detailed follower sync status (leader only) |
| `/_raft/resync_stats` | GET | Snapshot resync statistics |
| `/_raft/force-resync?target=ADDR` | POST | Force snapshot resync to follower (leader only) |
| `/_raft/append` | POST | Internal: log replication |
| `/_raft/snapshot` | GET/POST | Internal: snapshot transfer |

## Ops Manual Intervention

### Scenario: Node crashed and needs resync

When a node crashes and restarts, it will automatically resync via snapshot if it's too far behind. However, you can also force a manual resync:

```bash
# 1. Check which nodes are lagging/desynced
cargo run --bin bombarder -- cluster sync-status --leader http://127.0.0.1:8080

# Example output:
#   127.0.0.1:8081 (healthy)    - Last replicated: 150 | Lag: 0
#   127.0.0.1:8082 (desynced)   - Last replicated: 50  | Lag: 100

# 2. Force resync the desynced node
cargo run --bin bombarder -- cluster resync \
  --leader http://127.0.0.1:8080 \
  --target 127.0.0.1:8082

# 3. Verify the node is back in sync
cargo run --bin bombarder -- cluster sync-status --leader http://127.0.0.1:8080
```

### Direct API calls

```bash
# Get sync status
curl http://127.0.0.1:8080/_raft/sync-status | jq

# Force resync
curl -X POST "http://127.0.0.1:8080/_raft/force-resync?target=127.0.0.1:8082"

# Check resync statistics
curl http://127.0.0.1:8080/_raft/resync_stats | jq
```

## KVEntry Model

```rust
pub struct KVEntry {
    pub id: Uuid,           // Primary key
    pub key: String,        // Indexed for lookups
    pub value: String,      // Payload
    pub seq: u64,           // Sequence number for ordering
    pub source_node: u64,   // Which node created this
    pub created_at: DateTime<Utc>,
}
```

## Testing Scenarios

### Scenario 1: Basic Replication

```bash
# 1. Start cluster
cargo run --bin bombarder -- cluster start

# 2. Create an entry
curl -X POST http://localhost:8080/api/kv \
  -H "Content-Type: application/json" \
  -d '{"key":"test1","value":"hello"}'

# 3. Verify on followers
curl http://localhost:8081/api/kv
curl http://localhost:8082/api/kv
```

### Scenario 2: Storm Test (Recommended)

```bash
# Storm distributes load across ALL nodes with random operations
cargo run --bin bombarder -- storm --count 500 --concurrency 50

# Check consistency
cargo run --bin bombarder -- verify
```

### Scenario 3: Chaos Testing

```bash
# Storm with automatic node kills/restarts every 10 seconds
cargo run --bin bombarder -- storm \
  --count 2000 \
  --concurrency 100 \
  --chaos \
  --kill-interval 10

# Should survive with 100% success rate and consistent data
```

### Scenario 4: Node Recovery

```bash
# 1. Start cluster and add data
cargo run --bin bombarder -- cluster start
cargo run --bin bombarder -- flood --count 100

# 2. Kill a follower
pkill -f "stress_node.*--port 8082"

# 3. Add more data while node is down
cargo run --bin bombarder -- flood --count 100

# 4. Check sync status (node 8082 should be desynced)
cargo run --bin bombarder -- cluster sync-status

# 5. Restart the node - it will auto-resync
cargo run --bin stress_node -- --node-id 2 --port 8082 --peers 8080,8081 &

# 6. Verify resync completed
cargo run --bin bombarder -- cluster sync-status
cargo run --bin bombarder -- verify
```

## Expected Results

A healthy cluster should show:
- **Success Rate**: 100% on storm/flood tests
- **Consistency**: All nodes have identical data (matching hashes)
- **Latency**: P99 < 50ms under normal load
- **Chaos Survival**: 100% success rate even with node kills

## Troubleshooting

### "Failed to reach majority" errors

This error occurs when the leader cannot get acknowledgment from enough followers. Check:
- Are all nodes running? (`cluster status`)
- Network connectivity between nodes
- Use `sync-status` to see follower health

### Inconsistent data across nodes

If `verify` shows different hashes:
- Check replication lag with `sync-status`
- Wait for followers to catch up
- Force resync if needed: `cluster resync --target <addr>`

### Node stuck in "desynced" state

```bash
# Force a snapshot resync
cargo run --bin bombarder -- cluster resync \
  --leader http://127.0.0.1:8080 \
  --target 127.0.0.1:8082
```

### High CPU at idle

If nodes consume CPU when idle, check the logs for repeated replication attempts. The incremental catchup should prevent this - if not, there may be a bug in follower index tracking.

## Architecture

```
                    ┌─────────────────┐
                    │   Bombarder     │
                    │  (Test Client)  │
                    └────────┬────────┘
                             │ HTTP requests
            ┌────────────────┼────────────────┐
            │                │                │
            ▼                ▼                ▼
    ┌───────────────┐ ┌───────────────┐ ┌───────────────┐
    │   Node 8080   │ │   Node 8081   │ │   Node 8082   │
    │   (Leader)    │ │  (Follower)   │ │  (Follower)   │
    └───────┬───────┘ └───────────────┘ └───────────────┘
            │              ▲                   ▲
            │   /_raft/append (log entries)   │
            └──────────────┴───────────────────┘
                    │
                    │ /_raft/snapshot (if desynced)
                    └─────────────────────────────────►
```

## Limitations (Current Implementation)

| Feature | Status | Notes |
|---------|--------|-------|
| Leader election | Static | Node 0 is always leader |
| Quorum writes | Best-effort | Async replication, not strict quorum |
| Split-brain protection | Not implemented | Single leader assumed |
| TLS inter-node | Not implemented | Plaintext communication |
| Network partition handling | Basic | Followers marked desynced after failures |

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `STRESS_TEST_DATA` | `data` | Base directory for node data |
| `RUST_LOG` | `info` | Log level (debug for verbose) |

## Cleanup

```bash
# Stop cluster
cargo run --bin bombarder -- cluster stop

# Remove data
rm -rf data/
```
