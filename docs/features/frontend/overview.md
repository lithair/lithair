# Frontend Overview

Lithair supports multiple frontend serving strategies with a focus on performance and DX.

- Memory-first serving using SCC2 for production performance
- Development mode for instant asset updates from disk
- Hybrid mode: production-level performance with API-triggered reload
- Multiple frontends (e.g., public + admin) via virtual hosts

## Key concepts

- Virtual hosts: map multiple frontend roots to paths
- Asset discovery: `public/`, `frontend/public/`, `static/`, `assets/`
- MIME detection for correct Content-Type headers
- Hot reload (hybrid): scan first, replace each changed asset in place, continue serving

## Persistent assets and reloads

`FrontendEngine` keeps serving from memory. Directory loads and reloads compare
content and serving metadata before writing: unchanged assets retain their IDs
and timestamps and produce no journal entry. Only additions, changes and removal
tombstones are persisted. `load_directory` reconciles the complete source tree,
including files removed while the process was stopped. Programmatic updates with
unchanged content/MIME and deletion of missing paths are also no-ops.

Frontend writers and reloads are serialized. A retained file is replaced in place
and never temporarily disappears, but publication of the complete directory is
not one transaction: concurrent readers can see files from both versions. Scan
failure leaves the live set untouched; persistence failure reports an error and
may leave a durable prefix of changes. Cached statistics reflect published assets.

Startup reads legacy JSON asset events and deletion tombstones one line at a time,
then compacts them into a checked checkpoint. Peak replay memory depends on the
live assets and largest event, rather than total historical log size. Malformed
JSON/checksums fail startup without discarding the journal.

During operation, compaction runs when the journal exceeds twice the live asset
count (minimum one event), or eight times live content bytes (minimum 1 MiB).
The crossing mutation/reload may temporarily exceed that threshold. The atomic
`state.raftsnap` checkpoint selects a `native-journal-*.raftlog` generation; old
history is reclaimed only after publication and sync. Snapshots preserve MIME,
asset metadata and deletions, including assets with no source directory.

Frontend persistence always uses synchronous JSON, independently of application
model `LT_ENABLE_BINARY` and `LT_OPT_PERSIST` settings. Content remains compatible
with the existing JSON byte-array encoding. This is local frontend persistence,
not cluster replication.

Existing installations are adopted automatically on first startup; no manual
deletion of `events.raftlog` is needed. For an offline backup, retain the **whole
frontend directory**, including its checkpoint and selected journal. Older
frontend binaries that ignore these checkpoints cannot safely reopen compacted
stores; restore a pre-upgrade backup if downgrading.

## Related guides

- Serving modes: `../../guides/serving-modes.md`
- Environment variables: `../../reference/env-vars.md`
