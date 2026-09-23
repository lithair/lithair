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
Synchronous mutation callbacks must not re-enter mutation/index operations on
the same engine. A read callback must not call mutation/index operations on that engine or acquire
a write lock on its own bucket.

This is a per-record mutation contract, not a multi-record transaction API.
Two separate reads are not a consistent snapshot of the whole database.
`write`, `insert_sync`, `remove_sync` and `clear_sync` are volatile helpers:
they maintain in-memory state/indexes but do not generate durable events or
perform event-level uniqueness validation. Direct `internal_map()` access is an
internal escape hatch that bypasses coordination altogether.

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

The generated native HTTP handler currently has its own map/persistence path.
Its durability settings are not changed by the SCC engine configuration.
Frontend asset writes also have their own journal-before-publication adapter.
Unifying these callers behind one native commit contract is separate work.

Retention can keep recent full records in memory and older pinned fields in a
warm map. Loading a full evicted record currently replays its history from the
journal; that path is not a memory-only lookup. Crash-atomic snapshots/compaction,
indexed cold reads, and coherent native/Turso backup and restore still need
separate qualification. This change does not establish power-loss guarantees
for the complete storage lifecycle.

The private OpenRaft foundation remains opt-in and separate. Three-node
Probatum/Compose tests exercise its test state machine; they do not yet qualify
replication of these native models, Turso documents or sessions.

## Validation

`lithair-core/tests/native_scc2_test.rs` covers simultaneous readers, concurrent
updates, unique and shared indexes, persistence failures, retention and legacy
journal replay. `cucumber-tests/tests/native_scc2_bdd.rs` runs executable model
scenarios from `features/core/native_scc2.feature` in `cidx run test`.

Run `cidx run ci` before review; its cluster phase retains the independent
Probatum and Docker Compose failure/recovery qualification. No throughput claim
is inferred from the concurrency tests. Compare workloads under the same
acknowledgement/durability mode before drawing performance conclusions.
