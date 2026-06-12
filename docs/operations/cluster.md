# Cluster operations runbook

Lithair cluster mode is **production-stable within the operating envelope
documented here** (G1 decision, issue #104, 2026-06-12). That phrase is
load-bearing: inside the envelope the cluster has a measured stress record
of zero drops, zero panics, and zero replication divergence; outside it
(higher write rates, multi-host redirects, runtime membership changes) you
are off the tested path. Read the [election model](#election--failover-model)
and [limitations](#limitations) before deploying — Lithair's cluster is
**not textbook Raft**, and operating it safely depends on knowing how it
actually behaves.

Everything below traces to `lithair-core/src/cluster/`,
`lithair-core/src/app/mod.rs`, and the measured results posted on
issue #104 (v0.13.0). Where behavior may change between versions, it is
marked "verify against your version".

## Topology and startup

A cluster node is an ordinary `LithairServer` with clustering enabled:

```rust
LithairServer::new()
    .with_port(8080)
    .with_raft_cluster(0, vec!["127.0.0.1:8081", "127.0.0.1:8082"])
    .with_model::<Product>("./data/node_0/products_events", "/api/products")
    .build()?
    .serve()
    .await
```

The reference binary is `lithair-cluster-node`
([`examples/09-replication/cluster_node.rs`](../../examples/09-replication/cluster_node.rs)):

```bash
cargo build --release -p replication --bin lithair-cluster-node

# Terminal 1 — node 0 (initial leader, see election model below)
lithair-cluster-node --node-id 0 --port 8080 --peers 8081,8082
# Terminal 2 — follower
lithair-cluster-node --node-id 1 --port 8081 --peers 8080,8082
# Terminal 3 — follower
lithair-cluster-node --node-id 2 --port 8082 --peers 8080,8081
```

`--peers` is a comma-separated list of the *other* nodes' ports; the peer
list is fixed for the lifetime of the process (see
[limitations](#limitations)). **Always include a node with ID 0** — the
static election makes node 0 the initial leader; without it the cluster
starts leaderless and only converges after the first election timeout.

### Per-node data directories (mandatory)

Raft state (WAL + snapshots) lives under
`{base}/raft/node_{id}/{wal,snapshots}` where `{base}` resolves as
`LITHAIR_DATA_DIR` > `EXPERIMENT_DATA_BASE` > `./data`
(`lithair-core/src/app/builder.rs::raft_base_dir`). The example binary
additionally scopes each model's event store under `{base}/node_{id}/`.

Two nodes must **never** share a WAL path. The original #29 "reliably
failing multi-write" hang was exactly this (fixed in commit bbfeb44):
hard-coded relative WAL paths were shared across runs, every start
replayed stale entries, and a fresh write waited forever on a phantom
prior log index. Distinct `--node-id` values give distinct directories
automatically — keep it that way, and never copy one node's `raft/`
directory onto another node.

### Liveness and leadership checks

| Endpoint | Returns |
|---|---|
| `GET /health` | exactly `{"status":"healthy"}` — process liveness only, no cluster info |
| `GET /status` | `{"status":"ready",...,"raft":{"enabled":true,"node_id":N,"is_leader":bool,"leader_port":P,"peers":N}}` |
| `GET /raft/leader` | leader id/port as this node sees it (token-protected if auth enabled) |
| `GET /_raft/health` | term, commit/applied index, per-follower health (healthy/lagging/desynced), latency, snapshot status |
| `GET /_raft/sync-status` | leader only: per-follower replication lag in entries |

`/status` is the endpoint the cluster itself uses for elections — if it
answers, the node is a live election candidate. A follower's
`leader_port` is `0` until it receives its first heartbeat.

### Securing the cluster ports

`X-Raft-Token` auth (`with_raft_auth(token)` / `LITHAIR_RAFT_TOKEN`)
protects only `{raft.path}/heartbeat` and `{raft.path}/leader`. The
replication endpoints (`/_raft/append`, `/_raft/snapshot`,
`/internal/replicate*`) accept **unauthenticated** POSTs in v0.13.0.
Cluster nodes must only be reachable from each other and from trusted
clients — put them on a private network segment. Do not expose a cluster
node's port to the public internet.

## Election & failover model

**This is not Raft leader election.** There are no randomized timeouts,
no vote RPCs, and no term increments on failover. What ships
(`lithair-core/src/cluster/mod.rs::RaftLeadershipState`):

- **Static initial leadership.** A node boots as leader iff its
  `node_id == 0` (or it has no peers). Everyone else boots as follower.
  Deterministic: same topology, same leader.
- **Heartbeats.** The leader sends empty `AppendEntries` to all peers
  every ~1.7 s (election timeout / 3, from the background replication
  task) plus a legacy JSON heartbeat to `{raft.path}/heartbeat` every 2 s
  (`heartbeat_interval_secs`). Both refresh the follower's heartbeat
  clock and teach it the leader's port.
- **Failure detection.** A follower that sees no heartbeat for 5 s
  (`election_timeout`, fixed in `RaftLeadershipState::new` — the
  `LITHAIR_RAFT_ELECTION_TIMEOUT` config knob is *not* wired into this
  struct in v0.13.0) starts an "election".
- **Election = lowest alive node ID wins.** The candidate polls every
  peer's `/status` (2 s timeout each); among the nodes that answered
  (plus itself), the lowest node ID becomes leader. No quorum of votes is
  required to *become* leader — but a leader without a majority cannot
  *commit* (next section), which is what actually prevents committed
  split-brain writes.

The BDD suite (`cucumber-tests/features/core/real_cluster_test.feature`)
exercises exactly this: "Static leader election with lowest node ID" and
"Follower detects leader failure and triggers election" (kill leader,
wait 12 s, new leader elected, cluster operational). Budget **up to ~12 s
of write unavailability** for a leader failover: 5 s detection + status
polls + heartbeat propagation of the new leader's port.

### What clients experience

- Writes to a follower get **307 Temporary Redirect** with `Location` and
  `X-Raft-Leader` headers pointing at the leader. The `Location` host is
  hardcoded `127.0.0.1` — redirects only work when client and cluster
  share a host. Multi-host clients must discover the leader via `/status`
  and write to it directly.
- If the follower does not yet know the leader's port (before the first
  heartbeat, e.g. right after an election), writes get **503 +
  `Retry-After: 1`** with body
  `{"error":"Leader port not yet discovered, retry after heartbeat"}`.
  Back off and retry.
- During the dead-leader window, redirects still point at the dead
  leader (connection refused) until the election completes. Clients need
  retry logic for the full failover budget.
- Reads are served locally by every node, leader or follower, with no
  read lease — follower reads can trail the leader briefly (replication
  is commit-notification plus a 100 ms catch-up tick).

### Old-leader rejoin: the honest part

Because leadership is static, a **restarted node 0 always boots believing
it is the leader**, even if another node was elected while it was down.
Reconciliation happens through AppendEntries: any node that accepts an
AppendEntries from another leader steps down to follower (the PR #39
step-down in `handle_raft_append_entries`), and AppendEntries carrying a
*lower* term than the receiver's are rejected. But elections never
increment terms, so the rejoining node 0 and the incumbent usually hold
**equal terms**, and which one ends up leader is decided by whose
AppendEntries lands first — in practice the lowest ID tends to reclaim
leadership, but for a window of a few seconds (bounded by the ~1.7 s
heartbeat cadence) **both nodes may accept writes**. Writes accepted by
the losing side in that window can be lost or conflict.

Two operational rules follow:

1. **Pause client writes when restarting a previously-failed node 0**
   (or any node that was leader when it died). Verify single leadership
   with `/status` on every node before resuming writes.
2. A node that booted as leader and was later demoted does **not** run
   the follower election monitor (it is only spawned on nodes that boot
   as followers). Single leader failover is the designed and BDD-tested
   case; after *multiple* successive failovers, if `/status` disagrees
   across nodes about who leads, do a full-cluster cold start (below)
   rather than improvising.

## Operating envelope (measured)

Sustained-load results from issue #104 (2026-06-12, v0.13.0, 3-node
local cluster, release build, single-write mode, ~1 KB items):

| Stage | Load | Outcome | Latency |
|---|---|---|---|
| Sequential | 200 writes, conc 1 | 200/200, replication exact | p50 7.8 ms, p99 10 ms |
| Moderate | 10 000 writes, conc 64 | 10k/10k, replication exact | p50 258 ms, p99 340 ms |
| Saturation | 20 000 writes, conc 512 | 20k/20k, replication exact | p50 2.28 s (pure queueing) |
| Endurance | 140 000 writes, conc 64, 11.3 min | 140k/140k, replication exact (170 200 on all 3 nodes) | p50 296 ms, p99 534 ms, stable |

Verdict across 170 200 total writes: zero drops, zero panics, zero
replication divergence. Leader RSS grew 135 → 307 MB — i.e. the dataset
itself, exactly as the memory-first model in
[capacity-planning.md](capacity-planning.md) predicts (RAM ≈ item_count ×
item_size); disk was 156 MB append-only.

**The envelope's hard edge: ~210–240 single-write ops/s per leader**,
flat across concurrency 1 → 64 → 512. This is a stable, predictable
ceiling, not an instability — every write is one consensus round (WAL
group commit at a 5 ms interval, in parallel with a synchronous
replication round that waits for majority ack). Raising client
concurrency does not raise throughput; it only adds queueing delay
(Little's law — the 2.28 s p50 at concurrency 512 is the ceiling, not a
malfunction). Mild decay over the endurance run (241 → 206 ops/s) is
under investigation in the perf workstream (#126).

Levers if you need more than ~200 writes/s:

- `POST /api/{model}/_bulk` batches many items per request — but in
  v0.13.0 **cluster mode** the consensus write path records a `_bulk`
  POST as a single `Create` log entry without batch semantics (the code
  comment in `handle_model_request` says "Proper BatchOperation support
  is not yet available"). Verify bulk replication end-to-end on your
  version before relying on it for cluster ingest.
- Ceiling-raising work (true batch operations, group-commit tuning) is
  tracked in #126.
- Reads do not count against the ceiling — they are served from memory
  on every node and scale independently.

## Quorum & recovery

Writes commit on **majority acknowledgment**: quorum = N/2 + 1 including
the leader (`replicate_log_entries_to_followers`). For a 3-node cluster,
quorum is 2.

- **One follower down (3-node):** writes continue. The follower is
  marked lagging, then desynced (>1000 entries behind, or unresponsive
  >30 s with pending work). Desynced followers are excluded from the
  replication round and recover via snapshot resync.
- **Leader down:** writes unavailable for the failover budget (~12 s),
  then resume on the new leader. Committed writes are on a majority and
  survive.
- **Majority down:** the surviving node may still *call itself* leader
  (lowest-alive-ID election needs no votes), but every write fails with
  **503 `{"error":"Replication failed: ..."}`** because majority ack is
  unreachable. Reads continue from local state. This is the committed-
  split-brain protection: commit requires majority, regardless of who
  claims leadership. Treat a 503 on write as *indeterminate* — the entry
  may already be in the leader's WAL and on some followers; retry with
  idempotent semantics (client-supplied IDs).

### Restarting a node

1. Start the binary with the **same `--node-id` and same data
   directory**. The WAL replays into the consensus log
   (`replay_from_wal_entries`); `commit_index` deliberately restarts at 0
   and is re-established by the leader's AppendEntries — this is normal,
   not data loss.
2. Watch `/_raft/health` on the leader: the node should go
   `desynced/lagging → healthy`. Deep gaps are healed by snapshot resync
   (automatic; `POST /_raft/force-resync` to trigger manually).
3. Schema state does **not** auto-heal: `POST /_admin/schema/sync` is a
   stub in v0.13.0 (logs and returns current state — see
   [upgrade.md](upgrade.md)). If schemas changed while the node was
   down, restart it from current code so it derives the same schemas.
4. If the restarted node was previously the leader, follow the
   [rejoin rules](#old-leader-rejoin-the-honest-part) above.

### Full-cluster cold start

Start node 0 first (it is the static leader), confirm `/status` shows
`"is_leader":true`, then start the followers and wait for
`leader_port != 0` on each. Starting all nodes simultaneously also
converges, but ordering removes the leaderless window. Each node replays
its own WAL and event stores — restore per-node data directories from
backup *to the same node ID* if needed
([backup-restore.md](backup-restore.md)).

## Kubernetes note

The bundled manifests
([deployment-systemd-k8s.md](deployment-systemd-k8s.md),
`examples/deployment/k8s/`) deploy **one single-node instance** and must
stay at `replicas: 1` — scaling a Deployment does not create a cluster,
it creates diverging independent stores. Running cluster mode on k8s is
a different topology (one pod per node with stable identity and its own
volume, StatefulSet-style, distinct `--node-id` per pod) and is out of
scope for the bundled manifests.

## Limitations

- **Single-leader writes** at a measured **~210–240 ops/s ceiling**
  (v0.13.0, see envelope above). Scale reads horizontally, writes do not
  scale with node count.
- **Static lowest-ID election, not Raft voting.** No term increments on
  failover; old-leader rejoin has a brief dual-leader write window;
  protection against *committed* split-brain comes from majority-ack on
  the write path, not from the vote machinery.
- **A demoted boot-leader never self-elects** (election monitor only
  spawns on boot-followers). Multi-failover sequences beyond the tested
  single-failover are unsupported; cold-start if leadership is ambiguous.
- **307 write redirects hardcode `127.0.0.1`** — same-host clusters
  only; multi-host clients need leader discovery via `/status`.
- **No runtime membership changes.** The peer list is fixed at process
  start; adding or removing a node means a rolling restart of every node
  with the new peer list (and a leaderless/contended window, so pause
  writes).
- **`/_admin/schema/sync` is a stub** ([upgrade.md](upgrade.md));
  cluster `_bulk` lacks true batch semantics (verify against your
  version).
- **Replication endpoints are unauthenticated** in v0.13.0 — network
  isolation of cluster ports is mandatory.
- **Follower reads are eventually consistent** (no read-index/lease).
