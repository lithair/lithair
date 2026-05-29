# Memory/disk retention tiering

Since v0.12.0, models can declare how many items stay fully projected in RAM
and which fields survive eviction. The event log remains the source of truth:
evicted items are reloaded on demand by replaying their events. This lets
Lithair stay memory-first without requiring the entire historical dataset
to fit in RAM.

## Declaring retention

Three dimensions can be combined on a `#[retention(...)]` attribute:

- `memory = N` — keep at most `N` items fully in memory (count-based)
- `memory = "30d"` — evict items older than the cutoff (`s`/`m`/`h`/`d`/`w`/`y` suffixes)
- `max_mb = 512` — evict oldest until total hot-storage size ≤ budget

Fields marked `#[pinned]` stay in a warm map after eviction. Pinned fields
remain queryable (listing and filtering against them stays instant); non-
pinned fields are reloaded from the event store when an evicted item is
accessed by id.

```rust
use lithair_core::DeclarativeModel;
use serde::{Serialize, Deserialize};

#[derive(DeclarativeModel, Serialize, Deserialize, Clone, Debug)]
#[retention(memory = 1000)]      // or memory = "30d", or max_mb = 512
pub struct Email {
    #[pinned] pub from: String,   // always in RAM, even after eviction
    #[pinned] pub subject: String,
    pub body: String,             // evicted with the rest; reloaded on demand
}
```

## Runtime overrides

Each dimension can be tuned at deploy time without recompiling, via
environment variables:

- `LT_<MODEL>_MEMORY_RETENTION=<count>`
- `LT_<MODEL>_MEMORY_DURATION=<duration>` (e.g. `30d`)
- `LT_<MODEL>_MEMORY_MAX_MB=<megabytes>`

The model name is the last segment of `std::any::type_name::<T>()`, sanitized
to alphanumeric + underscore and uppercased. So the `Email` model above
becomes `LT_EMAIL_MEMORY_RETENTION=2000`.

Runtime overrides win over the compile-time annotation, so the same binary
can be deployed at different memory pressures across hosts.

## Eviction model

When the configured cap is exceeded, the oldest item moves from the hot
collection to the warm map. The warm entry keeps only the pinned fields plus
the aggregate id needed to replay; the rest of the payload is dropped from
RAM. On a subsequent read by id, Lithair reverse-scans the event store for
the matching `aggregate_id` and rehydrates the full item. Listing and
filtering against pinned fields stays instant because the warm map is always
in memory.

The event store is never trimmed by retention — it remains the durability
and replay surface. Retention controls only the in-RAM projection.

## See also

- CHANGELOG v0.12.0 entry — full semantics, edge cases, and the runtime override naming scheme.
- PRs [#96](https://github.com/lithair/lithair/pull/96), [#99](https://github.com/lithair/lithair/pull/99), [#100](https://github.com/lithair/lithair/pull/100), [#101](https://github.com/lithair/lithair/pull/101) — the implementation arc, from annotation parsing through eviction correctness fixes.
