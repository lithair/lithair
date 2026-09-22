# Durable peer leaf certificate rotation

Issue [#263](https://github.com/lithair/lithair/issues/263), part of
[RFC 248](../../rfcs/248-three-node-cluster.md). This is the private consensus
foundation and experimental offline operator CLI. It does not enable production
native/Turso/session replication or provide a live administration endpoint.

## Identity and storage contract

The initial three voter IDs, their original fingerprints, cluster ID and designated
bootstrap node remain immutable. `plan_sha256` and wire version 2 remain unchanged.
In particular, **do not replace `peers[].certificate_sha256` with renewed pins**:
those fields describe genesis, even after all original certificates have expired.
They are public fingerprints, not a requirement to keep old private keys online.

A separate `CredentialPolicy` contains an unsigned 64-bit generation, the current
pin for each of those same three IDs, and at most one pending node/pin pair.
Generation zero is exactly the initial enrollment. Odd generations accept both
current and pending pins for one peer; even generations have one pin per peer.
Pins must be distinct across identities, including the pending pin.

Only adjacent transitions are accepted:

1. Stable -> overlap: preserve every current pin and stage one new pin.
2. Overlap -> stable: replace that peer's current pin with its pending pin.

A caller supplies the exact expected generation. There is no skip, rollback,
abort-to-old-policy or silent retry. Repeating a completed transition fails stale;
inspect the durable state before deciding what to do next. A later deliberate
operator enrollment of a previously used fingerprint is not prevented by a
permanent revocation history. Use freshly generated keys for renewals.

`DurableLog::transition_credentials` is offline: it must obtain the same exclusive
lock as the running node. It validates identity, policy, committed journal and
snapshot integrity without truncating tails or reclaiming orphans. Only then does
it sync/rename a new checksummed manifest and sync the directory. An error after
publication can mean the update took effect. The owned blocking task completes
its admitted filesystem operation even if the caller cancels its future.

Existing version-3 stores remain version 3 on reads and ordinary recovery. Their
implicit policy is generation zero. The first explicit rotation upgrades only
the manifest to version 4, embedding `credentials` alongside the unchanged node
binding, bootstrap claim, store UUID and journal/snapshot references. Subsequent
writes and compaction preserve it. Older binaries reject version 4; binary rollback
to a version-3-only implementation is unsupported after this step. Unbound stores
are never adopted. Rotation neither initializes Raft nor changes its membership.

Normal startup uses `open_with_credentials` and an exactly matching configured
policy. A mismatch fails before journal repair/cleanup. The legacy `open_for_node`
requires generation zero. Offline inspection can use a stale configured policy to
report the actual durable policy; it still requires the original identity and
valid local TLS files. `configured_credential_generation` and
`store.credentials.generation` distinguish those observations.

## Operator configuration

Version 1 still means generation zero and forbids a `credentials` table. Version 2
requires the table. Keep the existing `peers` entries and append this to each node's
configuration when staging node 2 (replace every placeholder with 64 hex digits):

```toml
# At the top of the file, replace version = 1 with version = 2.
[credentials]
generation = 1

[credentials.current]
"1" = "NODE_1_CURRENT_SHA256"
"2" = "NODE_2_CURRENT_SHA256"
"3" = "NODE_3_CURRENT_SHA256"

[credentials.pending]
node_id = 2
certificate_sha256 = "NODE_2_RENEWED_SHA256"
```

For generation 2, change `credentials.current."2"` to the renewed pin and remove
`credentials.pending`. Later rotations use generations 3/4, 5/6, etc. All policy
fields reject unknown input. Existing configuration size limits still apply.
The public policy report contains fingerprints only; keys stay in protected PEM
files outside consensus storage.

With the **local node stopped**, apply the prepared configuration:

```bash
lithair cluster check --config /etc/lithair/node-overlap.toml
lithair cluster update-credentials --config /etc/lithair/node-overlap.toml --expected-generation 0
lithair cluster inspect --config /etc/lithair/node-overlap.toml
```

`update-credentials` prints a JSON success report or exits 2 with an error. It does
not change the configuration file, certificate files or process supervisor. The
runtime must subsequently load that exact policy. `provision` only accepts
generation zero; it is never a rotation or replacement operation.

## Rolling procedure and interrupted actions

This procedure renews **one existing peer's leaf certificate under the same CA**,
with a new private key, the same TLS name, and valid client/server usages. It is
not a CA rotation or emergency key-compromise protocol: both keys remain valid
during overlap. Validate CA, usages, name, expiry and key pairing before beginning.
Distribute only each node's own key(s); other nodes need public pins and CA trust.

1. Confirm all three nodes are healthy and caught up, and that the deployed
   binaries understand manifest version 4. Record the shared initial plan and
   current durable generation. Prepare the same next policy on all three hosts.
2. One node at a time, stop it, apply the overlap transition with the expected
   generation, select that configuration for startup, restart it with its current
   certificate, and wait for recovery. Verify acknowledged writes on the other two
   and full convergence before stopping the next node. Do not switch certificates
   until all three durably accept both pins.
3. Stop the renewing node, replace its certificate/key paths with the new pair,
   and restart under the overlap policy. Wait for authenticated replication and
   convergence. The other two nodes retain quorum while it is stopped.
4. One node at a time, stop it, apply the next generation removing the old pin,
   restart, and verify recovery. Retirement is complete only once **every** node
   has durably installed the stable new policy and restarted. Until then a peer
   still on overlap can accept the old certificate.
5. Verify old client and server credentials are rejected, record the final
   generation, and remove the retired key from the normal deployment secrets.
   Retain the original public genesis fingerprints.

The CLI does not contact other nodes and cannot prove these quorum/recovery
conditions. The operator must enforce this ordering; automatically coordinated
rotation and readiness gates are future work. Losing a second node during
maintenance denies authoritative writes. Do not force a transition or reset
bootstrap to repair that loss of quorum.

After a timeout, cancellation or process/host failure, stop/reconcile the local
process and inspect the durable manifest. If the old generation remains, the same
explicit transition may be retried. If the next generation is present, select its
matching configuration and resume the procedure; do not repeat with a new expected
generation against the same policy. If config and manifest disagree, recovery
fails closed. There is no automatic manifest downgrade or configuration adoption.
Inspection cannot salvage corruption and must never be replaced by editing the
manifest manually.

## Executable evidence

`cidx run test` includes Rust regressions for adjacent transitions, stale and
invalid policies without disk mutation, exclusive locking and competing operators,
identity/vote/commit/bootstrap preservation, compaction/restart, and process death
and I/O faults around manifest publication. The registered `cluster_operator_bdd`
feature exercises offline rotation and stale-command rejection. CLI subprocess
tests check required arguments, structured success and stale retry exit status.

`cidx run cluster` uses Probatum and three isolated Compose nodes. It stages each
policy with real node stops and majority writes, deliberately removes quorum,
switches a same-CA leaf/key, retires its old pin, probes retired client and server
identities, and cold-restarts all three nodes three times. Every acknowledged
mutation must survive. Artifacts include `rotation.json`, the retired-server probe,
all final durable policies and the usual Probatum verdict/logs.

Membership replacement with new learner IDs/joint consensus, production state
machines, live administration and qualification on three independent VMs remain
separate work.
