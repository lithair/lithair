# Optimized Storage

Storage optimizations for high throughput and low latency.

## Techniques

- **Binary payloads**: compact encoding for hot paths
- **Zero-copy reads**: mmap or buffered readers where applicable
- **Batch appends**: amortize fsync costs with grouped writes
- **Compression**: selective compression for cold segments
- **Indexing**: lightweight offsets index for fast scans

## Considerations

- Balance CPU vs I/O: avoid over-compressing hot data
- Keep metadata small and cache-friendly
- Measure: use benchmarks to validate choices per workload

## Benchmarks

- Throughput and p99 latency under mixed append/scan workloads
- Footprint comparisons: JSON vs binary vs hybrid

## See also

- Event store: `./event-store.md`
- Snapshots: `./snapshots.md`
- Deserializers: `./deserializers.md`
