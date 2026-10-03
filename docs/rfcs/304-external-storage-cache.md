# RFC 304: L1 cache for external storage models

Status: **implemented** in `lithair-turso` 0.3.1 and `lithair-postgres` 0.1.1 (v1: point reads). Decisions: [Decisions](#decisions).
Discussion and tracker: [#304](https://github.com/lithair/lithair/issues/304).
Follows [RFC 296](296-postgres-storage-tiers.md) (phase 4).

## Summary

Give models whose authority is an external store (`lithair-postgres`, `lithair-turso`) an opt-in **L1 copy tier**: recently used records kept in the node's memory and served from RAM, with **coherence across nodes** that sharing one PostgreSQL requires. This applies RFC 296 rules R3 (caches hold copies, never authority; writes go to the authority; current-state reads bypass copies) and R4 (durable data may be copied).

## What already exists

- **Native models already have L1/L2 tiering**: `#[retention(memory = N | "30d", max_mb = M)]` bounds the projected hot set, `#[pinned]` fields stay in a warm map, and evicted records are rebuilt from the event log on demand ([retention guide](../features/retention.md)). The log stays the authority, so this is already "copies under R3". #73 tracks the remaining gap (filtering and listing on evicted, non-pinned fields). **Not changed by this RFC.**
- **External models have no L1 at all.** Every read is a SQL round trip, and the derive currently refuses `#[retention]` on `#[storage(turso)]`.

## Goals and non-goals

Goals:
- Serve hot point reads (`GET /api/x/{id}`, `Store::get`) of external models from memory.
- Bound memory like native retention does (count, age, size), configurable per model and at deploy time.
- **Never** serve a copy older than the configured staleness bound, including after writes from another node.

Non-goals (v1):
- Caching **lists and filters**: they remain SQL queries (pagination and filtering stay in the database). Query-result caching needs query-level invalidation; this can be a later step.
- Moving authority between tiers (forbidden by R2).
- Write-back or deferred writes: writes always go synchronously to the authority.
- Changing native retention.

## Proposed design

### Declaration
Reuse the native vocabulary, so "how much of this model stays in RAM" is declared the same way whatever the authority:

```rust
#[derive(DeclarativeModel)]
#[storage(postgres, durable, collection = "invoices")]
#[retention(memory = 10_000, max_mb = 64)]   // L1 copies of an external model
struct Invoice { /* ... */ }
```

For external models, `memory`/`max_mb` bound the L1 copy set (LRU), and an optional `ttl = "5m"` caps the age of a copy. `#[pinned]` stays native-only. The `LT_<MODEL>_MEMORY_*` runtime overrides apply unchanged. Without `#[retention]`, nothing is cached (current behavior).

### What is cached
- **Documents by `(namespace, collection, id)`**, including "absent" (negative entries, short TTL).
- Permission hooks (`can_read`) still run on every read, so a cached document is never served to someone who can't read it.
- **Bypasses**, which always read the authority:
  - application commands (`View` reads must be serializable);
  - the read inside a write (update/patch/delete load the current row in their transaction);
  - an explicit `Store::get_fresh` for callers that need the current state.

### Coherence
- **Same node (read-your-writes).** After a committed write, the writing node invalidates its own entries, including every record touched by a batch or a command, before returning. This is the only case for Turso, which is single-process.
- **Other nodes (PostgreSQL).** Every write transaction also runs `pg_notify('lithair_cache', '<namespace>/<collection>/<id>')`. Notifications are transactional: delivered only after commit, never for a rollback. Each node holds one listening connection and evicts the named entries. Typical staleness: a few milliseconds after commit.
- **Missed notifications.** If the listening connection drops, the node **flushes its whole L1** and serves from the database until it listens again (an epoch change), because notifications sent while disconnected are lost. TTL is the final safety net for anything this misses.
- **Fallback when LISTEN is impossible** (e.g. some poolers in transaction mode): refuse L1 for that database at startup, unless a TTL-only mode is explicitly chosen (Q2).

### Memory and eviction
LRU bounded by count and approximate bytes, like native retention. Metrics report hits, misses, evictions and resident bytes per model; they never expose documents.

## Qualification
- Two "nodes" (two pools plus two listeners) on one PostgreSQL. A write on A invalidates B, measured within a bound.
- Batch and command writes invalidate every touched record. A rollback invalidates nothing and is not visible.
- A killed listener connection flushes L1 and serves from the database until reconnected.
- The TTL bound is respected even without notifications.
- Read-your-writes on the writing node, for Turso and PostgreSQL.
- Permission filtering on cached documents, and memory bounds with eviction order.
- The parity suite (RFC 296 phase 3) runs **with and without cache** and must produce identical transcripts.

## Decisions

| | Question | Decision |
|---|---|---|
| Q1 | Goal | **Both** latency and database load; v1 caches point reads, list caching may follow. |
| Q2 | Staleness contract | **Notification-based by default** (pg_notify, full flush on a lost listener, TTL safety net). **TTL-only only as an explicit opt-in.** |
| Q3 | Declaration | **`#[retention(...)]`**: one vocabulary for "what stays in RAM", plus `ttl` for external models. |
| Q4 | Turso | **Included in v1** (one process: local invalidation is complete). |
| Q5 | Negative caching | **Yes**, with a short TTL; notifications also invalidate "absent" entries. |

## Alternatives considered

- **Version check on each hit** (`SELECT version …` before serving the copy): always fresh, but still a round trip. It only saves transferring large documents.
- **An external cache (Redis, memcached)**: a new dependency and network hop, against the memory-first goal. L1 belongs in the node.
- **Native replication of the cache through Raft**: couples the copies to the native cluster's quorum, which is exactly what PostgreSQL models avoid (RFC 296 Q5).
