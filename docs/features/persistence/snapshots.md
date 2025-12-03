# Snapshots

Snapshots provide fast boot times and reduce replay costs by capturing materialized state.

## Concepts

- **Periodic snapshots**: taken after N events or time-based intervals
- **Atomic write**: write to temp file then atomic rename
- **Format**: binary-encoded state with versioning header

## Strategies

- Size-aware: trigger when event log grows beyond threshold
- Time-aware: trigger every T minutes in active systems
- Hybrid: combine both for balanced cost

## Recovery

- Load latest snapshot → verify checksum → replay events since snapshot offset
- Integrity checks: handle partial/corrupt snapshots gracefully

## See also

- Event store: `./event-store.md`
- Deserializers: `./deserializers.md`
- Optimized storage: `./optimized-storage.md`
