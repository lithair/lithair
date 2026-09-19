# Private OpenRaft peer transport

Part of [#248](https://github.com/lithair/lithair/issues/248), implemented by
[#253](https://github.com/lithair/lithair/issues/253). This is a private foundation
behind `cluster` **and** `tls`, not an alternative application builder. The current
`with_raft_cluster()` path, SQL cluster startup guard and local sessions remain
unchanged. See [RFC 248](../../rfcs/248-three-node-cluster.md).

## Identity and protocol

The caller supplies a cluster ID (1–128 bytes), stable local node ID, an explicit
map of enrolled node IDs to socket addresses, TLS server names and SHA-256 leaf
certificate fingerprints, a private CA trust store, and a local certificate/key.
A node cannot share its enrolled certificate with another ID. Addresses can be
remote VM IPs; TLS server names are verified independently of the dial address.
There is no DNS discovery or automatic enrollment.

`PeerTransport` implements OpenRaft's `RaftNetworkFactory`/`RaftNetwork`. Its
separate `serve(TcpListener, Raft, shutdown)` listener accepts HTTPS only, with
mandatory client certificates validated by rustls. CA validation and certificate
pinning are both required: a CA-signed but unenrolled certificate is rejected.
The client also verifies the server name and expected target certificate. Trust
is explicit; neither the system root store nor ambient HTTP proxies are used.

One HTTP/1.1 POST per TLS connection carries JSON to one of:

- `/raft/v1/vote`: RequestVote;
- `/raft/v1/append`: AppendEntries;
- `/raft/v1/snapshot`: OpenRaft's chunked InstallSnapshot.

The envelope has `version`, `cluster`, `sender`, `recipient` and `payload`.
Unknown envelope fields, invalid JSON, unsupported versions, wrong clusters and
wrong recipients are rejected before Raft is called. The sender must match the
verified client certificate and the candidate/leader identity in the Raft vote.
Replies preserve OpenRaft's typed success/error result. HTTP rejection, TLS,
serialization, timeout and socket errors become transport errors, never votes
or successful acknowledgements. There are no bootstrap, write-forwarding,
public model or readiness endpoints on this listener.

Enrollment is immutable for the lifetime of a transport. The caller must supply
the intended fixed group; OpenRaft owns voting membership and quorum decisions.
`C::Node` metadata does not override enrolled addresses or certificates. Dynamic
membership, credential rotation and persistent node/cluster identity manifests
must be implemented and qualified before exposing a supported deployment API.
Reusing a durable directory with a different identity is not made safe by TLS.

## Bounds and shutdown

Requests and responses are limited to 1 MiB of encoded JSON. Serialization uses
a bounded writer, and HTTP bodies are collected through a size limiter, including
chunked bodies. Keep snapshot chunks at or below 64 KiB: JSON byte arrays expand
binary data. Configure append batches to fit the same bound; this implementation
returns an error for oversized messages instead of silently splitting commands.
The future application state machine must also bound total snapshot size.

There are at most 32 accepted connections per listener and 32 active outbound
exchanges per shared transport. An excess incoming connection is closed. The
whole incoming connection, including TLS handshake, request and Raft dispatch,
has a five-second deadline. The outbound deadline includes admission waiting,
connect, TLS and response collection and is the smaller of five seconds and
OpenRaft's RPC deadline. Connections do not remain alive for another request.
Caller cancellation drops the outbound socket; listener shutdown cancels and
joins its owned connection tasks. A canceled request can already have reached
Raft: transport failure is not proof that a command was never committed.

## Executable evidence

`cidx run test` explicitly enables `cluster,tls` for
`lithair-core/tests/openraft_consensus_test.rs`, alongside the storage target.
`cidx run code` also checks these targets and the Gherkin runner with clippy.
Transport regressions exercise valid vote/append/snapshot-response paths, remote
Raft errors, missing/untrusted/unenrolled client certificates, server-name and
fingerprint mismatch, wrong envelopes and payload limits.

The shared fixture in `lithair-core/tests/support/openraft_processes.rs` starts
three **child processes**, each with its own durable log directory and test-only
OpenRaft MemStore state machine. It explicitly creates stores and initializes the
three-member configuration once. Recovery calls `open()` and never initializes
because peers are absent. Snapshots and log purge are disabled in this fixture;
all application state is reconstructed from the retained committed log.

Six directed TCP relays reserve ephemeral listening sockets and forward real TLS
traffic. Partitioning disables selected relays and drops their established TCP
connections. Restart binds a fresh reserved listener and updates only the relay's
destination; Raft peer identities and persisted membership stay the same.
Tests poll state with deadlines and check:

- acknowledged commands survive repeated leader kills and the old leader's return;
- all three processes can cold-restart and recover without reinitialization;
- an isolated former leader cannot acknowledge writes or a consistent read barrier;
- the remaining majority can write and all replicas converge after healing.

`cucumber-tests/tests/openraft_consensus_bdd.rs` uses the same real-process
fixture for the three scenarios in `features/core/openraft_consensus.feature`.
It is registered with `no_orphan_features` and executes in the cidx PR gate.
Certificates are generated per test; no private keys are committed.

## Remaining work

This establishes consensus with a test state machine, not durable application
snapshots or integration with native models, Turso, sessions and authorization.
The snapshot RPC response test does not qualify crash-safe snapshot installation.
Physical journal compaction, snapshot recovery, explicit operator bootstrap,
persisted identity checks, replacement membership and certificate rotation remain
milestone 2 follow-ups. HTTP contracts, read admission, request deduplication,
application checkpoints, rolling deployment and qualification on three independent
VMs remain later milestones. No production availability or throughput claim follows
from these loopback correctness tests.
