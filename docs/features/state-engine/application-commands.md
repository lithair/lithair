# Atomic application commands (experimental)

`cluster::native::commands::CommandStore` is an opt-in trusted Rust API behind
`cluster,tls`. It shares the native consensus transport, durable log, checkpoint
publication and admission machinery. It has no generated HTTP routes and does
not use model CRUD or expose an unauthenticated command endpoint.

Declare an explicit application contract version and collection names, provision
three operator stores, open one store per node, then explicitly bootstrap on the
designated node. `open_with_listener` accepts an already reserved peer socket.
There is no automatic provisioning, bootstrap, schema migration or model/command
store conversion. Contract hashes distinguish command stores from model stores.

```rust,ignore
use lithair_core::cluster::native::commands::{CommandStore, Decision, Write};
let store = CommandStore::open("node.toml", "orders-v1",
    &["orders", "operations", "events", "outbox", "receipts", "policy"]).await?;
// Bootstrap is a separate, explicit operator action, never a request action.
```

## Decision and commit

`execute` takes an owned synchronous callback. Lithair serializes admission on
the leader and establishes a fresh quorum/apply barrier before passing a borrowed
`View` of committed collections. The application must authenticate before calling
this API, and evaluate current tenant/principal permissions inside the callback,
**before** looking up an existing receipt. `View` is trusted storage access, not
an authorized query interface; callers must filter reads and lists themselves.

A decision is either `Read(reply)` (no log write, including authorized receipt
replay) or `Commit { writes, reply }`. `Write::put` and `Write::delete` address
individual keys in declared collections. The complete batch plus reply becomes
one canonical consensus command with an expected group revision. Committed apply
only interprets data; it does not invoke application callbacks. A conflicting
revision refuses all business changes. Callbacks must be deterministic, short
and free of network, filesystem or other external effects.

For example, an application can return five writes for an order change, an
operation, its event, an outbox entry and a receipt in the same commit. An outbox
publisher reads committed entries, delivers them, then acknowledges through a
new application command. Lithair does not provide that publisher or promise
exactly-once network delivery. Consumers must tolerate redelivery.

Use a durable receipt scoped by tenant + principal + command kind + business key,
with a fingerprint of normalized arguments, target and expected aggregate
revision. The application owns retention/deletion. These records survive native
log compaction and the independent 256-entry technical response cache; no
implicit receipt TTL or eviction applies. `View::revision()` is the group
revision, not the version of an individual business aggregate.

`read` uses the same quorum barrier and returns the callback's result without
writing. Followers, uninitialized stores and isolated leaders refuse consistent
reads and commands. `inspect` is local diagnostic information only.

## Failure, capacity and compatibility

Admission has 128 slots, a three-second caller deadline and a six-second task
deadline. Once admitted, the task can continue after its caller is cancelled.
Timeout, leadership loss, cancellation and storage errors can leave an **unknown
commit outcome**; retry the same business key and payload through the callback,
including authorization. A callback panic becomes a task error without stopping
the runtime. Synchronous callbacks cannot be preempted: they must not block.

Batches must be nonempty, reference declared collections, and address each
collection/key pair at most once. Keys must be nonempty and at most 1024 bytes;
collection names are distinct ASCII letters/digits/`._-`, at most 128 bytes.
Canonical commands including replies are limited to 256 KiB. Checkpoints remain
limited to 48 MiB, with 64 KiB reserved during admission. Invalid and oversized
batches are refused before proposal. Capacity is bounded; applications must plan
journal/receipt retention and handle refusal. This implementation clones and
serializes the candidate state during admission and publishes a complete durable
checkpoint on apply; it is not a large-dataset storage strategy.

The model-store contract and old model checkpoints remain unchanged. Command
stores use a distinct application hash. Application schema changes require a
new explicit version and a future migration facility; opening an existing store
with a different contract is refused. Operational export/restore to fresh
identities, schema migration and disaster-recovery fencing are not implemented
by this API. Cold restart and follower snapshot installation retain the original
cluster identities and are not a substitute for those operations.

## Executed qualification entry points

- `cidx run code`: format and Clippy, including cluster/TLS and executable fixtures.
- `cidx run test`: canonical batch invariants; seven subprocess crash boundaries
  during checkpoint publication; three-node integration and executable native
  Gherkin coverage for atomic state/events/outbox/receipts, 265 later commands,
  current policy before replay, conflicting aggregate revisions, invalid/oversized
  decisions, callback panic, snapshot catch-up, failover, quorum loss and restart.
- `cidx run cluster`: Probatum drives a trusted sample application over isolated
  Docker processes, volumes and replication network. It tests atomic changes,
  concurrent revisions, policy revocation before replay, receipt retention,
  snapshot catch-up, discarded HTTP responses, leader SIGKILL, minority refusal,
  full cold restart and converged collection digests.

The sample in `lithair-core/tests/support/command_example.rs` is deliberately a
trusted test application, not production identity middleware. The Compose
control endpoints do not ship as framework routes. Production OIDC, application
HTTP identity isolation, outbox delivery/ack recovery and backup restore need
application-level qualification before opening business writes.
