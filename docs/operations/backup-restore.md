# Backup, restore, and point-in-time recovery

This runbook covers protecting a Lithair deployment's data: what to back
up, how to restore it, and how far point-in-time recovery (PITR) reaches.

Lithair is event-sourced: every create/update/delete is appended to an
on-disk event log, and the in-memory state is reconstructed by replaying
that log on startup. That single fact drives everything below.

## Mental model

- **The event store is the source of truth.** RAM is a derived
  projection rebuilt from the log at boot
  (`replay_events` in `lithair-core/src/http/declarative.rs`).
- **Restore = replay.** There is no separate "import the database" step
  for the physical path: you put the data directory back and the engine
  replays it.
- **Two distinct strategies, do not conflate them:**

  | | Strategy A — logical export | Strategy B — physical event store |
  |---|------------------------------|-----------------------------------|
  | What | Current in-memory model data as JSON | The on-disk data directory (the log itself) |
  | History | No event history | Full history preserved |
  | PITR | No | Yes (bounded by snapshot cadence) |
  | Use for | Migration, seeding, inspection | Disaster recovery |
  | Restore | `POST /_admin/data/import` (or manual re-POST) | Drop dir back, restart, replay |

Strategy B is the real DR path. Strategy A is a convenience export, not
a backup of record.

## What to back up

Strategy B backs up the **data directory**. The single-file event store
layout (`lithair-core/src/engine/persistence.rs`):

| File | Purpose |
|------|---------|
| `events.raftlog` | Append-only event log (JSON lines, CRC32-prefixed) |
| `events.raftidx` | Index: aggregate id → byte offset in the log |
| `state.raftsnap` | Latest state snapshot (JSON), written by compaction |
| `meta.raftmeta` | Metadata (version, checksums) |
| `dedup.raftids` | Dedup id set (present only if dedup is enabled) |

Back up the **whole directory**, not individual files — the snapshot and
the log are a matched set (the log holds only events appended *after* the
snapshot once compaction has run; see PITR below).

A single deployment may register multiple models, each with its own data
subdirectory (e.g. `with_model::<Mail>("./data/mails", ...)`). Back up
the parent that contains all of them.

Default data-directory locations by deployment:

| Deployment | Data dir | Owner |
|------------|----------|-------|
| Local / bare binary | `./data` (or the path passed to `with_model`) | invoking user |
| systemd unit | `/var/lib/lithair` (via `StateDirectory=lithair`) | `lithair:lithair`, mode 0750 |
| Docker / compose | `/app/data` on the `lithair-data` volume | `lithair` (UID 1000) |
| Kubernetes | `/app/data` on the `lithair-data` PVC | UID 1000 |

See `docs/operations/deployment-docker.md` and
`examples/deployment/` for the artifacts these refer to.

## Strategy A — logical export

A live server exposes `POST /_admin/data/backup` (handled in
`lithair-core/src/app/mod.rs`). It walks every registered model, calls
`export_json()` on each, and returns one JSON document:

```bash
curl -fsS -X POST http://localhost:8080/_admin/data/backup \
  -o lithair_backup.json
```

The response (`Content-Disposition: attachment;
filename="lithair_backup.json"`) looks like:

```json
{
  "backup_type": "full",
  "timestamp": "2026-06-09T12:00:00Z",
  "model_count": 2,
  "models": [ { "...": "per-model export_json() output" } ]
}
```

`data_admin_enabled` must be on for this route to exist; it lives under
`/_admin/data/*` (see `handle_data_admin_request`). A per-model variant,
`GET /_admin/data/models/{name}/export`, exports a single model.

**This is the current model state only — no event history.** It is good
for migration, seeding a fresh environment, or eyeballing data. It is
**not** a disaster-recovery backup.

**Restore via the import endpoint.** `POST /_admin/data/import` (issue `#37`)
is the symmetric counterpart of `backup`: feed the backup document straight
back and it re-applies every record as an event.

```bash
# Capture the HTTP status — a clean import is 200, a partial one is 207
# (see "Partial success" below); do NOT rely on `curl --fail`, which treats
# both as success.
code=$(curl -sS -o /tmp/import.json -w '%{http_code}' \
  -X POST http://localhost:8080/_admin/data/import \
  -H 'Content-Type: application/json' \
  --data-binary @lithair_backup.json)
test "$code" = 200 || { echo "import not clean ($code):"; cat /tmp/import.json; }
```

It accepts the full backup shape (`{ "models": [ {model, data}, … ] }`),
a bare array of per-model exports, or a single `{model, data}` object, so
you can replay a whole backup or just one model's slice. The response
lists a per-model result and `total_imported`:

```json
{ "status": "imported", "total_imported": 3,
  "models": [ { "model": "Article", "status": "imported", "requested": 3, "imported": 3 } ],
  "note": "logical import: re-applies items as events, idempotent by id, does not restore event history" }
```

Properties to understand before relying on it:

- **Idempotent by `id`.** Re-importing overwrites the entity in place; it
  never creates a duplicate. Re-running an import is safe for the
  resulting state.
- **Appends events.** Each import writes one event per record (event type
  `Replicated`), so the event log grows on every run even though the
  final state is unchanged. Fine for a one-time migration; not a substitute
  for the physical store if you need a compact log.
- **No history.** It restores current state, not the original event
  timeline. For full fidelity including history, use Strategy B.
- **Partial success is surfaced.** Unknown models, a missing/non-array
  `data` field, or undeserializable records are reported per-model and the
  call returns `207 Multi-Status` instead of failing the whole import.
  Because `207` is a `2xx`, `curl --fail` does **not** flag it — automation
  must test the status code (`!= 200`) or inspect the per-model `status` in
  the response body to catch a partial import.
- **Cluster: route to the leader.** Import is a write; on a follower it
  is redirected to the leader like any other event-store mutation (unlike
  the node-local frontend reload).

Records can also be re-created one at a time through the model's normal
write endpoints (`POST`/`PUT` to the model's `base_path`) — the import
endpoint just does that in bulk in a single call.

## Strategy B — physical event-store backup

This copies the data directory to off-site storage. Two consistency
postures: cold (recommended) and hot.

### Cold / consistent backup

The simplest correct backup stops writes first, so the copied directory
is a quiescent, internally consistent set.

**Why stopping matters: the background flusher.** A running server batches
event writes and flushes them on a timer (`flush_events()` every
`LT_FLUSH_INTERVAL_MS`, default 100 ms). At any instant a copy of a
*running* store may miss the last unflushed batch — the in-memory event
count can be ahead of what is on disk. Stopping or draining the service
flushes the buffer on the way down, so the copied `events.raftlog` holds
every committed event. Do **not** copy a running store and assume the
last few writes are on disk; stop or drain first (or accept the torn-tail
loss documented under "Hot / live backup" below). This is the single
most common mistake in a cold backup of an active deployment.

Option 1 — stop the service:

```bash
sudo systemctl stop lithair          # systemd
# or
docker compose stop lithair          # compose
```

Option 2 — drain in-flight requests without a hard kill, using the
`serve_with_graceful_shutdown` hook added in PR #114
(`LithairServer::serve_with_graceful_shutdown`, `lithair-core/src/app/mod.rs`).
Wire a signal (e.g. SIGTERM) to the shutdown future so the accept loop
stops taking new connections and in-flight requests finish before the
process exits; then copy the now-idle directory.

With the directory at rest, sync it to S3-compatible storage under a
timestamped prefix so each backup is immutable and distinguishable:

```bash
TS=$(date -u +%Y%m%dT%H%M%SZ)

# AWS CLI / S3:
aws s3 sync /var/lib/lithair "s3://my-bucket/lithair/${TS}/"

# MinIO client / any S3-compatible endpoint:
mc mirror /var/lib/lithair "myminio/my-bucket/lithair/${TS}/"
```

For Docker, copy out of the named volume first (it is not on the host
filesystem directly):

```bash
docker run --rm -v lithair-data:/data -v "$PWD:/out" alpine \
  tar czf "/out/lithair-${TS}.tar.gz" -C /data .
aws s3 cp "lithair-${TS}.tar.gz" "s3://my-bucket/lithair/"
```

### Hot / live backup

Copying the directory while the server is running is **viable with a
caveat**, because of how the log is read back.

On restore the engine reads the log line by line
(`read_all_events`, `lithair-core/src/engine/persistence.rs`): empty
lines are skipped, each line's CRC32 is validated, and a line that fails
validation is logged and **rejected without aborting the replay** — the
loop continues. Above that, `replay_events` parses each surviving line
with `if let Ok(envelope) = serde_json::from_str(...)`, silently skipping
any line that does not deserialize. So a hot copy that captures a torn or
partially-written **final** record replays the intact prefix and drops
only that last incomplete event.

In **binary mode** (`LT_ENABLE_BINARY=true`, off by default) the log uses
length-prefixed framing (`[u64 length][payload]`) instead of JSON lines.
The reader (`read_all_event_bytes`, same file) stops cleanly at the first
frame whose length prefix or payload runs past the end of the file
(logged as "Incomplete length prefix / payload at end of file"), keeping
every fully-written frame before it. The net effect matches JSON mode: a
torn final frame is dropped, the intact prefix is preserved.

> **Binary mode was broken before the G7 drill (fixed).** Two latent bugs
> in binary mode were found while building the restore drill (issue #133)
> and fixed in the same change:
>
> 1. The event envelope serialized its `event_hash` / `previous_hash`
>    fields with `skip_serializing_if`, which is incompatible with bincode
>    (a non-self-describing format). The genesis event (whose
>    `previous_hash` is `None`) failed to decode, so it was silently
>    dropped on binary replay and `verify_chain()` reported zero events.
> 2. `EventStore::new()` chose the JSON line reader regardless of the
>    `LT_ENABLE_BINARY` env var when first opening a store, so reopening an
>    existing binary log (on restart or `lithair verify`) failed with
>    "stream did not contain valid UTF-8".
>
> If you took binary-mode backups before this fix, re-verify them with
> `lithair verify` on a build that includes it.

Caveats, in order of importance:

- A hot copy is **not** a guaranteed point-in-time consistent snapshot.
  Concurrent compaction can truncate `events.raftlog` while you copy,
  and the snapshot/log/index files are read at slightly different
  instants — you may capture a log that is ahead of or behind the
  snapshot you copied. Replay tolerates a torn *tail*; it does not
  reconcile a snapshot and log copied seconds apart.
- For a guaranteed-consistent set, prefer **cold** backup (stop or
  drain). Reserve hot copies for "better than nothing" continuous
  snapshots where losing the last few seconds of writes is acceptable.
- If you must take hot copies, disable auto-compaction during the window
  (or take them from a filesystem/volume snapshot that is itself atomic,
  e.g. an LVM/EBS snapshot), so the log is not truncated mid-copy.

## Restore

Restore from a Strategy B backup is: place the directory, fix ownership,
start the server, let replay run, verify.

### systemd

```bash
sudo systemctl stop lithair
sudo rm -rf /var/lib/lithair/*
# pull the timestamped backup back down
aws s3 sync s3://my-bucket/lithair/20260609T120000Z/ /var/lib/lithair/
# StateDirectory expects lithair:lithair ownership
sudo chown -R lithair:lithair /var/lib/lithair
sudo systemctl start lithair
journalctl -u lithair -f        # watch replay; look for "Replayed N events"
```

### Docker / compose

The container user is UID 1000, so restored files must be owned by 1000:

```bash
docker compose stop lithair
# pull the backup tarball back down from object storage
aws s3 cp s3://my-bucket/lithair/lithair-20260609T120000Z.tar.gz .
# restore into the named volume from a tarball
docker run --rm -v lithair-data:/data -v "$PWD:/in" alpine \
  sh -c 'rm -rf /data/* && tar xzf /in/lithair-20260609T120000Z.tar.gz -C /data && chown -R 1000:1000 /data'
docker compose start lithair
docker compose logs -f lithair
```

The named `lithair-data` volume is created with UID-1000 ownership, so
the `chown` matters most when restoring onto a bind mount or a freshly
created host directory (see the UID-1000 gotcha in
`docs/operations/deployment-docker.md`).

### Verify the restore

1. **Liveness** — `/health` returns exactly `{"status":"healthy"}`:

   ```bash
   curl -fsS http://localhost:8080/health
   ```

2. **Spot-check data** — query a model endpoint and confirm a known
   record is present and correct:

   ```bash
   curl -fsS http://localhost:8080/api/mails/<known-id>
   ```

3. **Chain integrity** — verify the event hash chain (see Verification
   below).

A restore round-trip (`backup → restore → boots → state matches`) is the
only proof a backup is good. Test it on a schedule; do not trust an
untested backup.

**This procedure is now proven by an automated drill.** Strategy B
(write → force-flush → copy dir → wipe → restore → replay → verify) is
exercised end-to-end in `lithair-core/tests/backup_restore_drill_test.rs`
for both JSON and binary (`LT_ENABLE_BINARY`) log modes. The drill
asserts field-level record identity (not just counts), a valid
`verify_chain()` on the restored store, and that a write after restore
continues the chain cleanly. It also proves the torn-tail claim below: a
truncated final record is dropped while the intact prefix survives, in
both modes. (Building that drill surfaced and fixed two bugs in binary
mode — see the note under "Hot / live backup".)

**See also:** restoring a pre-upgrade backup is the rollback step when a
version upgrade goes wrong — see [`upgrade.md`](upgrade.md) (Rollback),
which also explains the rollback window for event-sourced deployments.

## Point-in-time recovery (PITR)

Because state is rebuilt by replaying the log, recovering to an earlier
point means replaying only up to a chosen event — the event-sourced
model makes this natural. The event history needed for PITR lives in
`events.raftlog`.

**The reach of PITR is bounded by compaction.** Compaction
(`with_auto_compaction(threshold, interval)` in
`lithair-core/src/app/builder.rs`, executed by `compact()`) writes a
snapshot of full state to `state.raftsnap` and then **truncates**
`events.raftlog`. After a compaction, the events before the snapshot
point no longer exist on disk — you cannot replay to a moment older than
the most recent retained snapshot.

The default snapshot threshold is **10 000 events**
(`DEFAULT_SNAPSHOT_THRESHOLD` in `lithair-core/src/engine/snapshot.rs`);
the default check interval is 300 s. So with auto-compaction on, your
oldest recoverable point is roughly "the last snapshot," which advances
every time the log crosses the threshold.

Tuning the PITR/disk trade-off:

- **Fine-grained PITR** needs the log preserved. Either disable
  auto-compaction (accept unbounded `events.raftlog` growth — see
  `docs/operations/capacity-planning.md` for disk sizing), or **back up
  `events.raftlog` frequently** so you retain log segments off-site even
  after on-disk truncation.
- **Bounded disk** comes from compaction, at the cost of PITR depth.
- Raise the threshold to widen the PITR window at the cost of slower
  startup replay; lower it for faster replay and a shorter window.

There is no built-in "replay to event N / timestamp T" flag today
(issue #37 lists `--to-event` / `--to-timestamp` as the eventual CLI
goal). Until that lands, PITR is achieved by restoring a backup taken at
or before the target time and accepting that point as the recovery
point. Frequent timestamped Strategy B backups are therefore the
practical PITR mechanism.

## Verification and integrity

Each event carries a SHA256 hash linking it to the previous event.
`EventStore::verify_chain()`
(`lithair-core/src/engine/events.rs`) walks the whole log and returns a
`ChainVerificationResult`:

- each event's own hash is recomputed and compared (detects tampering);
- each event's `previous_hash` is checked against the prior event's hash
  (detects chain breaks / missing events);
- legacy events without hashes are counted and noted, not failed.

The result reports `total_events`, `verified_events`, `legacy_events`,
`invalid_hashes`, `broken_links`, and an overall `is_valid`.

### `lithair verify` — offline integrity check

Run `lithair verify <data-dir>` against a **restored** backup before you
start the server, so a bad restore is caught before it becomes the live
source of truth. It opens the event store offline (no running server),
walks the hash chain via `verify_chain()`, prints a summary, and sets a
scriptable exit code:

| Exit | Meaning |
|------|---------|
| `0`  | Chain valid (no tampered hashes, no broken links) |
| `1`  | Chain INVALID (tamper/corruption detected) |
| `2`  | Store could not be opened (bad path, unreadable files) |

```console
$ lithair verify /var/lib/lithair
Event store: /var/lib/lithair
  total events:    8
  verified:        8
  invalid hashes:  0
  broken links:    0
Result: OK — Chain fully verified: 8/8 events

$ lithair verify /var/lib/lithair-tampered
Event store: /var/lib/lithair-tampered
  total events:    8
  verified:        8
  invalid hashes:  1
  broken links:    0
  first bad hash:  index 0 (event_id evt-0)
Result: INVALID — Chain INVALID: 1 hash errors, 0 broken links out of 8 events
$ echo $?
1
```

It reads both JSON and binary (`LT_ENABLE_BINARY`) logs — pass the same
env var the server used so the log format is detected correctly. Restore
replay also rejects CRC32-corrupt log lines at boot
(`parse_and_validate_event`), so gross on-disk corruption surfaces in the
logs even without an explicit verify step; `verify` additionally catches
intentional tampering that fixes the CRC but cannot forge the SHA256
chain.

The logical import counterpart (`POST /_admin/data/import`) has landed —
see Strategy A above. A `--to-event` / `--to-timestamp` point-in-time
replay flag remains tracked in issue #37.

## Operational checklist

1. **Choose the strategy.** Disaster recovery → Strategy B (physical).
   Migration/seeding → Strategy A (logical export). Do not rely on
   Strategy A for DR.
2. **Backup cadence.** Schedule Strategy B to off-site S3-compatible
   storage under timestamped prefixes. Match the interval to your
   tolerance for data loss (RPO).
3. **Consistency posture.** Prefer cold (stop or
   `serve_with_graceful_shutdown` drain) for the backup of record; use
   hot copies only where a torn-tail / few-seconds loss is acceptable.
4. **Retention.** Keep enough timestamped backups to cover your PITR
   window — especially if auto-compaction is on, since on-disk history
   beyond the last snapshot is gone.
5. **Test restores.** Periodically run a full round-trip into a throwaway
   host/volume and confirm `/health`, a data spot-check, and
   `lithair verify <data-dir>` on the restored directory before starting
   the server. An untested backup is a guess. The automated drill in
   `lithair-core/tests/backup_restore_drill_test.rs` proves the procedure
   in CI; your scheduled restore test proves your *backups*.
6. **RPO/RTO.** RPO is set by backup frequency (and, for PITR depth, by
   snapshot cadence — the 10 000-event default bounds how far back you
   can go on a single restored directory). RTO is dominated by replay
   time at boot, which scales with log size; lower the snapshot threshold
   to shorten replay (see `docs/operations/capacity-planning.md`).

For `events.raftlog` durability semantics (fsync mode, crash safety) see
`lithair-core/DURABILITY.md`. For sizing the backup target, see
`docs/operations/capacity-planning.md`.
