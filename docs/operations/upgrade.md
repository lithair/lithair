# Upgrading Lithair across versions

This playbook covers moving a deployment from one Lithair version to
another while an on-disk event store is present: what compatibility
surfaces to check, how additive vs. breaking schema changes behave, the
step-by-step procedure, and how rollback works.

## What "upgrade" means here

Lithair is event-sourced **and** compiled. There is no standalone server
binary you swap independently of your code: your application links
`lithair-core` (and the `lithair-macros` proc macros) and *is* the
server. Upgrading is two operations done together:

1. **Rebuild your application** against a new `lithair-core` crate.
2. **Restart it against the existing on-disk event store**, which it
   replays at boot to reconstruct in-memory state
   (`replay_events`, `lithair-core/src/http/declarative.rs`).

That gives two compatibility surfaces to clear:

- **(a) Rust API** — does your code still compile against the new crate?
  Caught at `cargo build` time.
- **(b) On-disk event-store format** — can the new binary replay the old
  log? This depends on your *model* changes (field additions, removals,
  type changes), not just the crate version. See
  [Additive](#additive-changes-safe-path) vs.
  [Breaking](#breaking-changes-managed-path) below.

## Downtime: what is and isn't zero-restart

Lithair is explicit about this — it is **not** a marketing "zero-downtime"
claim, because the single-binary model cannot truthfully make one:

- **Content updates** (event-sourced writes through the API) — **no restart.**
- **Frontend updates** (`POST /_admin/frontend/*/reload`) — **no restart**, hot
  in memory.
- **A binary / version change** (this playbook) — a **brief graceful restart**:
  the old process drains in-flight connections (`serve_with_graceful_shutdown`),
  then the new one boots and replays the event log to rebuild state. Boot time
  scales with the number of events **since** the last snapshot — typically
  seconds; size it with [capacity-planning](capacity-planning.md) and a snapshot
  cadence.

True zero-downtime *across a binary change* is an **operational pattern, not a
framework guarantee**: run two instances behind a proxy/load balancer and cut
over (blue-green / rolling), each instance pointed at its own replayed state.
That sits on top of Lithair; it is intentionally out of the v1.0 contract for
the single-binary deployment. (A built-in hand-off may come post-1.0; it is not
promised.)

## Before you upgrade

- **Read the CHANGELOG for the target version.** Note breaking changes
  and any `lithair-macros` bump. A `lithair-macros` bump forces
  recompilation of every model deriving `DeclarativeModel` — v0.12.0
  did exactly this (CHANGELOG `[0.12.0]` → *Changed*: "`lithair-macros`
  bumped to 0.12 (helper-attribute declaration fix requires
  recompilation of every model deriving `DeclarativeModel`)"). Treat a
  Lithair minor bump as *rebuild your app*, not *swap a binary*.
- **Take a backup FIRST.** A verified-restorable physical backup of the
  event store is your rollback plan. Follow
  [`backup-restore.md`](backup-restore.md) (Strategy B — physical
  event-store backup, cold/consistent posture). Do not skip this: an
  untested backup is a guess.
- **Mind the SemVer status.** This project adheres to Semantic
  Versioning (CHANGELOG header) and is **pre-1.0**. Under SemVer, `0.x`
  minor bumps (e.g. `0.11 → 0.12`) may carry breaking changes — the
  "minor = compatible" guarantee does not apply until 1.0. The 1.0
  roadmap is tracked in issue #108 (Pillar: Adoption infrastructure), so
  verify each minor against its CHANGELOG.

## Additive changes (safe path)

Adding a field **with a default** is the safe upgrade. The mechanism is
the `#[lithair_model]` **attribute** macro — not the `DeclarativeModel`
derive. A derive macro cannot modify the struct it is applied to, so it
cannot inject `#[serde(default)]`; the attribute macro rewrites the
item and can. When a field carries `#[db(default = X)]` and has no
explicit `#[serde(default)]`, `#[lithair_model]` generates a default
function and attaches `#[serde(default = "...")]` to the field
(`lithair-macros/src/lithair_model.rs`, the `db_default_value` →
generated `__lithair_default_<field>` → `#[serde(default = ...)]`
block). At deserialization, events written by the **old** version that
lack the new field deserialize cleanly — the missing field is filled
with the default.

> **You must use `#[lithair_model]` for this to work.** With a bare
> `#[derive(DeclarativeModel)]`, `#[db(default = X)]` does NOT produce a
> `#[serde(default)]`, so old events lacking the new field fail to
> deserialize and are silently skipped on replay (`replay_events` does
> `if let Ok(...)`), dropping those records. The attribute macro is what
> makes an additive change safe.

Worked example. Add a `phone` field with a default to an existing
`User` model:

```rust
#[lithair_model]                 // attribute macro — injects serde(default)
#[derive(DeclarativeModel)]
pub struct User {
    #[db(primary_key)]
    pub id: Uuid,
    pub name: String,
    #[db(default = "")]      // #[lithair_model] emits #[serde(default = "...")]
    pub phone: String,       // <-- new field
}
```

The old event store replays unchanged: every `User` event written
before the upgrade is missing `phone`, so each old record deserializes
with `phone = ""`. New writes carry `phone`. No migration step, no
consensus. (See the live demo in `examples/08-schema-migration`, which
adds `priority`, `category`, and `featured` this way.)

The schema-change detector classifies this as
`MigrationStrategy::Additive` with `requires_consensus: false`
(`SchemaChangeDetector::determine_migration_strategy_for_add` /
`requires_consensus_for_add`, `lithair-core/src/schema/mod.rs`). Adding
a field *without* a default is treated as breaking (see below).

Bump the declared schema version with the struct attribute
`#[schema(version = N)]` (parsed by `parse_schema_version`,
`lithair-macros/src/declarative_simple.rs`; defaults to `1`). This is a
tracking/diffing number — it does not by itself make a change safe; the
field-level `#[db(default = ...)]` is what makes the data compatible.

## Breaking changes (managed path)

The following are **not** automatically backward/forward compatible.
Removing a field, renaming a field, or changing a field's type means old
events no longer deserialize into the new struct the way the new code
expects — `serde(default)` cannot reconstruct data that was never
written, nor reinterpret a value under a different type.

The detector marks these as `MigrationStrategy::Breaking` with
`requires_consensus: true` (`lithair-core/src/schema/mod.rs`):
`RemoveField`, and `ModifyFieldType` (a field-type change). They are
gated by the schema-migration approval flow.

Detect and manage them through the admin endpoints. **Verified exact
paths and methods** (dispatched in `lithair-core/src/app/mod.rs`,
handled in `lithair-core/src/app/schema_handlers.rs`):

| Method | Path | Purpose |
|--------|------|---------|
| `GET`  | `/_admin/schema` | List current schemas (name, version, field/index counts). |
| `GET`  | `/_admin/schema/pending` | List pending schema changes awaiting approval. |
| `GET`  | `/_admin/schema/diff` | Compare current model specs vs. stored schemas (pre-deploy validation). |
| `POST` | `/_admin/schema/approve/{change_id}` | Manually approve a pending change. |
| `POST` | `/_admin/schema/reject/{change_id}` | Manually reject a pending change. |
| `POST` | `/_admin/schema/sync` | Force schema sync from the cluster leader (cluster mode only). |

Typical flow for a breaking change:

```bash
# 1. Detect what changed between your new code and the stored schema:
curl -fsS http://localhost:8080/_admin/schema/diff

# 2. List anything awaiting approval:
curl -fsS http://localhost:8080/_admin/schema/pending

# 3. Approve (or reject) a specific change by id:
curl -fsS -X POST http://localhost:8080/_admin/schema/approve/<change_id>
curl -fsS -X POST http://localhost:8080/_admin/schema/reject/<change_id>
```

`/_admin/schema/diff` reports each model as `in_sync`, `changed`
(listing each change's `type`, `field`, `strategy`, and `breaking`
flag), or `new`. Breaking changes that require consensus are not
auto-applied — they surface as pending and need explicit approval.

For a **cluster**, schema changes propagate through Raft consensus
(internal `/_raft/schema/*` endpoints) and a node recovering from desync
can pull the leader's schemas with `POST /_admin/schema/sync`. Note that
`sync`'s full leader-communication path is not yet implemented (the
handler logs and returns current state; see `handle_admin_schema_sync`).
Cluster mode is production-stable within the documented operating
envelope (issue #104 — see [`cluster.md`](cluster.md)); because of the
sync stub, a node that missed schema changes while down must be
restarted from current code rather than relying on `sync`.

**Honest limitation.** The framework *detects and gates* breaking
changes; it does not transform arbitrary data. A type change or rename
that needs the actual stored data rewritten still requires a manual data
migration: export the current model state (Strategy A logical export,
`POST /_admin/data/backup` — see [`backup-restore.md`](backup-restore.md)),
transform it, and re-load it into the new schema through the model's
write endpoints. Plan for this when the CHANGELOG or your own model diff
shows a `ModifyFieldType` or `RemoveField`.

## Upgrade procedure

1. **Back up** the event store first (cold/consistent, Strategy B —
   [`backup-restore.md`](backup-restore.md)). This is the rollback plan.
2. **Bump `lithair-core`** in your `Cargo.toml` to the target version,
   and confirm `lithair-macros` matches (they version together; if you
   pin `lithair-macros` directly, keep it on the same minor):

   ```toml
   [dependencies]
   lithair-core = "1.0"
   ```

3. **`cargo build`** — this clears compatibility surface (a), the Rust
   API. Breaking API changes fail here, before anything runs:

   ```bash
   cargo build --release
   ```

4. **Run your tests / CI.** `task ci:full` runs fmt + clippy (`-D
   warnings`) + tests; `cidx run code` gives the same rustfmt + clippy
   feedback in a container matching CI (see project `CLAUDE.md`).

5. **Stage against a copy of prod data.** Restore the backup from step 1
   into a throwaway host/volume, start the **new** binary against it,
   and verify compatibility surface (b):

   ```bash
   curl -fsS http://localhost:8080/health        # → {"status":"healthy"}
   curl -fsS http://localhost:8080/_admin/schema/diff
   ```

   Confirm replay succeeds in the logs (look for the replayed-events
   line), `/health` is healthy, and the diff is `all_in_sync` (additive
   path) or shows only changes you expect to approve (managed path).
   Spot-check a known record via its model endpoint.

6. **Promote.** Deploy the new binary to production and restart it
   against the live event store.

### systemd variant

```bash
cargo build --release -p <your-example>
sudo systemctl stop lithair
sudo install -m 0755 target/release/<your-example> /usr/local/bin/lithair
sudo systemctl start lithair
journalctl -u lithair -f        # watch replay, look for the replayed-events line
curl -fsS http://localhost:8080/health        # → {"status":"healthy"}
```

See [`deployment-systemd-k8s.md`](deployment-systemd-k8s.md) for the
unit and `StateDirectory` details.

### Docker / Kubernetes variant

Build and push a new image tag, then roll it out. The event store lives
on the `lithair-data` volume / PVC and is **not** rebuilt — the new
image replays the existing store on start:

```bash
docker build -t ghcr.io/youorg/lithair:<new-version> .
docker push ghcr.io/youorg/lithair:<new-version>
# k8s: edit image: in examples/deployment/k8s/deployment.yaml, then:
kubectl rollout status deploy/lithair
curl -fsS http://localhost:8080/health        # → {"status":"healthy"}
```

The k8s Deployment uses the `Recreate` strategy with a single replica
(`examples/deployment/k8s/deployment.yaml`), so the old pod stops before
the new one starts against the same PVC — there is no window of two
binaries writing the one event store. See
[`deployment-docker.md`](deployment-docker.md) and
[`deployment-systemd-k8s.md`](deployment-systemd-k8s.md).

## Rollback

Because state is event-sourced, rollback is: **redeploy the previous
binary and restore the pre-upgrade backup**. Reinstall the old
binary/image, restore the Strategy B backup taken in step 1, restart,
let replay run, verify `/health`. The restore procedure (systemd and
Docker variants, ownership fixes) is in
[`backup-restore.md`](backup-restore.md).

**Critical caveat.** Events written under the **new** version after the
upgrade may not be replayable by the **old** binary if the schema
changed. An added field old code can usually ignore; but a type change,
rename, or any event shape the old struct cannot deserialize will be
**silently skipped on replay** (`replay_events` parses each line with
`if let Ok(envelope) = ...` and skips lines that fail to deserialize —
see the hot-backup discussion in
[`backup-restore.md`](backup-restore.md)). So rolling back after
significant new writes can lose or fail to replay those newer events:
restoring the pre-upgrade backup cleanly discards all post-upgrade
writes, while running the old binary against the *post-upgrade* store
risks silently dropping events it can't read.

The clean rollback window is therefore **before significant new writes
under the new version**. This is exactly why step 5 (stage against a
copy of prod data) matters: validating the new binary on a copy first
lets you catch problems without ever needing a production rollback.

## Pre-flight checklist

1. **Read the CHANGELOG** for the target version — breaking changes and
   any `lithair-macros` bump (= mandatory recompile).
2. **Back up** the event store, cold/consistent, and confirm it restores
   ([`backup-restore.md`](backup-restore.md)).
3. **Classify your model changes.** Additive (new field + `#[db(default
   = X)]`) → safe. Removal / rename / type change → breaking, needs the
   approval flow and possibly a manual data migration.
4. **Bump `lithair-core`** (and matching `lithair-macros`) in
   `Cargo.toml`.
5. **`cargo build`** — clears the Rust-API surface; breaking API changes
   fail here.
6. **Run `task ci:full` / `cidx run code`** and your tests.
7. **Stage against a copy of prod data**: replay, `/health`, and
   `/_admin/schema/diff` all clean.
8. **Promote**, then re-verify `/health` and a data spot-check.
9. **Know your rollback window**: clean only before significant new
   writes under the new version.
