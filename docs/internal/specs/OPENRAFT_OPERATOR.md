# Experimental cluster operator tooling

[#259](https://github.com/lithair/lithair/issues/259) adds offline tools for the
[identity-bound store](OPENRAFT_IDENTITY.md) and a shared-plan bootstrap preflight
for the [peer transport](OPENRAFT_TRANSPORT.md). This is part of
[RFC 248](../../rfcs/248-three-node-cluster.md), with a test state machine. It does
not start or replicate native models, Turso or sessions.

## Offline commands

Build the opt-in CLI from this checkout:

```bash
cargo install --path lithair-cli --features cluster-ops
lithair cluster check --config /etc/lithair/node.toml
lithair cluster provision --config /etc/lithair/node.toml
lithair cluster inspect --config /etc/lithair/node.toml
```

Each successful command prints one JSON object. Errors go to stderr and exit with
status 2, without printing configuration contents or private key material. These
commands do not open a listener or initialize a consensus group. The default CLI
build still provides the existing scaffolding and event-store verification tools.

Example for node 1; replace all fingerprint placeholders with the SHA-256 of each
leaf certificate's DER encoding (64 hexadecimal digits):

```toml
version = 1
cluster_id = "site-production"
node_id = 1
bootstrap_node = 1
data_dir = "/var/lib/lithair/consensus"
tls_ca = "tls/ca.pem"
tls_certificate = "tls/node.pem"
tls_key = "tls/node-key.pem"

[[peers]]
node_id = 1
address = "10.0.0.11:9443"
server_name = "node1.internal"
certificate_sha256 = "REPLACE_WITH_NODE_1_SHA256"

[[peers]]
node_id = 2
address = "10.0.0.12:9443"
server_name = "node2.internal"
certificate_sha256 = "REPLACE_WITH_NODE_2_SHA256"

[[peers]]
node_id = 3
address = "10.0.0.13:9443"
server_name = "node3.internal"
certificate_sha256 = "REPLACE_WITH_NODE_3_SHA256"
```

All three files describe the same cluster, bootstrap node and three enrolled
voters. Change `node_id` and local file paths per host. IDs and certificate pins
must be unique. Dial addresses must be explicit IP socket addresses with a
nonzero port, not unspecified or multicast addresses. TLS server names are
validated separately. Relative paths resolve against the canonical configuration
file's parent, regardless of the shell's current directory.

Configuration version 1 rejects unknown fields. Input files must be regular
files: configuration and private key are limited to 64 KiB each; CA bundle and
local certificate chain to 1 MiB each. Certificates use PEM, with the local leaf
first; the key file contains exactly one supported PEM private key. Validation
checks CA trust, local server name, validity period, server/client authentication
usage, local fingerprint and certificate/key agreement. Operators supply and
protect these files; the CLI does not generate keys, enroll peers or contact them.
Use trusted directories and permissions appropriate for the service account.

- `check` validates configuration and local TLS material, then reports the local
  identity, voter IDs, resolved data path and shared `plan_sha256`. It does not
  create storage or prove remote connectivity.
- `provision` performs those checks, then explicitly creates a new bound store in
  an empty dedicated directory. Its parent must already exist. Directory creation
  is synced. Existing initialized, unbound or partially provisioned stores are
  rejected; there is no overwrite or implicit adoption. Provisioning never claims
  bootstrap permission. An I/O failure can leave a partial directory requiring
  inspection; do not assume a failed command made no files.
- `inspect` requires an existing store and obtains its normal exclusive lock.
  Stop the local node first. It validates expected identity, manifest, committed
  journal framing and selected snapshot integrity, then reports store UUID,
  bootstrap claim, retained entries, commit/purge/snapshot indexes and ignored
  journal tail bytes. It neither truncates the tail nor reclaims orphan files,
  creates missing files, syncs recovery state or changes the bootstrap claim.
  Invalid identity/corruption or an occupied lock fails the command. Snapshot
  contents are opaque: this is offline consensus integrity, not validation of
  application semantics, quorum, data freshness or public readiness. A store can
  be pristine yet have a consumed bootstrap claim; inspect both fields.

## Shared plan and explicit bootstrap

The shared digest is SHA-256 of `lithair-initial-group-v1` followed by a NUL byte,
cluster-ID byte length as little-endian u64, UTF-8 cluster ID, bootstrap node ID
as little-endian u64, then the three voter IDs (ascending, little-endian u64) and
their 32-byte certificate fingerprints. Local node ID, dial addresses and TLS
names are excluded. Thus every node can compare the same digest while retaining
its own local identity and routes. Identity validation precedes digest creation.

Wire version 2 requires this digest in every authenticated RPC envelope. A
mismatch is rejected before Raft dispatch. The private `/raft/v2/preflight` POST
accepts a null payload and returns the receiver's node ID, digest and initialized
state. It uses the same mTLS, enrollment, size and connection/time limits as Raft
RPCs. It does not mutate consensus state or expose bootstrap. Wire version 1 is
rejected; mixed v1/v2 operation is unsupported. This changes the private protocol,
not existing public model routes or the version-3 durable identity manifest.

The runtime's explicit local `PeerTransport::bootstrap(store, raft)` first checks
its local identity and designation, then contacts both other enrolled peers. All
three must agree and be uninitialized before it asks storage for the durable,
single-use bootstrap permission. An unavailable, initialized or differently
configured peer refuses this attempt before permission is consumed, allowing an
explicit retry after correcting that condition. This is not an atomic distributed
reservation: peers can fail after preflight. The durable claim retains the
[interrupted-bootstrap limitations](OPENRAFT_IDENTITY.md) once it is consumed.
Normal running operation still requires a 2/3 quorum, not all three nodes.

There is no CLI command or HTTP route to start an application or invoke bootstrap
in this milestone. The real-process fixture exercises the runtime action through
its private test control channel. Replacement membership, credential rotation,
public server integration and the dedicated authenticated deployment console
remain subsequent work; the offline tools do not establish three-VM readiness.

## Validation

`cidx run test` includes `cluster_operator_test`, the actual CLI subprocess tests
with `cluster-ops`, transport plan/preflight regressions, and the registered
`cluster_operator_bdd` runner. Coverage includes invalid/bounded input, relative
paths, lock/corruption refusal, nonmutating inspection, incompatible peer plans,
and retry after an unavailable peer followed by writes and cold restart.
`cidx run code` checks the optional targets; `cidx run build` also builds the
optional CLI in release mode. `cidx run ci` is the complete validation pipeline.
