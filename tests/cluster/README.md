# Three-container consensus qualification

Run from the repository root:

```sh
cidx run cluster
# Full validation, including the same cluster phase:
cidx run ci
```

Requires a local Linux Docker engine with Compose support and an amd64 runtime.
The cidx tool containers supply Rust 1.97.1, Docker/Compose, Python and Probatum
0.10.0 from digest-pinned images. No host Cargo, Python or Probatum installation
is needed. Initial image pulls require registry access. The Docker client needs
access to `/var/run/docker.sock`; this is a trusted test runner with authority to
create and stop this run's resources. The nodes receive no Docker socket.

## What runs

The `cluster-build` cidx container compiles `cluster-compose-node`, an explicit
Cargo example under `lithair-core/tests/support/`. This test executable imports
the same private durable log and mTLS transport as the Rust integration tests,
plus their snapshot-backed MemStore application. It is not a supported server
builder, an admin API or a deployment template. Normal applications never install
its `/test/*` control routes.

The `cluster-probatum` container builds disposable node/driver images and creates
a unique `lithair-cluster-<random>` Compose project. `prepare` generates a temporary
CA and separate node keys, explicitly provisions three bound stores, and exits.
Each node receives only its own private key, mounted read-only, and its own data
volume. Node restart calls recovery only. No keys or stores are copied into run
evidence or committed to the repository.

The nodes attach to two internal bridge networks. Each peer listener binds only
to its replication address; each test HTTP listener binds only to its control
address. The Probatum driver joins control only. No service publishes host ports.
The driver can control Docker lifecycle/network faults through the socket; plane
isolation assertions test ordinary network access, not containment of that trusted
operator. Separate Compose projects can run without fixed host ports/subnets. The runner
asks Docker for a free subnet and reserves it explicitly (retrying allocation
conflicts). This permits restoring the original peer IP on Docker engines that
reject static addresses in automatically configured networks.
All three replication addresses are fixed within that run's reserved subnet;
parallel cold starts cannot allocate another stopped node's address.

Probatum owns the ordered checks in `probatum.toml`; `checks.py` implements HTTP
assertions and Docker actions. It uses deadline polling, bounded subprocesses and
request timeouts. Assertions verify:

1. Fresh nodes answer with distinct IDs, empty state and no initialization.
2. Data/credential volumes are distinct; peer RPCs are inaccessible on the control
   interface; the real peer listener rejects a TCP client without a certificate.
3. Missing node 3 refuses bootstrap, then an explicit retry succeeds on its return.
4. A SIGKILL of the current leader is observed as exit 137; the two survivors
   acknowledge new writes and the returning node converges.
5. Disconnecting the leader's replication interface leaves test control reachable.
   The minority rejects writes and a linearizable barrier while the majority
   accepts writes. Healing restores its original enrolled IP and convergence.
6. A killed follower stays behind while the leader snapshots and purges beyond
   its last applied index; its return must actually install a remote snapshot.
7. All three containers are killed and cold-started three times using their existing volumes.
   Every acknowledged mutation is recovered and repeated bootstrap is rejected.
8. A nested Probatum run with a deliberately wrong expected response must exit 1
   and emit valid JSON, proving assertions do not silently pass.
9. Offline inspection succeeds on each stopped, identity-bound store.

A timed-out/rejected write has an uncertain outcome; the suite tracks successful
acknowledgements and does not incorrectly assert that rejected writes never commit.
Every convergence check also compares all replicas, not just one chosen key.

## Evidence and failure cleanup

Each run writes `.probatum/runs/lithair-cluster-<random>/` with the Probatum verdict,
its frozen checks and command evidence, acknowledged keys, offline inspection
reports, container logs and cleanup log. GitHub's Cluster job uploads that exact
path with `if: always()`. Secret volumes are never included. The directory is
ignored by Git.

The outer runner traps normal success/failure and interrupt/termination. It
collects evidence before removing only its own containers, networks, volumes and
image tags, including the externally managed replication network. Cleanup failure
makes the gate fail. No global prune, shared container
name or fixed host port is used. A killed host, lost Docker daemon or SIGKILL of
the outer runner cannot execute a shell trap; use the recorded project name for
manual cleanup after restoring Docker:

```sh
docker compose -p lithair-cluster-REPLACE_WITH_RUN_ID -f tests/cluster/compose.yml down -v --remove-orphans
```

That manual command also needs the image environment variables recorded in
`project.env` in the evidence directory; source that file first. Remove the
external replication network afterwards with
`docker network rm "$COMPOSE_PROJECT_NAME-replication"` (and the temporary
`"$COMPOSE_PROJECT_NAME-subnet-probe"` if interruption occurred during allocation).

To test the entire failing-gate cleanup path deliberately:

```sh
LITHAIR_CLUSTER_INJECT_FAILURE=1 cidx run cluster
```

Expect a failed cidx phase, a failed Probatum verdict, preserved evidence and no
remaining resources for that project. The ordinary CI run includes the nested
negative assertion without making the overall run fail.

## Limits

The three containers share one host/kernel. They prove real socket, TLS,
container-crash, disk-reopen and network-partition behavior with the test state
machine. They do not prove independent-VM failure tolerance, power-loss semantics,
replacement membership, certificate rotation, native/Turso/session replication,
application admission or rolling production upgrades. These remain the follow-up
milestones in [RFC 248](../../docs/rfcs/248-three-node-cluster.md).
