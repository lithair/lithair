# Native model HTTP commits

For the separate opt-in OpenRaft runtime and its deployment limits, see
[native model consensus](native-cluster.md). The contract below covers the local
handler and legacy replicated-apply paths.

Model declarations and `with_model` / `with_declarative_model` / `with_model_ref`
stay unchanged. Native generated CRUD, admin edits and replicated apply helpers
now use one handler commit path. `with_handler` and `with_model_ref` also register
PATCH, alongside PUT.

## Acknowledgements and concurrency

Within one handler, a mutation holds a permit from validation against current
state through journal append, explicit flush, memory publication and notification.
Concurrent PATCH requests merge against the preceding committed record. Unique
checks and publication cannot race. PUT, PATCH and replicated updates reject a
primary key that differs from the addressed record. Journal formats and legacy
crate-qualified event replay remain compatible.

Successful mutations have flushed the journal before the response or notification.
The existing `LT_FSYNC_ON_APPEND` setting still applies: its default is **off**,
which flushes to the operating system but does not promise survival of an OS crash
or power loss. Set `LT_FSYNC_ON_APPEND=1` to request fsync for appended events.
Checkpoint durability is described in [Native snapshots and compaction](native-checkpoints.md).

Blocking append/flush runs on Tokio's blocking pool. Hot reads continue from
memory while the journal is busy; the memory write lock is only taken to publish.
Once a commit is admitted, an owned task finishes persistence and publication even
if the initiating request is cancelled. Cancellation is not rollback, and an
ambiguous response is not permission to retry as if nothing happened.

An append/flush failure returns HTTP 500 (including PATCH), leaves the candidate
unpublished, and stops subsequent handler mutations. Programmatic apply/admin
methods return an error. Failed commits emit no SSE/mutation notification. A cold
read only considers the journal prefix already published by the handler. Partial
I/O can still leave an ambiguous event on disk: recover/reopen and reconcile before
resuming writes. This is not an exactly-once retry protocol.

Bulk create and replicated batches commit items in order; a failure can leave a
committed prefix. There is no multi-record transaction or consistent snapshot
across separate reads. Each successful bulk item now emits the ordinary create
notification.

## Configuration and retention

The handler no longer starts a periodic flusher. `LT_EVENT_MAX_BATCH` does not
delay an acknowledged HTTP write, and `LT_FLUSH_INTERVAL_MS` does not control this
path. Native handlers reject `LT_OPT_PERSIST=1` at startup: that legacy optimized
writer only queues flush requests and cannot confirm successful persistence.
Its buffer/timer settings remain available to separate low-level users.

Compaction shares the mutation permit, so a live writer cannot commit between
snapshot capture and log truncation. Compaction refuses to truncate while evicted
warm records still depend on the journal. Records evicted after an earlier
compaction can be loaded from that snapshot. A synced, atomically published
checkpoint selects the journal generation used at restart; see
[the checkpoint protocol](native-checkpoints.md).

Retention's existing unique-field limitation remains: warm records contain pinned
fields only. Use `#[pinned]` with `#[db(unique)]` when uniqueness must be checked
against evicted records. Cold lookup still scans persisted history; hot reads do
not acquire that cost.

`reconcile_replace_all` remains a volatile helper; it joins mutation ordering but
does not write a journal or validate constraints. Direct mutation through
`get_event_store()` bypasses the handler contract; stop handler activity before
using that escape hatch. Use one handler per model directory.

This is a single-process contract. The legacy HTTP replication proposal and local
commit are not a distributed transaction. Native/Turso/session state machines
still need integration with the private OpenRaft foundation and separate failure
qualification. SCC and HTTP retain their own memory containers while following
the same journal-before-publication rule.

## Validation

`native_http_commit_test` covers HTTP failures/reopen, concurrent partial edits and
unique writes, cancelled callers, key identity, compaction and retention.
`native_http_commit_bdd` owns `features/core/native_http_commit.feature` and runs
through `cidx run test`. Run `cidx run ci` before review; its Probatum/Compose phase
continues to qualify the private consensus fixture, not application replication.
No throughput improvement is inferred from these correctness tests.
