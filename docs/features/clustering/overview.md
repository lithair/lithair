# Clustering Overview

Lithair provides built-in clustering capabilities for horizontal scaling and high availability through leader-follower replication.

## Architecture

```text
                    ┌─────────────────┐
                    │    Clients      │
                    └────────┬────────┘
                             │
            ┌────────────────┼────────────────┐
            ▼                ▼                ▼
    ┌───────────────┐ ┌───────────────┐ ┌───────────────┐
    │   Node 0      │ │   Node 1      │ │   Node 2      │
    │   (Leader)    │ │  (Follower)   │ │  (Follower)   │
    │   Port 8080   │ │   Port 8081   │ │   Port 8082   │
    └───────┬───────┘ └───────────────┘ └───────────────┘
            │              ▲                   ▲
            │     Log Replication              │
            └──────────────┴───────────────────┘
```

## Key Concepts

### Leader/Follower Model

- **Leader**: Handles all write operations, replicates to followers
- **Followers**: Receive replicated data, can serve read operations
- **Static assignment**: Node 0 is always the leader (no automatic election)

### Replication Flow

1. Client sends write request to leader
2. Leader appends to local consensus log
3. Leader replicates entry to followers via `/_raft/append`
4. Followers acknowledge receipt
5. Leader commits and responds to client

### Snapshot Resync

When a follower falls too far behind (marked as "desynced"):

1. Leader creates a snapshot of current state
2. Leader sends snapshot to follower via `/_raft/snapshot`
3. Follower installs snapshot and resumes normal replication

## Quick Start

### Enable Clustering

```rust
use lithair_core::app::LithairServer;

// Leader node (node_id = 0 is always the leader)
LithairServer::new()
    .with_port(8080)
    .with_raft_cluster(0, vec!["127.0.0.1:8081", "127.0.0.1:8082"])
    .with_model::<Product>("./data/products", "/api/products")
    .serve()
    .await?;

// Follower node
LithairServer::new()
    .with_port(8081)
    .with_raft_cluster(1, vec!["127.0.0.1:8080", "127.0.0.1:8082"])
    .with_model::<Product>("./data/products", "/api/products")
    .serve()
    .await?;
```

### Cluster Configuration

| Node | Port | node_id | Role | Peers |
|------|------|---------|------|-------|
| Leader | 8080 | 0 | Leader (static) | 8081, 8082 |
| Follower 1 | 8081 | 1 | Follower | 8080, 8082 |
| Follower 2 | 8082 | 2 | Follower | 8080, 8081 |

> **Note**: Node 0 is always the leader. This is a static assignment—no automatic election.

## Monitoring Endpoints

### Health Check

```bash
curl http://127.0.0.1:8080/_raft/health
```

Response:
```json
{
  "node_id": 0,
  "state": "Leader",
  "term": 1,
  "commit_index": 150,
  "last_applied": 150,
  "followers": {
    "127.0.0.1:8081": "healthy",
    "127.0.0.1:8082": "healthy"
  }
}
```

### Sync Status (Leader only)

```bash
curl http://127.0.0.1:8080/_raft/sync-status
```

Response:
```json
{
  "node_id": 0,
  "is_leader": true,
  "commit_index": 150,
  "followers": [
    {
      "address": "127.0.0.1:8081",
      "health": "healthy",
      "last_replicated_index": 150,
      "lag": 0,
      "last_latency_ms": 2,
      "consecutive_failures": 0
    },
    {
      "address": "127.0.0.1:8082",
      "health": "healthy",
      "last_replicated_index": 150,
      "lag": 0,
      "last_latency_ms": 1,
      "consecutive_failures": 0
    }
  ]
}
```

### Resync Statistics

```bash
curl http://127.0.0.1:8080/_raft/resync_stats
```

Response:
```json
{
  "node_id": 0,
  "is_leader": true,
  "resync_stats": {
    "snapshots_created": 2,
    "send_attempts": 3,
    "send_successes": 2,
    "send_failures": 1
  }
}
```

## Ops Manual Intervention

### Force Resync a Follower

When a follower is stuck in "desynced" state or needs manual recovery:

```bash
# Check current sync status
curl http://127.0.0.1:8080/_raft/sync-status

# Force snapshot resync to specific follower
curl -X POST "http://127.0.0.1:8080/_raft/force-resync?target=127.0.0.1:8082"
```

Response:
```json
{
  "target": "127.0.0.1:8082",
  "success": true,
  "message": "Snapshot resync to 127.0.0.1:8082 completed successfully"
}
```

### Node Recovery Procedure

1. **Check cluster status**: Identify which nodes are down or desynced
2. **Restart failed node**: Node will auto-resync if data is recoverable
3. **Force resync if needed**: Use `/_raft/force-resync` endpoint
4. **Verify consistency**: Check that all nodes have same data count

## Follower Health States

| State | Icon | Meaning | Action |
|-------|------|---------|--------|
| healthy |  | In sync, replication working | None |
| lagging |  | Behind but catching up | Monitor |
| desynced |  | Too far behind, needs snapshot | Auto/manual resync |
| unknown |  | No replication activity yet | Wait or investigate |

## Configuration Options

### CLI Arguments

When using `LithairServer::from_cli()` or the built-in CLI:

| Argument | Description |
|----------|-------------|
| `--node-id <N>` | Node identifier (0 = leader) |
| `--peers <PORTS>` | Comma-separated peer ports (e.g., `8081,8082`) |
| `--port <PORT>` | HTTP port for this node |

Example:

```bash
# Start leader
cargo run -- --node-id 0 --port 8080 --peers 8081,8082

# Start follower
cargo run -- --node-id 1 --port 8081 --peers 8080,8082
```

### Replication Tuning

| Parameter | Default | Description |
|-----------|---------|-------------|
| Batch size | 100 | Max entries per replication batch |
| Flush interval | 5ms | WAL group commit interval |
| Resync threshold | 1000 | Entries behind before snapshot resync |
| Snapshot timeout | 60s | Max time for snapshot transfer |

## Limitations

| Feature | Status | Notes |
|---------|--------|-------|
| Leader election | Static | Node 0 is always leader |
| Quorum writes | Best-effort | Async replication |
| Split-brain protection | Not implemented | Assumes single leader |
| TLS inter-node | Not implemented | Use network isolation |
| Network partitions | Basic | Followers marked desynced |

## Future Roadmap

- [ ] Automatic leader election
- [ ] Strict quorum writes (majority acknowledgment)
- [ ] TLS for inter-node communication
- [ ] Dynamic cluster membership changes
- [ ] Read scaling (consistent reads from followers)

## See Also

- [Stress Testing Example](../../../examples/replication_stress_test/README.md)
- [Persistence Overview](../persistence/overview.md)
- [Event Sourcing](../persistence/event-store.md)
