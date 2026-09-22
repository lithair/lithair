# RFC 248: reliable three-node deployment

Status: agreed direction, **not implemented or production-qualified**.
Delivery tracker: [#248](https://github.com/lithair/lithair/issues/248).
Architecture: [#249](https://github.com/lithair/lithair/issues/249).

## Decision and boundary

Run one application on three independent VMs, with three full voting replicas
and a quorum of two. Every VM stores native models, embedded Turso models and
persistent sessions. The third node is a full replica, not a witness. The target
tolerates one unavailable VM; an upgrade consumes that fault allowance.

Keep ordinary declarative model registration and opt-in `#[storage(turso)]`.
Cluster configuration belongs to the server and deployment. Native models retain
their memory-first reads; core does not acquire a dependency on the Turso driver.
Single-node applications remain independent of clustering. Each model has one
authoritative backend, as in [RFC 235](235-hybrid-storage.md). This design does
not provide transactions spanning native and SQL models.

Use one OpenRaft consensus group for the application's authoritative mutations.
OpenRaft owns elections, terms, votes, membership and log agreement; Lithair owns
durable storage and deterministic application of committed commands. See the
[OpenRaft integration interfaces](https://docs.rs/openraft/0.9.25/openraft/docs/getting_started/index.html)
and the [Raft paper](https://raft.github.io/raft.pdf). The intended failure model
is process, disk and network failure, not malicious voting peers.

## Current release versus target

The following describes the audited **1.12.2** implementation, not the target:

| Surface | Current behavior | Required change |
|---|---|---|
| Leadership | Static initial leader and lowest-live-ID failover | OpenRaft with durable votes and recovery |
| Native cluster mutations | Separate dispatch and replication path | Shared HTTP contract and ordered application |
| Turso | Startup rejects native clustering with SQL models | Explicit replicated backend capability |
| Persistent sessions | Local event store per process | Ordered session mutations and consistent authorization |
| Peer addressing | Public redirects use a loopback leader address | Explicit multi-host identity and internal forwarding |
| Readiness | `/ready` reports process readiness | Recovery, compatibility and quorum-aware admission |
| Upgrade tests | Some real-process replication/failover tests; broader mocks | Repeated failures, partitions and hybrid rollout |

Source anchors: `cluster/mod.rs`, `app/model_dispatch.rs`, `app/replication.rs`,
`app/ops_endpoints.rs`, `session/persistent_store.rs` and the SQL startup guard in
`app/mod.rs`, all under `lithair-core/src/`. Historical measurements in the
[cluster runbook](../operations/cluster.md) do not qualify this deployment.
The existing `cluster/upgrade.rs` types and
[rolling-upgrade draft](../internal/specs/ROLLING_UPGRADE_SPEC.md) are not evidence
of a working upgrade protocol. Keep the Turso startup guard until its replacement
has executable coverage.

## Identity, transport and bootstrap

Server configuration supplies a cluster ID, stable node ID, peer RPC addresses,
public entry point, local data directory and peer credentials. Each VM owns its
files; nodes never share a database file or copy another node's vote/log identity.
Reject duplicate identities, unexpected clusters and incompatible wire versions.

Use a dedicated internal HTTPS listener with mutual TLS and configured trust.
Bind the authenticated peer identity to cluster membership. Bound RPC sizes,
timeouts and concurrent work. The public listener does not expose the internal
consensus plane. Credential provisioning and rotation must be exercised before
deployment qualification, not inferred from private-network placement.

Bootstrap the initial three-member configuration once, through an explicit
operator action. A restart recovers membership and votes; it never initializes a
new cluster because peers are temporarily unreachable. Replacement nodes require
an explicit membership procedure. General autoscaling is outside the first target.

Clients use a stable public origin. A follower forwards a mutation internally to
the leader with a bounded hop count and authenticated forwarding context; it
does not redirect a browser to a private or loopback address. The leader validates
the end-user session and permissions. Client-supplied identity headers are not
trusted. The public proxy's availability is a separate deployment responsibility.

## Durable commands and the success boundary

Introduce a versioned consensus storage format separate from the legacy cluster
WAL. Persist votes, entries and snapshot metadata according to OpenRaft's storage
contract. Check format versions, lengths and checksums; propagate I/O errors.
Corruption must not silently become an empty cluster. Durability completion must
mean the promised write has reached stable storage, under documented filesystem
assumptions. See the [log storage contract](https://docs.rs/openraft/0.9.25/openraft/storage/trait.RaftLogStorage.html).

A command identifies a trusted namespace, stable model/collection ID, operation,
request ID and version. Rust crate paths and HTTP route prefixes are not durable
identities. The leader resolves generated IDs, timestamps and other nondeterministic
values once. Followers apply canonical commands without rerunning arbitrary
callbacks or generating new values.

Preserve validation, permissions, field filtering and status codes of generated
routes, including `204 No Content` for DELETE. State-dependent authorization and
mutation preconditions must be evaluated against the ordered state at application
time, not only against a pre-proposal snapshot. Versioned deterministic checks or
explicit preconditions must make concurrent role changes and updates safe.
Unsupported hooks must prevent cluster activation instead of being silently skipped.
External side effects need a separate durable delivery design.

A successful mutation response requires **durable quorum commitment and successful
durable application on the responding leader**. A follower may still be catching
up. A committed command with an unknown application/response outcome cannot be
reported as definitely absent. Cancellation or a dropped response does not undo
commitment; application recovery completes the ordered work.

Define a durable request-ID/result record with bounded, documented retention.
Within that window, a repeated ID with the same payload returns the stored result;
the same ID with a different payload conflicts. Scope IDs to the trusted caller
and namespace. After retention, require reconciliation rather than promising
deduplication. Do not claim exactly-once HTTP delivery or external side effects.

## Applying native state, Turso and sessions

Add an internal replicated-backend capability with prepare/check, committed apply,
checkpoint and snapshot operations. It must not depend on a native-versus-SQL
boolean. All mutation entry points in cluster mode, including programmatic stores,
custom routes, administrative actions and background cleanup, must use the ordered
authority or explicitly reject the operation. Local standalone handles must not
be a bypass for clustered models.

- **Native:** preserve event compatibility and rebuild memory from durable state.
  Use a durable command identity/checkpoint so replay cannot duplicate an event or
  audit effect after a crash between persistence and acknowledgement.
- **Turso:** each node applies the committed document change, request result and
  applied checkpoint in one local SQL transaction. Reads continue to use SQL.
  Existing model batches remain atomic within their documented model/namespace.
  Startup must not independently run a migration ahead of the cluster's schema.
- **Sessions:** replicate creation, refresh, logout, revocation and role-affecting
  changes. Persist absolute expiry values selected by the authority. Expiry checks
  require a documented clock-skew bound and must fail closed when that bound cannot
  be established; monotonic timers only schedule local work.

Order the state machine globally even though stores have separate checkpoints.
Advance the global applied watermark only when every participating store has
completed all commands through that index (including explicit no-ops for commands
targeting another store). A store failure stops application and public readiness;
restart replays idempotently from durable checkpoints. This is recovery ordering,
not an atomic transaction across stores.

Authorization and account/session reads require a current consensus read barrier
and application through its index. The default for clustered mutable model reads
is also consistent; a follower may obtain the barrier from the leader and wait for
local application. Native lookups then use memory, without collection scans or
disk reads. A minority cannot authorize against stale revocations. Any future
stale-read option must be explicit and excluded from authentication decisions.

## Snapshots, recovery and existing data

A snapshot represents one committed, fully applied index across native data,
Turso, sessions, membership and retained request results. Establish a consistent
barrier across stores before exporting it. Snapshot creation must not prune log
entries still needed by another store. Include format/schema versions and checksums.
Restore into staging, validate all components, then atomically activate a manifest
pointing to the complete generation. A crash during installation cannot expose a
mixture of generations; the node stays unready until recovery finishes.

Keep ordinary native event replay compatibility. Migration from the current
cluster is an explicit maintenance operation: stop writes, select and verify the
authoritative application state, export all stores, then bootstrap the new group
from that verified state. Reconcile divergent copies before import. Never
reinterpret a legacy log index as proof of OpenRaft commitment. Online conversion
of an existing cluster and automatic divergence resolution are outside this RFC.

## Readiness and progressive deployment

Liveness means the process runs. Public readiness additionally requires complete
restoration, compatible command/schema versions, application through a confirmed
read barrier, usable backends and a reachable majority. Draining or recovering
nodes stay out of public traffic. Use fresh consensus evidence and bounded
readiness validity; an old `is_leader` flag is insufficient. Public draining does
not prematurely stop the peer listener or its participation in consensus.

Deployment changes one binary at a time, within the same logical data cluster:

1. Check that all three nodes are healthy and that the new binary is explicitly
   compatible with the current wire, command, snapshot and schema versions.
2. Drain one follower, upgrade it, catch it up, validate it, and return it to service.
3. Repeat for the second follower only after the first has fully recovered.
4. Transfer leadership to a validated upgraded node and confirm the new leader.
5. Drain and upgrade the former leader; restore three healthy replicas.

Pause if another node fails or compatibility cannot be established. Do not run
independent writable blue and green copies of the same application data.
Temporary retryable errors during leadership change remain possible; the promise
to qualify is preservation of acknowledged writes, not zero failed requests.

Negotiate a supported capability intersection while versions coexist. Software
semver alone does not establish wire, callback or on-disk compatibility. Ordered
migration commands need deterministic, compatible execution on every replica;
stage schema changes using expand/contract steps. Enable new commands only after
every participating binary supports them. Refuse unsafe downgrades; a destructive
migration cannot be made reversible by calling it an automatic rollback.

## Administration target

Provide a dedicated authenticated cluster/deployment console for node health,
leader/quorum status, replication lag, versions and guarded operator actions. Its
access is separate from public application traffic and the internal mTLS peer
listener. Existing data administration does not implement these cluster controls.
The console must use the same validated operational APIs as operator tooling;
it cannot bypass quorum, compatibility, recovery or authorization checks.

## Delivery and acceptance

Each milestone gets a draft PR before implementation and must leave unsupported
paths rejected. Merging this RFC closes only #249, not the delivery tracker #248.

| Milestone | Deliverable and exit evidence |
|---|---|
| 1. Durable consensus storage | [#251](https://github.com/lithair/lithair/issues/251): private OpenRaft log/vote implementation, upstream storage suite and crash/reopen regressions |
| 2. Three-node consensus | [#253](https://github.com/lithair/lithair/issues/253): authenticated transport and real-process election/partition tests with a test state machine. [#255](https://github.com/lithair/lithair/issues/255): durable snapshots and physical compaction. [#257](https://github.com/lithair/lithair/issues/257): persisted identity and guarded bootstrap. [#259](https://github.com/lithair/lithair/issues/259): offline operator tooling and shared-plan preflight. [#261](https://github.com/lithair/lithair/issues/261): Probatum/Compose failure qualification on separate containers. Live administration, membership replacement and credential rotation remain follow-ups |
| 3. Native integration | Ordered native writes, idempotency, consistent reads and generated-route contract parity; restart and old-leader return tests |
| 4. Turso integration | Replicated SQL apply/checkpoints, coordinated schema changes and hybrid snapshots; crash/replay tests without duplicate mutations |
| 5. Sessions and admission | Session continuity/revocation, clock handling, complete hybrid recovery and quorum-aware readiness |
| 6. Deployment qualification | Leadership transfer, mixed-version rejection and follower/follower/leader rollout under writes, then failure trials on three independent VMs |

The real-process harness must use isolated data directories, reserved ephemeral
ports and deadline polling. Fault injection must actually interrupt processes or
transport: a step that only prints a simulated partition is not evidence.
For every acknowledged native, SQL or session mutation, assert durable recovery
after one-node loss, repeated elections and the old leader's return. Exercise a
minority partition, asymmetric connectivity, dropped responses and retries,
duplicate delivery, disk failures, and snapshot installation interrupted by death.
Assert state and response contracts rather than fixed failover timings.

Add executable Gherkin coverage alongside each runtime promise and register new
features with `no_orphan_features`. Put a bounded three-process correctness subset
in the cidx PR gate; run heavier load and three-VM qualification through documented
cidx configurations. Today's default gate excludes cluster BDD and cannot supply
that evidence. Documentation CI passing means the RFC is reviewable, not that a
hybrid deployment is ready.
