# Native model consensus (experimental)

This opt-in runtime connects declarative native models to three-voter OpenRaft.
It is separate from legacy `with_raft_cluster`. Standalone events/replay stay
unchanged. Turso, sessions, membership changes and rolling schema upgrades remain
follow-ups in [#248](https://github.com/lithair/lithair/issues/248).

## Activation

Enable `lithair-core` features `cluster,tls`; provision three independent stores
with the [operator CLI/configuration](../../internal/specs/OPENRAFT_OPERATOR.md).
Each node gets its own key, certificate, store and identity. Existing native
events and legacy Raft WALs are not adopted: import needs a separate verified
migration procedure. Keep the ordinary `DeclarativeModel` declaration:

```rust,ignore
use lithair_core::{app::LithairServer, cluster::native::{Model, NativeCluster}};
let cluster = NativeCluster::open(
    "node.toml", "my-site-v1",
    vec![Model::of::<Article>("site.articles", "/api/articles")?],
).await?;
// Explicit one-time operator action on the designated node, after all peers
// are listening: cluster.bootstrap().await?;
LithairServer::new().with_admin_panel(false)
    .with_native_cluster(cluster).serve().await?;
```

`site.articles` is the storage identity, independent of Rust crate/module or HTTP
path. The explicit application version, sorted model identities and field schemas
determine a shared digest. Every authenticated peer RPC checks it, including votes
and bootstrap preflight. The durable manifest binds it before the first vote.
Changed contracts fail startup: no silent schema updates or data transformations.
Change the explicit version when defaults/validation/serialization behavior changes
beyond the field schema.

`open` binds the dedicated mTLS peer address and opens provisioned storage only.
`bootstrap` checks all three peers before consuming its durable single-use claim.
Restart never bootstraps. Shared Rust operator methods work with management HTTP
and UI absent. Existing admin HTTP/UI, local data-admin, hooks and custom routes
are refused until they have consensus-aware implementations. Public HTTP exposes
no peer RPC/bootstrap route. Future optional management API/UI must remain separate
from both public CRUD and mTLS peers.

## HTTP and durability

Mutations require `Idempotency-Key`: 1–128 ASCII letters, digits, dots, underscores
or hyphens, scoped to cluster plus stable model identity. Only anonymous model
permissions are supported: declarations are checked against an empty permission
set. Client headers cannot assert internal roles; local session/RBAC stores fail
startup rather than authorizing from stale state.

The leader orders preparation against committed state, resolves serde defaults,
IDs and timestamps once, validates fields, primary keys, immutable values and
uniqueness, then proposes canonical data and its result. Concurrent PATCH requests
preserve unrelated fields. Followers execute no model callbacks, clocks or ID
generators. A revision precondition prevents stale preparation overwriting
intervening changes after timeouts or leadership changes.

Success requires quorum-durable commitment **and durable application on the
responding leader**. One atomic checkpoint contains every native model, membership,
applied log position, application revision and retained results. The memory view
changes only after successful publication. Apply/I/O failure closes readiness
until recovery.

The last **256 distinct committed results across the group** are retained in commit
order, including validation/conflict results. Matching retries return their original
status/body across deletes, leader changes and restarts. Different method, record
identity or JSON with a retained key returns 409. Retries do not extend retention.
Outside this window a key can execute again: reconcile long-delayed ambiguous
outcomes. Stale preparation produces a durable 409; reread and use a new key for
a new attempt.

Timeout, 503 and disconnect outcomes can be **unknown**; they do not roll back
commands. Retry through the ready leader with the same key/body within retention.
Admission is bounded to 128 requests; HTTP waits 3 seconds and the owned proposal
task up to 6 seconds. Submitted commands remain governed by consensus afterwards.

POST returns 201; PUT/PATCH 200; DELETE 204 without a body. GET lists share native
filtering/sorting/pagination: `{data,total,skip,take,has_more}`, with `skip`, `take`,
`sort` and field filters. GET/HEAD perform a fresh quorum barrier, wait for local
apply, then read memory. Followers and isolated leaders return 503 with
`Retry-After: 1`, without private-address redirects. Route via `/ready`, which
qualifies the current leader; `/health` only reports liveness. Every operation
still checks consensus after external readiness probes.
OPTIONS returns 204 with an Allow header without requiring quorum.

## Limits and recovery

This first implementation serializes and syncs the complete checkpoint per applied
batch. It favors one auditable durability boundary for small datasets, not high
throughput. Individual reads use the memory map, lists evaluate their collection,
and neither performs disk I/O. Write cost scales with retained state. Bodies are
limited to 64 KiB, canonical commands to 256 KiB and checkpoints to 48 MiB. Capacity
rejection returns 507 before proposing growth; deletes can recover space.

Declarations use ordinary serde names with optional defaults. Custom serde
transforms/flattening, handwritten handlers, relations, owner policies, history,
audit retention, eviction, local compaction, SSE and mutation hooks are refused.
Primary keys use the request-key alphabet/length, excluding `.` and `..` and the
reserved helper names `count`, `random-id`, `_schema`, `stream` and `_bulk`.
Those helper endpoints return 404 in this runtime. Bulk and local programmatic
mutations are unavailable. Configure firewall rules on the server explicitly.
There is no Turso/session replication, live schema transformation or membership/
certificate replacement in this runtime.

Snapshots contain the complete durable application state. Lagging nodes install
them before reporting progress; covered logs can be purged/compacted. Cold restart
checks identity, application digest, snapshot metadata and retained journal before
replay. Filesystems must honor file/directory sync, atomic rename and exclusive
locks. Process-death tests do not emulate power loss.

Application binding adds `application` to the version-3 bound-node manifest.
Earlier binaries reject the unknown field. Downgrade and implicit migration are
unsupported; retain a verified offline backup before activation.

## Qualification

`cidx run test` includes state-machine crash/error tests, generated HTTP/restart
regressions and registered `native_cluster.feature` scenarios. Child processes die
at seven checkpoint publication boundaries; recovery checks data, position and
results together.

`cidx run cluster` adds Probatum checks on three Compose nodes with separate
credentials, volumes and replication/control networks. Generated CRUD runs with
admin/UI disabled. Assertions cover concurrent fields/uniqueness, lost response
and retry, leader SIGKILL, minority refusal, snapshot catch-up after compaction,
and cold restart with retained DELETE/creation results. Evidence/cleanup logs
remain in `.probatum/runs/`. Lower-level consensus suites remain in the pipeline.
