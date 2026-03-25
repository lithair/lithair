# Distribution & Clustering - Technical Stack

## Technologies Used

- **Consensus**: Raft protocol (in-house implementation)
- **State management**: SCC2 (State Concurrent Cache v2)
- **Network**: TCP with custom heartbeats
- **Persistence**: Event sourcing with WAL (Write-Ahead Log)
- **Serialization**: Bincode for Raft messages

## Critical Monitoring Points

- `raft.current_term`: Current Raft term
- `raft.state`: Follower/Candidate/Leader
- `scc2.partitions`: Network partition state
- `election.timeout_ms`: Configured election timeout
- `log.committed_index`: Committed entry index

## Debug Commands

```bash
# View cluster state
curl http://localhost:8080/admin/cluster/status

# Force an election
curl -X POST http://localhost:8080/admin/cluster/election

# Simulate a network partition
curl -X POST http://localhost:8080/admin/debug/partition/1
```

## Log Patterns to Watch

- `Election timeout`: Election failure
- `Lost leadership`: Leadership loss
- `Partition detected`: Partition detection
- `Log replication failed`: Replication failure
