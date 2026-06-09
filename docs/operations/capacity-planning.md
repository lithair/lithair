# Capacity planning

This guide answers one question: **how much RAM, disk, and CPU does a
Lithair deployment need for `N` items at write rate `R`?**

The mental model is memory-first: the working set lives in RAM as a
lock-free concurrent map, and every mutation is appended to an on-disk
event log. Queries always hit RAM; disk is the durability and replay
surface. Sizing therefore splits cleanly:

- **RAM** scales with the *live working set* (item count × item size).
- **Disk** scales with *lifetime write volume* (every create/update/delete
  is an event), bounded by snapshots + compaction.
- **CPU** scales with request concurrency and event-store fsync.

Every number below is taken from the source; where a value depends on
your data, the formula is given so you can plug in your own measurements.

## RAM sizing

### Base model (no retention)

Each item registered via `with_model::<T>(...)` is held in memory in
full, for the lifetime of the server. The HTTP handler keeps a
`HashMap<String, T>` (`lithair-core/src/http/declarative.rs`) and the
SCC2 engine keeps a concurrent `state_map`
(`lithair-core/src/engine/scc2_engine.rs`). There is no eviction, no
LRU, and no on-demand reload — if you registered a model, you pay for
its full size in RAM.

The first-order formula (same one stated in the README):

```text
RAM(T) ≈ item_count × average_serialized_size(T)
```

**Worked example.** A model with 50 000 items averaging 4 KB each:

```text
50 000 × 4 KB ≈ 200 MB
```

That is the collection alone. On top of it, budget for:

- The in-memory index plus event-sourcing metadata — roughly ~200 B per
  item (see `docs/operations/deployment-docker.md`).
- Replay spikes, snapshot generation, and Rust allocator overhead.

Provision host RAM at **2–3 ×** the estimated collection total to cover
those, matching the README's operational checklist.

### Retention-bounded model (v0.12+)

With `#[retention(...)]` + `#[pinned]`, RAM is **no longer unbounded**.
The retention layer (`lithair-core/src/engine/retention.rs`,
`lithair-core/src/lifecycle/mod.rs`) keeps a bounded *hot* set fully in
memory and demotes the rest to a *warm map* that stores **only the
`#[pinned]` fields** as JSON. Non-pinned fields are dropped from RAM and
reloaded from the event store on demand.

Three eviction modes are available (`RetentionConfig`):

| Mode | Annotation | Field | Semantics |
|------|------------|-------|-----------|
| Count | `memory = N` | `memory_count` | Evict oldest when hot count `> N`. |
| Duration | `memory = "30d"` | `memory_duration_secs` | Evict items older than the window. |
| Budget | `max_mb = N` | `memory_budget_bytes` | Evict oldest until hot bytes `≤ budget`. |

The modes **combine**: `needs_eviction` returns true when *any*
configured limit is exceeded, and budget mode may drop several small old
items in a single insert to get back under cap (`track_insert` loops).

So the bounded RAM estimate becomes:

```text
RAM_bounded(T) ≈ (hot_item_count                      × avg_full_size(T))
               + ((total_item_count - hot_item_count) × avg_pinned_size(T))
               + index_overhead
```

The first term is the hot working set (fully projected items). The
second is the warm tail — only pinned fields survive eviction, and only
for items NOT in the hot set (a hot item is removed from the warm map
when it is promoted back, so the two never overlap). Listing and
filtering on pinned fields stays in RAM; reading a non-pinned field of
an evicted item replays its events from disk.

**Worked example.** 1 000 000 items, `#[retention(memory = 10000)]`,
pinned fields averaging ~200 B, full items averaging 4 KB:

```text
hot:   10 000   × 4 KB   ≈  40 MB
warm: 990 000   × 200 B  ≈ 198 MB   (total - hot = 1 000 000 - 10 000)
                          ─────────
                          ≈ 238 MB
```

Without retention the same dataset would need ~4 GB
(`1 000 000 × 4 KB`). Use retention for the cold tail; keep the hot
working set comfortably in RAM.

### Per-model environment overrides

Retention limits can be overridden per model at deploy time without
recompiling. The prefix is `LT_<MODEL>` where `<MODEL>` is the last
segment of the type name, uppercased and stripped to alphanumerics
(`Email` → `LT_EMAIL`); see `model_env_prefix` in
`lithair-core/src/http/declarative.rs`:

| Variable | Maps to | Format |
|----------|---------|--------|
| `LT_<MODEL>_MEMORY_RETENTION` | count mode | integer item count |
| `LT_<MODEL>_MEMORY_DURATION` | duration mode | seconds, or `30d` / `12h` / `45m` / `2w` / `1y` |
| `LT_<MODEL>_MEMORY_MAX_MB` | budget mode | integer megabytes |

Example: `LT_EMAIL_MEMORY_RETENTION=10000`.

## Disk sizing

### Append-only event log

The event store is append-only: every create, update, and delete is one
appended event. Disk therefore grows with **lifetime write volume**, not
with live item count — a record that is created and deleted still leaves
two events on disk until compaction.

File layout (`lithair-core/src/engine/persistence.rs`):

| File | Purpose |
|------|---------|
| `events.raftlog` | Append-only event log (JSON lines) |
| `events.raftidx` | Index: aggregate id → byte offset in the log |
| `state.raftsnap` | Latest state snapshot (JSON) |
| `meta.raftmeta` | Metadata (version, checksums) |

First-order disk estimate before compaction:

```text
raftlog_bytes ≈ total_mutations × average_event_size
```

### Bounding growth: snapshots + auto-compaction

A snapshot captures full state so the log before it can be discarded.
The default snapshot threshold is **10 000 events**
(`DEFAULT_SNAPSHOT_THRESHOLD` in `lithair-core/src/engine/snapshot.rs`).

Auto-compaction (opt-in, off by default) periodically snapshots and
truncates `.raftlog` once its event count crosses a threshold. Wire it
via the builder (`lithair-core/src/app/builder.rs`):

```rust
use std::time::Duration;
LithairServer::new()
    .with_model::<Mail>("./data/mails", "/api/mails")
    .with_auto_compaction(10_000, Duration::from_secs(300))
    .serve()
    .await?;
```

`with_auto_compaction(events_threshold, check_interval)` spawns one
background task per model that triggers snapshot + truncate when
`event_count > events_threshold`. The default check interval is 300 s
(`DEFAULT_AUTO_COMPACTION_CHECK_INTERVAL`); a threshold of `0` is
rejected. With compaction enabled, steady-state disk is roughly:

```text
disk ≈ snapshot_size + (events_threshold × average_event_size)
```

Without compaction, plan for the *lifetime* write volume.

**Worked example.** A model taking 5 000 mutations/day at ~1 KB per
event:

```text
no compaction:  5 000/day × 1 KB ≈ 5 MB/day → ~1.8 GB/year
with compaction (threshold 10 000):
  bounded at ≈ snapshot + 10 000 × 1 KB ≈ snapshot + ~10 MB
```

The bundled k8s PVC requests 5Gi as a modest starting point
(`examples/deployment/k8s/pvc.yaml`); size yours for lifetime write
volume if compaction is off.

For `.raftlog` durability semantics (fsync mode, crash safety) see
`lithair-core/DURABILITY.md`. For backing up and restoring the event
store (and how compaction bounds point-in-time recovery) see
[`docs/operations/backup-restore.md`](backup-restore.md).

## CPU

CPU is the hardest dimension to estimate statically — **measure it under
your workload**. The honest baseline:

- Lithair runs on the Tokio multi-threaded runtime; throughput scales
  with request concurrency and available cores.
- The dominant per-write cost is event-store fsync (controlled by
  `LT_FSYNC_ON_APPEND`); fsync-on-append trades latency for durability.
- An idle hello-world server uses negligible CPU (~15 MiB RSS; see
  `docs/operations/deployment-docker.md`).

**Do not impose a hard CPU limit in Kubernetes.** A CFS quota throttles
the Tokio runtime and causes latency spikes even when the node has spare
capacity — the bundled manifest sets CPU *requests* but no *limit* for
this reason (`examples/deployment/k8s/deployment.yaml`). Set a request
that reflects measured steady-state usage and let bursts use spare
cores.

## Tuning levers

| Symptom | Lever |
|---------|-------|
| RAM too high | Add `#[retention(...)]` (count / duration / budget) + `#[pinned]`; override per model with `LT_<MODEL>_MEMORY_RETENTION` / `_MEMORY_DURATION` / `_MEMORY_MAX_MB`. |
| Disk growing unbounded | Enable `with_auto_compaction(threshold, interval)` to bound `.raftlog`. |
| Slow startup (long replay) | Lower the snapshot threshold so replay starts from a more recent snapshot (`with_snapshot_threshold` / auto-compaction). |
| Write-latency spikes under load | Avoid k8s CPU limits (CFS throttling); tune `LT_FSYNC_ON_APPEND` / batching for the durability/throughput trade-off. |

See `docs/features/retention.md` for the full retention model and
`#[pinned]` semantics.

## Operational checklist

This complements (does not replace) the storage-and-memory checklist in
the README. Before deploying:

1. **Estimate base RAM** per model: `item_count × average_serialized_size`.
2. **Decide on retention.** If the dataset has a cold tail that won't fit
   in RAM, add `#[retention]` + `#[pinned]` and re-estimate with the
   bounded formula. Confirm the hot working set fits comfortably.
3. **Set host RAM with margin** — 2–3 × the estimated collection total
   to cover replay spikes, snapshot generation, and allocator overhead.
4. **Plan disk for lifetime write volume.** Enable
   `with_auto_compaction(threshold, interval)` if mutations are frequent;
   otherwise size the PVC/volume for cumulative event growth.
5. **Set CPU requests, not limits.** Avoid CFS throttling; measure
   steady-state CPU under representative load.
6. **Verify liveness wiring.** The `/health` endpoint returns
   `{"status":"healthy"}` — wire it into your probes.
</content>
</invoke>
