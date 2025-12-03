# Lock-free Engine

Alternative state engine for specialized concurrency needs.

## Characteristics

- Lock-free data structures for high concurrency
- Tuned for specific read/write contention patterns
- Useful for experimental or niche workloads

## Operations

- Rebuild from events and snapshots similar to SCC2
- Careful tuning of atomic operations and memory ordering

## When to use

- High-contention scenarios where lock-free can outperform
- Specialized pipelines requiring fine-grained concurrency control

## See also

- Overview: `./overview.md`
- SCC2 engine: `./scc2.md`
- Persistence: `../persistence/overview.md`
