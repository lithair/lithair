# Event Store

Append-only event store per aggregate provides durability, auditability, and rebuildable state.

## Concepts

- **Aggregate logs**: one log file per aggregate type (e.g., `articles.raftlog`)
- **Append-only**: events are appended with monotonic offsets
- **Schema-forward**: events carry version info for safe evolution
- **Atomicity**: each append is atomic; corruption detection on startup

## Event format

- Header: version, timestamp, aggregate id, sequence
- Payload: binary or JSON (configurable)
- Checksum: integrity guard for partial writes

## API shape (high-level)

- `append(event)` → offset
- `stream(aggregate_id)` → iterator
- `scan(from_offset)` → iterator for recovery

## Usage patterns

- Write path: domain command → validated event → append
- Read path: replay events (+ snapshot) to build materialized state
- Audit: full timeline per aggregate preserved

## See also

- Snapshots: `./snapshots.md`
- Deserializers: `./deserializers.md`
- Optimized storage: `./optimized-storage.md`
- Architecture flow: `../../architecture/data-flow.md`
