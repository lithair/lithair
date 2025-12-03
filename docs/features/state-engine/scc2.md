# SCC2 Engine

High-performance, memory-first state engine powering production and hybrid modes.

## Characteristics

- In-memory dataset optimized for hot paths
- Atomic hot reload (hybrid) via dataset swap
- Predictable latency for static asset serving and lookups

## Operations

- Build from snapshots + event logs
- Validate integrity on load
- Swap with zero downtime on reload

## When to use

- Production/static assets, memory-first frontends
- Latency-sensitive workloads where disk I/O must be minimized

## See also

- Overview: `./overview.md`
- Lock-free engine: `./lockfree.md`
- Persistence: `../persistence/overview.md`
