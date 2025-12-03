# OpenRaft Integration

How Lithair integrates with the OpenRaft library for consensus.

## Key points

- Log replication integrated with event store
- Snapshot handoff between storage and cluster nodes
- Backpressure and flow control considerations

## Operational patterns

- Single-node dev mode vs multi-node clusters
- Rolling upgrades and leadership transfer
- Failure handling and reconfiguration

## See also

- Clustering overview: `./overview.md`
- Persistence: `../persistence/overview.md`
