# OpenRaft durable snapshots and journal compaction

[#255](https://github.com/lithair/lithair/issues/255) extends the private
[log foundation](OPENRAFT_STORAGE.md). No model declaration or deployed HTTP
cluster behavior changes. Application integration remains tracked in
[RFC 248](../../rfcs/248-three-node-cluster.md).

## One durable manifest

`durable.meta` remains the checksummed local durability authority. Version 1
selects `journal` and its acknowledged offset. Version 2 additionally selects
an optional journal generation and an optional snapshot generation. It must
select at least one new generation. Ordinary writes preserve the selected format;
`open()` never upgrades it implicitly. Creating a snapshot or compacting a journal
explicitly writes version 2. Old binaries reject this format; downgrading after
activation is unsupported. Legacy cluster WALs are never adopted.

A compacted `journal-<uuid>` begins with `LTRLOG02`, the original 16-byte store
identity and its own 16-byte generation ID. The remaining frames use the existing
bounded JSON/CRC32 format. Compaction serializes the current vote, purge watermark,
retained entries and commit watermark in replay order, including uncommitted
entries. It neither advances commitment nor discards an additional log prefix.
Reads still use the in-memory index.

A `snapshot-<uuid>` contains `LTRSNP01`, store and generation identities, a framed
OpenRaft `SnapshotMeta`, a 64-bit payload length and opaque payload bytes. The
manifest records the exact file length and SHA-256 of the whole file. Metadata
is bounded by the 8 MiB frame limit; the payload is capped at 64 MiB. Opening
validates bounds before allocation, file length, identities, digest, metadata
ordering and coverage of any purged prefix. Missing or damaged active files
fail recovery; no fallback silently selects an older snapshot.

The snapshot metadata carries the applied log ID and membership. Its index cannot
move behind the previous snapshot or purged prefix. Application payloads must
encode and validate their own schemas, versions and consistency with that metadata.
The private byte store cannot validate native/Turso/session contents. It does not
apply snapshots to application state by itself.

## Publication, cleanup and interruption

Under the same owned storage lock as votes and appends:

1. Validate and encode bounded metadata; create a uniquely named generation.
2. Write the complete generation, sync its file, then sync its directory entry.
3. Write and sync a uniquely named `manifest-<uuid>` staging file.
4. Atomically rename it to `durable.meta`, then sync the directory.
5. Select the new generation in memory, delete obsolete owned generations, sync
   deletions, and return success.

Ordinary journal appends use the same manifest replacement procedure after
syncing their appended frame. Before manifest activation, the old journal and
snapshot remain authoritative. Afterwards, the new complete generation is
selected. Reopening stabilizes and validates that selection before cleaning
unselected `journal-`, `snapshot-` and `manifest-` UUID files. Legacy `journal`
is reclaimed only after a generation replaces it. Other filenames are preserved;
these names are reserved inside the dedicated, trusted directory. Temporary
files from an interrupted version-1 bootstrap still require operator inspection.

Admitted disk work completes independently of caller cancellation. Any I/O error
or panic invalidates all handles until close/reopen, including cleanup failure
after activation: failure can mean an indeterminate outcome. Garbage collection
never precedes durable manifest activation. A reader holding an older snapshot's
`Bytes` handle can finish even after its old disk generation is reclaimed.

Compaction needs temporary space for the replacement journal. It rewrites all
retained entries, so its cost scales with retained state, not total historical
operations. This is an explicit operation, not a production scheduling or
throughput policy. Repeated successful compactions leave one journal and one
active snapshot, plus the lock and manifest.

## Coordination with OpenRaft

When this store holds a snapshot, ordinary purge rejects an index beyond its
coverage. The application state-machine adapter must publish snapshots durably
before permitting log removal. OpenRaft 0.9.25 can submit a purge command while
its state-machine worker is still installing the corresponding snapshot.
`wait_for_snapshot(log_id)` waits for publication without retaining the storage
mutex, then the adapter purges and compacts. Log and state-machine `Adaptor`
instances need **separate locks**; sharing one lock would prevent installation
from completing while purge waits.

Notifications are registered before inspecting snapshot state, so publication
cannot be missed. Storage failure wakes waiters. A 30-second bound fails closed
if the state-machine worker never publishes; timeout does not authorize purge.
This internal bound and the memory-backed snapshot representation must be revisited
with real application backends and their operational limits.

The test-only `SnapshotMachine` validates serialized MemStore state against the
snapshot metadata, publishes it before successful install/build completion, and
restores it before OpenRaft replays the retained committed suffix. It is not an
application backend or a native/SQL transaction mechanism.

## Evidence and remaining scope

The cidx gates include checkpoint corruption/limits, format-1 reopen/upgrade,
retained uncommitted suffixes, repeated compaction and actual file-size reduction,
real rename failures, cancellation, waiting purge, injected I/O failures and
18 child-process deaths across nine boundaries for snapshot and journal changes.
The original upstream storage suite and 28 journal-operation process deaths remain
in the gate.

The three-process mTLS fixture now publishes snapshots and compacts purged logs.
A follower is killed, the majority acknowledges enough writes to snapshot and
purge its missing prefix, and the follower returns. An installation counter
proves it received a snapshot over the actual transport. All three processes then
cold-restart and recover the acknowledged commands. The registered checkpoint
and consensus Gherkin runners execute these contracts in `cidx run test`.

These tests establish process-crash recovery under the documented filesystem
sync/rename assumptions. They do not emulate host power loss or qualify disks,
hybrid application checkpoints, membership replacement or deployment on three
independent VMs. Persisted cluster identity and operator bootstrap remain the next
milestone-2 work before native/Turso/session integration.
