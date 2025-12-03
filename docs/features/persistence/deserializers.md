# Deserializers

Forward-compatible readers for event and snapshot formats.

## Concepts

- **Versioned payloads**: each event/snapshot carries a version tag
- **Adapters**: map older versions to current domain structs
- **Graceful fallback**: unknown fields ignored, defaults applied

## Strategies

- Tagged enums for event kinds; per-variant upcasters
- Visitor pattern for streaming decoders on large datasets
- Benchmarks to validate no-regression on hot paths

## Testing

- Corpus of historical payloads ensures compatibility over time
- Round-trip tests: serialize → deserialize → compare semantics

## See also

- Event store: `./event-store.md`
- Snapshots: `./snapshots.md`
- Optimized storage: `./optimized-storage.md`
