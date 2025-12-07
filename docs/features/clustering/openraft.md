# Replication Implementation

Technical details of Lithair's replication system.

## Overview

Lithair implements a Raft-inspired replication protocol with:

- Leader-to-follower log replication
- Snapshot-based resync for desynced followers
- Incremental catchup for lagging followers
- Batched replication for efficiency

## Components

### Consensus Log

The `ConsensusLog` stores replicated operations:

```rust
pub struct ConsensusLog {
    entries: Vec<LogEntry>,
    commit_index: u64,
    last_applied: u64,
}

pub struct LogEntry {
    pub index: u64,
    pub term: u64,
    pub operation: ReplicatedOperation,
    pub timestamp: DateTime<Utc>,
}
```

### Replication Batcher

The `ReplicationBatcher` manages follower state and batched replication:

```rust
pub struct ReplicationBatcher {
    followers: HashMap<String, FollowerState>,
    batch_size: usize,
    flush_interval: Duration,
}

pub struct FollowerState {
    pub address: String,
    pub health: FollowerHealth,
    pub last_replicated_index: u64,
    pub consecutive_failures: u64,
    pub pending_entries: Vec<LogEntry>,
}
```

### Snapshot Manager

The `SnapshotManager` handles snapshot creation and transfer:

```rust
pub struct SnapshotManager {
    snapshot_dir: PathBuf,
    current_snapshot: Option<SnapshotMeta>,
}

pub struct SnapshotMeta {
    pub last_included_index: u64,
    pub term: u64,
    pub size_bytes: u64,
    pub checksum: u64,
}
```

## Replication Protocol

### Normal Operation

```text
Leader                          Follower
   │                               │
   │  /_raft/append                │
   │  {entries: [...], term: 1}    │
   ├──────────────────────────────►│
   │                               │ Append to log
   │  {success: true, index: 150}  │
   │◄──────────────────────────────┤
   │                               │
   │  Update follower state        │
```

### Snapshot Resync

```text
Leader                          Follower (desynced)
   │                               │
   │  /_raft/snapshot              │
   │  Headers: term, index, size   │
   │  Body: snapshot bytes         │
   ├──────────────────────────────►│
   │                               │ Install snapshot
   │  {success: true}              │ Reset state
   │◄──────────────────────────────┤
   │                               │
   │  Resume normal replication    │
```

## Internal Endpoints

### POST /_raft/append

Receives log entries from leader:

```json
{
  "term": 1,
  "leader_id": 0,
  "entries": [
    {
      "index": 151,
      "term": 1,
      "operation": {
        "model": "KVEntry",
        "action": "Create",
        "data": "..."
      }
    }
  ],
  "leader_commit": 150
}
```

### GET /_raft/snapshot

Returns current snapshot for resync:

Headers:
- `X-Snapshot-Term`: Term of snapshot
- `X-Snapshot-Index`: Last included index
- `X-Snapshot-Size`: Size in bytes
- `X-Snapshot-Checksum`: CRC32 checksum

Body: Binary snapshot data

### POST /_raft/snapshot

Installs snapshot received from leader:

Headers: Same as GET
Body: Binary snapshot data

## Background Tasks

### Incremental Catchup

Every 100ms, the leader checks for lagging followers and sends missing entries:

```rust
// Only send entries the follower is missing
let missing_entries = consensus_log
    .get_entries_from(follower.last_replicated_index + 1)
    .await;

if !missing_entries.is_empty() {
    send_entries_to_follower(peer, missing_entries).await;
}
```

### Desynced Follower Detection

Followers are marked desynced when:
- Consecutive failures > threshold (default: 10)
- Lag > threshold (default: 1000 entries)

### Snapshot Resync Task

Every 10 seconds, the leader checks for desynced followers and sends snapshots:

```rust
for peer in desynced_followers {
    if peer.eligible_for_resync() {
        send_snapshot_to_follower(peer).await;
    }
}
```

## Observability

### ResyncStats

Tracks snapshot resync operations:

```rust
pub struct ResyncStats {
    // Leader stats
    pub snapshots_created: AtomicU64,
    pub send_attempts: AtomicU64,
    pub send_successes: AtomicU64,
    pub send_failures: AtomicU64,

    // Follower stats
    pub snapshots_received: AtomicU64,
    pub snapshots_applied: AtomicU64,
}
```

### FollowerStats

Per-follower metrics:

```rust
pub struct FollowerStats {
    pub address: String,
    pub health: FollowerHealth,
    pub last_replicated_index: u64,
    pub last_latency_ms: u64,
    pub pending_count: usize,
    pub consecutive_failures: u64,
}
```

## Error Handling

### Replication Failures

When replication fails:
1. Increment `consecutive_failures` counter
2. Log warning with error details
3. If failures > threshold, mark as desynced
4. Desynced followers get snapshot resync

### Snapshot Failures

When snapshot send fails:
1. Log error with details
2. Increment `send_failures` counter
3. Retry on next resync cycle (10s)
4. Manual intervention available via `/_raft/force-resync`

## Performance Considerations

### Batching

- Entries are batched before sending (default: 100 entries max)
- Reduces network round-trips
- Improves throughput under load

### WAL Group Commit

- WAL writes are grouped (default: 5ms flush interval)
- Reduces fsync overhead
- Trades latency for throughput

### Incremental vs Full Catchup

- Incremental: Only missing entries sent (normal operation)
- Full snapshot: Complete state transfer (desynced followers)
- Automatic selection based on follower lag

## See Also

- [Clustering Overview](./overview.md)
- [Event Sourcing](../persistence/event-store.md)
- [Stress Test Example](../../../examples/replication_stress_test/README.md)
