# Persistence Overview

Event-sourced persistence and storage model in Lithair.

- Event store per aggregate (e.g., `products.raftlog`)
- Snapshots for fast boot/recovery
- Deserializers and forward-compatible schema
- Optimized storage formats and binary payloads (where applicable)

## Key concepts

- Write model: append-only events, audit trail
- Read model: rebuild state from events (+ snapshots)
- Recovery: startup replay and integrity checks

## Deep dives

- [Event Store](./event-store.md)
- [Snapshots](./snapshots.md)
- [Deserializers](./deserializers.md)
- [Optimized Storage](./optimized-storage.md)
- [Dual-Mode Serialization](./serialization.md) - JSON (simd-json) + Binary (rkyv)
- [Event Chain & Integrity](./event-chain.md) - CRC32 validation and event linking

## See also

- Storage module: `../../modules/storage/README.md`
- Architecture data flow: `../../architecture/data-flow.md`
- Env vars: `../../reference/env-vars.md`
