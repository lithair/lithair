# Durable OpenRaft log foundation

Implemented by `lithair-core/src/cluster/durable_log/`, tracked in
[#251](https://github.com/lithair/lithair/issues/251) under
[#248](https://github.com/lithair/lithair/issues/248). This private component is
compiled with `cluster`. It is not connected to `with_raft_cluster`, SQL models
or sessions and supplies no new deployment guarantee.

## OpenRaft interface

The component provides the log side of OpenRaft 0.9 through its existing
`RaftStorage` → `Adaptor` → `RaftLogStorage` bridge. Do not enable `storage-v2`
yet: `openraft-memstore` 0.9.25 imports the root `RaftStorage` symbol which that
feature removes. Keeping the adapter preserves the deprecated consensus module
without changing its in-memory behavior. Direct v2 implementations can replace
the bridge when that dependency is retired.

`into_log_store()` returns only the adapter's log handle. Its underlying state
machine and snapshot operations explicitly fail as unsupported. Integration must
supply a **separate durable state machine**; the tests pair the log with upstream
MemStore solely to run the upstream storage suite. No application commands are
applied by opening the log. The persisted consensus commit index is distinct
from both the durable file boundary and any future applied index.

## Files and durability

The caller provisions an existing, dedicated directory and makes its parent
entries durable before using it. Bootstrap calls `create()` explicitly on an
empty directory. Restart calls `open()`, which requires the initialized files
and never falls back to creation. No legacy WAL is adopted. The files are:

- `LOCK`: exclusive OS file lock, held until the last handle closes.
- `journal`: `LTRLOG01`, a random 16-byte store identity, then framed operations.
- `durable.meta`: version, matching store identity and durable journal offset,
  itself framed and checksummed. This is a local durability boundary, not proof
  of Raft quorum commitment.

Frames contain a little-endian 32-bit JSON byte length, the JSON payload and a
CRC32 over the length and payload. Serialization and decoding enforce an 8 MiB
payload limit; append batches are limited to 4096 entries. Operations record
votes, appends, suffix truncation, prefix purge or the consensus commit watermark.
Retained entries form one consecutive range. Already purged entries supplied
again by OpenRaft recovery are ignored rather than resurrected.

One serialized operation performs:

1. Validate and encode the operation without mutating visible state.
2. Append its frame and `sync_all()` the journal.
3. Write new metadata to a temporary file in the same directory and sync it.
4. Atomically rename that file over `durable.meta`, then sync the directory.
5. Update the in-memory state and complete the operation.

The OpenRaft adapter reports successful `LogFlushed` completion only after step 5.
Votes and log writes share the same lock. Disk work runs in an owned blocking task;
caller cancellation after admission does not roll back a partially completed
operation. Any disk error makes every handle unusable until close/reopen, because
the operation's outcome may be indeterminate. Invalid input rejected before I/O
does not poison the store. Log reads use the recovered in-memory index, not disk.

## Recovery

Validate the header, metadata version, store identity, frame bounds, checksums
and semantic ordering of every operation through the durable offset. Damage or
missing bytes within this prefix is an error, never an empty-store fallback.
Discard bytes after the offset, sync the resulting journal and directory, then
allow access. This also handles complete but unacknowledged frames whose metadata
was never activated. A temporary metadata file left by a dead process is not an
authority and is ignored in an initialized store.

Failure before metadata replacement selects the previous boundary. Failure after
replacement can select the previous or new complete boundary after a power loss;
the caller receives no success until the directory sync completes. The new
boundary always refers to an already synced journal. A partially initialized
directory or mismatched files fail explicitly and require operator inspection.

## Limits and validation

This assumes a local filesystem/device that honors file sync, atomic same-directory
rename, directory sync and exclusive file locking. Network filesystems and actual
power-loss/device-cache behavior are not qualified by these tests. The directory
must be trusted and never replaced while open. A node must not run two writers.

Purge removes entries from the logical index and persists its watermark; this
first journal format retains historical operation bytes on disk. Physical
compaction and coordinated application snapshots are required before runtime
deployment. Recovery time and disk usage therefore grow with operation history.
There is no throughput claim or format migration API in this milestone.

`cidx run test` explicitly enables `cluster` for the dedicated storage target
and runs:

- The upstream OpenRaft storage suite paired with a test-only state machine.
- `lithair-core/tests/openraft_storage_test.rs`: reopen, conflict replacement,
  purge, vote/commit recovery, corruption, isolation, cancellation and injected
  I/O errors; child processes die at each write/metadata durability boundary.
- `cucumber-tests/tests/openraft_storage_bdd.rs`: the registered executable
  `features/persistence/openraft_storage.feature` scenarios.

The integration and BDD harnesses compile the private source directly to avoid
publishing an application-facing API solely for tests. Process-death tests exercise
recovery without destructors; they do not simulate a machine losing its page cache.
