# State Engine Overview

Lithair uses in-memory state engines optimized for read performance and low latency.

- SCC2 engine for memory-first serving and atomic swaps
- Lock-free engine for specialized workloads
- Event-sourced rebuild: state derived from events (plus snapshots)

## Key concepts

- In-memory indices and caches tuned for hot paths
- Deterministic rebuild from event logs + snapshots
- Atomic dataset swap for hybrid reloads

## Related docs

- SCC2: `./scc2.md`
- Lock-free engine: `./lockfree.md`
- Persistence: `../persistence/overview.md`
