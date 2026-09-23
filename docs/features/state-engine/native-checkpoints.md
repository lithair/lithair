# Native snapshots and compaction

Native HTTP model declarations stay unchanged. `DeclarativeHttpHandler::compact()`
and the opt-in auto-compaction task capture state while excluding mutations, publish
a durable checkpoint, then reclaim the covered journal. SCC's `snapshot()` drains
its ordered writer under the mutation gate before capturing state. `truncate_log()`
refuses reclamation if events have arrived since that snapshot; capture again and
retry. It also refuses truncation without a checkpoint.

SCC recovery restores the snapshot before applying only the uncovered suffix.
Repeating recovery replaces the recovered state, rather than applying increments a
second time. Indexes and retention metadata are rebuilt. Cold reads start from the
checkpoint too. Both engines refuse to snapshot/compact an incomplete hot-only map
while warm records still depend on the journal.

## Files and interrupted maintenance

`state.raftsnap` is now a versioned, SHA-256 checked JSON envelope. It contains the
serialized state, selected journal filename, covered byte/event boundary, checksum
of that prefix and the last covered event hash. It acts as the checkpoint manifest:

1. Flush/drain writers and sync the covered journal and its directory.
2. Write and sync a temporary snapshot, rename it over `state.raftsnap`, then sync
   the directory. The old journal remains available throughout this transition.
3. For compaction, create and sync an empty `native-journal-<uuid>.raftlog` and sync
   its directory. Atomically publish the same snapshot selecting this new journal.
4. After that selection is durable, close the old writers and reclaim obsolete
   journals and temporary snapshots. The index is a derived cache and is reset.

Reopening follows the selected journal and skips the recorded prefix. Interruption
before or after either publication therefore preserves the snapshot plus suffix
without applying the covered events twice. Publication I/O errors stop further
writes through that storage instance; reopen to resolve an uncertain outcome.
Temporary/orphan generations are ignored when a valid checkpoint selects another
journal and are cleaned during a subsequent successful compaction.

Missing/corrupt selected files, invalid checkpoint checksums and unreadable journal
prefixes fail startup. Checkpointed journals also reject corrupt JSON/CRC records or
incomplete binary frames instead of silently skipping them. Recovery is conservative:
a torn, unacknowledged tail may require operator repair from a verified backup.
An ordinary legacy journal keeps its existing read behavior until checkpointed.

The protocol assumes a local filesystem supporting atomic rename and file/directory
fsync, with one owner per model directory. It does not qualify network filesystems
or multiple processes sharing that directory. Hot request reads remain in memory;
checkpointing and cold reconstruction may scan persisted state.

## Durability and compatibility

Checkpoint publication always uses fsync, including when ordinary HTTP appends use
the default `LT_FSYNC_ON_APPEND=0`. That default still does **not** promise survival
of acknowledged writes since the checkpoint after an OS crash/power loss. Enable
`LT_FSYNC_ON_APPEND=1` for that append contract. Process-interruption tests exercise
the ordering protocol, not physical power-loss behavior of a storage device.

Existing JSON/binary journals remain readable; retain the same binary-mode setting.
Legacy raw JSON snapshots are accepted and adopted by the next checkpoint. They have
no recorded replay boundary: their journal is treated as the subsequent suffix,
matching the old HTTP snapshot-then-truncate convention. An old snapshot taken
without truncating its covered events is ambiguous and needs offline verification.
After writing the new checkpoint format, do not reopen with an older Lithair binary.
Keep a pre-upgrade backup if rollback is required.

Low-level SCC replay/cold reconstruction and `Engine::new` now require state types
to implement `DeserializeOwned`, as well as `Serialize`, to restore their snapshots.
Declarative models already satisfy this. Direct `EventStore::save_snapshot` callers
must hold their application's mutation boundary and flush before capturing state.
Use `recovery_state()` for snapshot-plus-suffix replay; `get_all_events()` continues
to expose the physically retained journal. `LT_OPT_PERSIST=1` cannot checkpoint:
that separate legacy writer has no acknowledged flush barrier. Multi-file aggregate
snapshots remain on their existing separate path.

Back up the **entire stopped model directory**, including `state.raftsnap`, its selected
journal generation, metadata and deduplication files. Copying only `events.raftlog`
is no longer sufficient. Copying a live directory file by file is not a consistent
backup. Restore a copy and exercise replay before replacing production data.

This is single-process native storage. Cluster model/Turso/session replication still
requires integration with the consensus state machines and separate qualification.
