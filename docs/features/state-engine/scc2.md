# Native SCC engine: memory, concurrency and persistence

`Scc2Engine` is Lithair's internal concurrent state engine, currently backed by
`scc` 3.x. The historical type name does not identify the dependency version.
Applications keep their model declarations and do not need SQL for native data.

## Access and mutation contract

Reads by key use SCC's shared read access. Contention is not absence: a busy
bucket must not turn an existing item into `None`. A read of a resident record
uses memory only. SCC provides synchronization within one process; it is not a
multi-node consensus protocol, and its `HashMap` is not a universally lock-free
transaction engine.

All engine-managed mutations share one gate per engine. An event prepares a
candidate state once; the engine validates unique fields, queues its journal
entry, optionally awaits persistence, then publishes the candidate and maintains
secondary indexes. This keeps concurrent event order consistent between live
state and replay. Reads of unrelated records can continue during journal writes.
Synchronous mutation callbacks must not re-enter mutation/index operations or
cold `read_or_load` recovery on the same engine. A read callback must not call
those operations or acquire a write lock on its own bucket.

This is a per-record mutation contract, not a multi-record transaction API.
Two separate reads are not a consistent snapshot of the whole database.
`write`, `insert_sync`, `remove_sync` and `clear_sync` are volatile helpers:
they maintain in-memory state/indexes but do not generate durable events or
perform event-level uniqueness validation. Direct `internal_map()` access is an
internal escape hatch that bypasses coordination altogether.

## Updating evicted records

Call `replay_events::<ApplicationEvent>()` when opening a low-level `Scc2Engine`,
including a new empty store, before accepting mutations. `Engine<A>` already does
this with `A::Event`; model declarations are unchanged. Successful replay also
selects that event decoder for later warm-record mutations. It must decode the
complete history of the model (use the application's event enum when there are
multiple event variants).

An evicted record is reconstructed from its checkpoint plus journal suffix under
the same mutation gate as hot writes. Recovery first drains accepted queued
writes, checks every matching event and verifies the recovered version against
the warm metadata. Only then does the engine apply the new event, validate unique
fields and publish it. Indexes and version numbers continue across evictions.
Hot reads still use memory only; recovering a warm record can perform disk I/O
and scan the journal.

If recovery is unavailable or incomplete, the mutation returns an error without
publishing a candidate. This includes missing startup replay, unreadable history,
an incompatible decoder and previously uncheckpointed volatile changes.
`update_entry_volatile` uses the same preparation and returns `None` without
calling its callback on failure; `write` reports the error. An explicit
`insert_sync` supplies a complete replacement and does not need the old value.

Volatile changes remain volatile. After such a record is evicted, neither a
mutation nor `read_or_load` silently reconstructs an older durable value. A
successful full snapshot can make resident volatile state recoverable; ordinary
journal flushes cannot. `read_or_load` returns `None` on recovery failure because
its existing return type is `Option`.

## What success means

| Path | Successful return means |
| --- | --- |
| `apply_event`, persistence disabled | Candidate published in memory |
| `apply_event`, queued persistence | Journal entry queued and candidate published; persistence can still fail |
| `apply_event`, `force_immediate_persistence = true` | Journal flush acknowledged before candidate publication |
| `flush()` | All entries queued before this barrier flushed successfully |

The journal worker owns a dedicated thread so blocking file I/O and persistence
acknowledgements can progress independently of Tokio's executor. Event preparation
and mutation waits run through `spawn_blocking`; no SCC bucket guard crosses an
await. The queue is currently unbounded, so sustained ingestion still needs
workload qualification and admission limits at the application boundary.

The engine's writer uses `MaxDurability`: completed batches request fsync.
The separate `AsyncWriter` utility also exposes `Performance`, which omits
fsync. Its timer is a batching mechanism, not a bound on possible data loss.
A failed append/flush poisons that writer: future submissions and flushes return
an error. This prevents a subsequent empty flush from concealing an earlier
failure. Recover/reopen the store before resuming writes; do not blindly retry a
partly written batch.

An error or a cancelled caller is not proof that an event is absent from disk.
Once admitted, an event may finish even if its caller disappears. In immediate
mode, a storage failure leaves the live candidate unpublished, but recovery must
resolve any partial/ambiguous disk outcome. There is no exactly-once retry claim.

New SCC event entries use the existing `EventEnvelope` format with the actual
aggregate key and the event's serde payload. Legacy raw events and envelopes
remain readable; historical raw events without an aggregate ID retain their
`global` replay behavior. `Event::apply` must be deterministic for replay.

## Boundaries and next work

The generated native HTTP handler retains its own map but now uses a centralized
[journal-before-publication commit path](native-http.md), including admin and
replicated apply helpers. Its durability settings remain independent from SCC.
Frontend asset writes also have their own journal-before-publication adapter.

Retention can keep recent full records in memory and older pinned fields in a
warm map. Loading a full evicted record currently replays its history from the
journal; that path is not a memory-only lookup. Snapshots and compaction follow
the [native checkpoint protocol](native-checkpoints.md). Indexed cold reads and
coherent native/Turso backup and restore still need separate qualification. This change does not establish power-loss guarantees
for the complete storage lifecycle.

The private OpenRaft foundation remains opt-in and separate. Three-node
Probatum/Compose tests exercise its test state machine; they do not yet qualify
replication of these native models, Turso documents or sessions.

## Validation

`lithair-core/tests/native_scc2_test.rs` covers simultaneous readers, concurrent
updates, unique and shared indexes, persistence failures, retention and legacy
journal replay. Warm mutation regressions cover both acknowledgement modes, JSON
and binary journals, checkpoint suffixes, concurrent evictions, volatile changes
and failed recovery without partial publication. `cucumber-tests/tests/native_scc2_bdd.rs` runs executable model
scenarios from `features/core/native_scc2.feature` in `cidx run test`.

Run `cidx run ci` before review; its cluster phase retains the independent
Probatum and Docker Compose failure/recovery qualification. No throughput claim
is inferred from the concurrency tests. Compare workloads under the same
acknowledgement/durability mode before drawing performance conclusions.
