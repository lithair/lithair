# Lithair Playground - Design Plan

> **Objective**: Demonstrate ALL Lithair capabilities in an interactive reference demo.
> This demo will serve as a technical showcase and a foundation for future development.

## Overview

The Lithair Playground is an interactive web application that allows you to:
- Visualize Raft replication in real time
- Run integrated benchmarks
- Control the cluster (kill/restart nodes, force election)
- Test security features (rate limiting, firewall)
- Explore data with live CRUD operations

---

## Architecture

```
advanced/playground/
├── Cargo.toml
├── src/
│   ├── main.rs                 # Entry point, cluster setup
│   ├── models.rs               # DeclarativeModel models
│   ├── playground_api.rs       # /_playground/* endpoints
│   ├── benchmark.rs            # Benchmark engine
│   ├── node_controller.rs      # Node control (kill/restart)
│   └── sse_events.rs           # Server-Sent Events for live updates
├── frontend/
│   ├── index.html              # Main SPA
│   ├── style.css               # Styles (dark theme)
│   └── app.js                  # Frontend logic
└── run_playground.sh           # Cluster launch script
```

---

## Lithair Features to Demonstrate

### 1. Raft Consensus
| Feature | Demo |
|---------|------|
| Leader Election | "Force Election" button, current leader visualization |
| Log Replication | Real-time counter (commit index, term) |
| Automatic Failover | Kill leader, observe automatic election |
| WAL (Write-Ahead Log) | Persistence stats, WAL size |
| Snapshots | Manual trigger, snapshot stats |

### 2. SCC2 Engine
| Feature | Demo |
|---------|------|
| Lock-free Operations | Concurrent benchmark (1000+ ops/sec) |
| Versioned Entries (OCC) | Version display in data explorer |
| Secondary Indexes | Index-based search in explorer |

### 3. Cluster Health
| Feature | Demo |
|---------|------|
| Node Status | Per-node health check (healthy/unhealthy/desynced) |
| Replication Lag | Real-time per-follower lag graph |
| Follower Sync | Synchronization progress bars |

### 4. Security
| Feature | Demo |
|---------|------|
| Rate Limiting | "Test Rate Limit" button, see rejection |
| Firewall (IP filter) | Live rule configuration |
| Anti-DDoS | Blocked/allowed request stats |
| Circuit Breaker | Circuit state visualization |

### 5. RBAC & Sessions
| Feature | Demo |
|---------|------|
| Role-based Access | Login with different roles |
| Permission Checker | Forbidden actions depending on role |
| Persistent Sessions | Sessions survive restarts |

### 6. DeclarativeModel
| Feature | Demo |
|---------|------|
| Auto CRUD | Create/Update/Delete forms |
| Validation | Real-time validation errors |
| Replication Tracking | "replicated" badge on each entity |

### 7. Performance Metrics
| Feature | Demo |
|---------|------|
| Ops/sec | Real-time graph |
| Latency | P50/P95/P99 histogram |
| Throughput | Read/write MB/s |

---

## API Endpoints

### Cluster Control
```
GET  /_playground/cluster/status          # Full cluster state
POST /_playground/cluster/kill/:node_id   # Kill a node
POST /_playground/cluster/restart/:node_id # Restart a node
POST /_playground/cluster/force-election  # Force leader election
GET  /_playground/cluster/wal-stats       # WAL stats
POST /_playground/cluster/snapshot        # Trigger snapshot
```

### Benchmark
```
POST /_playground/benchmark/start         # Start benchmark
GET  /_playground/benchmark/status        # Progress/results
POST /_playground/benchmark/stop          # Stop benchmark

Body start:
{
  "type": "write|read|mixed",
  "concurrency": 100,
  "duration_secs": 30,
  "payload_size": 1024
}
```

### Live Events (SSE)
```
GET  /_playground/events/replication      # Stream replication events
GET  /_playground/events/cluster          # Stream cluster state changes
GET  /_playground/events/benchmark        # Stream benchmark progress
```

### Security Testing
```
POST /_playground/security/test-rate-limit
POST /_playground/security/test-firewall
GET  /_playground/security/stats
```

### Data Operations (via DeclarativeModel)
```
GET    /api/items                         # List
POST   /api/items                         # Create
GET    /api/items/:id                     # Read
PUT    /api/items/:id                     # Update
DELETE /api/items/:id                     # Delete
```

---

## Frontend UI

### Main Layout
```
+------------------------------------------------------------------+
|  LITHAIR PLAYGROUND                          [Node: 0] [Role: Leader]
+------------------------------------------------------------------+
|                                                                    |
|  +-- CLUSTER STATUS ----------------------------------------+     |
|  |                                                           |     |
|  |  [Node 0 - LEADER]   [Node 1 - Follower]   [Node 2 - Follower] |
|  |   ● Healthy           ● Healthy             ● Healthy     |     |
|  |   Commit: 1234        Commit: 1234          Commit: 1233  |     |
|  |                                                           |     |
|  |  [Kill Node 0] [Kill Node 1] [Kill Node 2] [Force Election]    |
|  +-----------------------------------------------------------+     |
|                                                                    |
|  +-- REPLICATION MONITOR -----------+  +-- BENCHMARK ---------+   |
|  |                                   |  |                      |   |
|  |  Ops/sec: ████████████ 15,234    |  | Type: [Write v]      |   |
|  |  Latency: ████░░░░░░░░ 2.3ms     |  | Concurrency: [100]   |   |
|  |                                   |  | Duration: [30s]      |   |
|  |  Term: 5   Commit Index: 1234    |  |                      |   |
|  |  WAL Size: 12.4 MB               |  | [START BENCHMARK]    |   |
|  |                                   |  |                      |   |
|  |  [Replication Graph - Live]      |  | Results:             |   |
|  |  ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~  |  | - Ops: 456,789      |   |
|  |  ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~  |  | - Avg: 2.1ms        |   |
|  |  ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~  |  | - P99: 8.4ms        |   |
|  +-----------------------------------+  +----------------------+   |
|                                                                    |
|  +-- DATA EXPLORER ------------------+  +-- SECURITY ----------+   |
|  |                                   |  |                      |   |
|  |  Items (42 total)                 |  | Rate Limit: 100/s   |   |
|  |  +---------------------------+    |  | [Test Rate Limit]   |   |
|  |  | ID    | Name   | Status  |    |  |                      |   |
|  |  |-------|--------|---------|    |  | Firewall: ON        |   |
|  |  | abc.. | Item 1 | active  |    |  | Blocked IPs: 3      |   |
|  |  | def.. | Item 2 | draft   |    |  | [View Rules]        |   |
|  |  +---------------------------+    |  |                      |   |
|  |                                   |  | DDoS Protection: ON |   |
|  |  [+ New Item] [Refresh]          |  | Circuit: CLOSED     |   |
|  +-----------------------------------+  +----------------------+   |
|                                                                    |
+------------------------------------------------------------------+
```

### Key Interactions

1. **Kill Node** -- API call -- SSE event -- UI update -- Observe failover
2. **Force Election** -- API call -- New leader elected -- All UIs update
3. **Start Benchmark** -- Progress stream -- Live graph update -- Final results
4. **Create Item** -- Replication event stream -- "synced" badge appears
5. **Test Rate Limit** -- Rapid calls -- See rejection counter increase

---

## Data Models

```rust
/// Simple item for CRUD + replication demo
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
pub struct PlaygroundItem {
    #[db(primary_key, indexed)]
    #[http(expose)]
    #[persistence(replicate, track_history)]
    #[serde(default = "uuid::Uuid::new_v4")]
    pub id: Uuid,

    #[db(indexed)]
    #[http(expose, validate = "non_empty")]
    #[persistence(replicate)]
    pub name: String,

    #[http(expose)]
    #[persistence(replicate)]
    pub description: String,

    #[http(expose)]
    #[persistence(replicate)]
    #[serde(default)]
    pub status: ItemStatus,

    #[http(expose)]
    #[persistence(replicate)]
    pub metadata: serde_json::Value,

    #[lifecycle(immutable)]
    #[http(expose)]
    #[serde(default = "chrono::Utc::now")]
    pub created_at: DateTime<Utc>,

    #[http(expose)]
    #[persistence(replicate)]
    #[serde(default = "chrono::Utc::now")]
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ItemStatus {
    #[default]
    Draft,
    Active,
    Archived,
}
```

---

## Implementation Phases

### Phase 1: Base Structure
- [ ] Create project structure (Cargo.toml, src/, frontend/)
- [ ] PlaygroundItem model with DeclarativeModel
- [ ] Set up 3-node cluster with ClusterArgs
- [ ] Basic existing /_raft/health endpoints

### Phase 2: Playground API
- [ ] /_playground/cluster/status (aggregate health from all nodes)
- [ ] /_playground/cluster/kill/:node_id (send SIGTERM to process)
- [ ] /_playground/cluster/restart/:node_id (relaunch the process)
- [ ] /_playground/cluster/force-election

### Phase 3: Live Events (SSE)
- [ ] /_playground/events/replication (broadcast each replicated op)
- [ ] /_playground/events/cluster (cluster state changes)
- [ ] Hook into ReplicationBatcher to emit events

### Phase 4: Benchmark Engine
- [ ] POST /_playground/benchmark/start with config
- [ ] Async worker that runs the benchmark
- [ ] Metrics: ops/sec, latency histogram, throughput
- [ ] Stream progress via SSE

### Phase 5: Frontend UI
- [ ] HTML layout with sections (cluster, replication, benchmark, data, security)
- [ ] JavaScript for SSE listeners
- [ ] Real-time graphs (simple canvas or SVG)
- [ ] CRUD forms
- [ ] Cluster control buttons

### Phase 6: Security Demo
- [ ] Firewall configuration in the playground
- [ ] Rate limiting test endpoint
- [ ] Anti-DDoS stats display
- [ ] Circuit breaker visualization

### Phase 7: Polish & Documentation
- [ ] run_playground.sh script to launch 3 nodes
- [ ] README with instructions
- [ ] Screenshots/GIFs demo
- [ ] Integration into examples/README.md

---

## Launch Scripts

### run_playground.sh
```bash
#!/bin/bash
set -e

ACTION=${1:-start}
DATA_DIR="./data"

case $ACTION in
  start)
    echo "Starting Lithair Playground Cluster..."
    mkdir -p $DATA_DIR

    # Node 0 (initial leader)
    cargo run --release --bin playground_node -- \
      --node-id 0 --port 8080 --peers 8081,8082 &

    # Node 1
    cargo run --release --bin playground_node -- \
      --node-id 1 --port 8081 --peers 8080,8082 &

    # Node 2
    cargo run --release --bin playground_node -- \
      --node-id 2 --port 8082 --peers 8080,8081 &

    echo "Cluster started!"
    echo "  - Node 0: http://localhost:8080"
    echo "  - Node 1: http://localhost:8081"
    echo "  - Node 2: http://localhost:8082"
    echo ""
    echo "Open http://localhost:8080 for the Playground UI"
    ;;

  stop)
    echo "Stopping Lithair Playground..."
    pkill -f "playground_node" || true
    ;;

  clean)
    echo "Cleaning data..."
    rm -rf $DATA_DIR
    ;;

  *)
    echo "Usage: $0 {start|stop|clean}"
    ;;
esac
```

---

## Success Metrics

The demo will be considered complete when:

1. **Functional**
   - [ ] 3-node cluster starts in <5 seconds
   - [ ] Kill leader -- new leader in <3 seconds
   - [ ] CRUD operations replicated and visible on all nodes

2. **Performance**
   - [ ] Benchmark reaches >10,000 ops/sec in write
   - [ ] Latency P99 <10ms under normal conditions
   - [ ] UI remains responsive during benchmarks

3. **UX**
   - [ ] Intuitive interface, no documentation needed to understand it
   - [ ] Immediate visual feedback for all actions
   - [ ] Professional dark mode

4. **Reference**
   - [ ] Well-documented, reusable code
   - [ ] Patterns extracted for other projects
   - [ ] Regression tests

---

## Open Questions

1. **Node Controller**: How to kill/restart processes from the API?
   - Option A: Spawn nodes as child processes of the main process
   - Option B: Use signals (SIGTERM/SIGKILL) + external script
   - **Recommendation**: Option A for a self-contained demo

2. **Frontend**: JS framework or vanilla?
   - **Recommendation**: Vanilla JS for zero dependencies, like the existing admin UI

3. **Charts**: Which library?
   - **Recommendation**: Native canvas or uPlot (lightweight, performant)

---

## Effort Estimate

| Phase | Effort | Priority |
|-------|--------|----------|
| Phase 1: Structure | 2h | P0 |
| Phase 2: Playground API | 4h | P0 |
| Phase 3: Live Events | 3h | P0 |
| Phase 4: Benchmark | 4h | P1 |
| Phase 5: Frontend | 6h | P0 |
| Phase 6: Security Demo | 3h | P2 |
| Phase 7: Polish | 2h | P1 |
| **Total** | **~24h** | |

---

*This plan serves as the reference for the Lithair Playground implementation.*
