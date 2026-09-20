# OpenRaft node identity and explicit bootstrap

[#257](https://github.com/lithair/lithair/issues/257) extends the private
[durable log](OPENRAFT_STORAGE.md) and [peer transport](OPENRAFT_TRANSPORT.md).
This is still a foundation for [RFC 248](../../rfcs/248-three-node-cluster.md),
not a public server builder or a production deployment API.

## Provisioning and recovery

`PeerTransport::node_identity()` derives enrollment from the
validated transport configuration: cluster ID, local node ID, exactly three
voter IDs with distinct certificate fingerprints, and one designated bootstrap
node. The local and bootstrap IDs must belong to that group. This initial group
is immutable for this milestone. All three operators must provision the same
cluster, enrollment and bootstrap designation, with each node's own local ID.
The [operator tooling](OPENRAFT_OPERATOR.md) validates a shared plan digest on
every peer RPC and requires all three peers to agree before initial bootstrap.

`DurableLog::create_for_node(directory, identity)` explicitly provisions an empty,
dedicated directory. `open_for_node(directory, expected_identity)` only recovers
an existing directory. Neither operation falls back to the other. The caller
provides a trusted directory and durably provisions its parents as described in
the storage contract. The exclusive store lock still prevents concurrent local
writers; identity checks do not make cloning a whole directory/key to another
VM safe or detect every duplicate process across hosts.

The new version-3 `durable.meta` embeds a `node` record with the identity and a
`bootstrap_claimed` boolean. The same checksummed manifest binds that record to
the store UUID, journal generation/offset and optional snapshot generation.
There is no independently swappable identity sidecar. Existing journal and
snapshot framing is unchanged. Every append, checkpoint and compaction preserves
this binding and version; old binaries reject version 3.

Recovery validates the expected identity before opening/replaying/truncating the
journal or reclaiming orphan generations. Wrong node, cluster, certificate
assignment, voter set or bootstrap designation is an error, even with otherwise
valid storage. Missing/corrupt identity, an unknown format or a manifest from a
different store also fails recovery. A rejected configuration leaves journal
bytes and unselected generations untouched.

The old unbound version-1/version-2 APIs remain available for the private storage
suite; they cannot open a bound store. The bound APIs cannot adopt an unbound
store. There is no migration from legacy Lithair cluster WALs.

Dial addresses and TLS server names remain explicit transport configuration;
they are checked by the transport but are not persisted in this identity record.
An address can change on restart while the certificate identity stays the same.
Trust roots and private keys are supplied separately; only certificate
fingerprints are stored here. Membership replacement and credential rotation
need explicit later procedures. Editing the manifest is not such a procedure.

## One explicit bootstrap action

Provisioning starts a learner with no group. A restart of provisioned, empty nodes
still does not initialize a group. Peer unavailability never triggers bootstrap.
The test operator invokes `PeerTransport::bootstrap(&store, &raft)` through its
parent/child control channel. After authenticated preflight of both remote peers,
this invokes `DurableLog::bootstrap(&raft)`; neither the internal peer HTTPS
listener nor the public HTTP listener exposes this action.

The helper checks the Raft node ID, then obtains a single-use `BootstrapPermit`:

1. Check that this is the designated node, no prior claim exists, and the store
   has no vote, log entries, commit/purge watermark or snapshot.
2. Serialize the claim through the same owned storage mutex as consensus writes.
3. Write/sync a replacement manifest, rename it, and sync the directory before
   returning the permit.
4. Consume the permit to call OpenRaft `initialize` with exactly the enrolled
   voters. The caller must pair this store and Raft instance correctly.

Concurrent callers produce at most one permit. The permit is neither cloneable
nor serializable. Dropping it, canceling the caller after admission, an initialize
error, compaction or restarting cannot reset a durable claim. OpenRaft separately
rejects initialization once consensus state exists.

A bootstrap claim is local durable intent, **not proof of quorum commitment or
application readiness**. I/O failure invalidates all store handles; reopening
resolves which manifest is durable. A claim interrupted before activation can
remain absent and permit a later explicit attempt after inspection. Once the
claim is present, even if a crash happened before calling OpenRaft, bootstrap is
refused. There is no automatic claim reset or interrupted-bootstrap repair in
this milestone. Inspect the whole group; do not delete a claim or initialize new
state merely because a leader is unavailable. Existing data must be recovered
through a separately defined operator recovery procedure.

## Executable evidence and remaining work

`openraft_identity_test` covers identity/configuration drift without disk mutation,
invalid enrollment, missing/unbound/corrupt/swapped manifests, concurrent claims,
consensus-history rejection, binding through compaction and snapshots, caller
cancellation, I/O failure and actual process death at five manifest publication
boundaries. Recovery validates the original identity before any bootstrap attempt.

The real three-process fixture derives its binding from its mTLS configuration,
uses only explicit provisioning/recovery and the guarded bootstrap operation,
and verifies that fresh nodes stay uninitialized through a cold restart. It
rejects follower bootstrap, wrong cluster/node recovery and rebootstrap after
acknowledged writes. Existing partition, leader-crash and snapshot catch-up cases
exercise the same bound stores. The registered `openraft_identity_bdd` runner
executes `features/core/openraft_identity.feature` in the cidx test gate.

Offline CLI preparation and inspection are described in the
[operator contract](OPENRAFT_OPERATOR.md). Live operator CLI/UI, persistent
membership replacement, credential rotation, native
models, Turso, sessions, public readiness and rolling deployment remain follow-up
work. The planned authenticated cluster/deployment console is separate from the
public application and internal mTLS replication access. These tests use a
MemStore application fixture and do not qualify deployment on three VMs.
