# Deploying Lithair with Docker

Production-grade Docker artifacts ship at the repo root:

- [`Dockerfile`](../../Dockerfile) — multi-stage build, `rust:1.95.0-slim-bookworm`
  builder, `debian:bookworm-slim` runtime, non-root `lithair` user (UID 1000),
  healthcheck on `/health`.
- [`docker-compose.yml`](../../docker-compose.yml) — single-node example with
  a `./data` volume for event-store persistence.
- [`.dockerignore`](../../.dockerignore) — keeps build context lean while
  preserving `Cargo.lock` for reproducible builds.

The default image bakes in the `hello-world` example
(`examples/01-hello-world`). It's the canonical minimal server and ships the
built-in operations endpoints (`/health`, `/ready`, `/info`, `/metrics`).

## Quick start

```bash
docker compose up -d
curl -fsS http://localhost:8080/health     # → {"status":"healthy", ...}
curl -fsS http://localhost:8080/info       # → build/runtime info
docker compose logs -f lithair             # tail server logs
docker compose down                         # stop (data/ persists)
```

The image runs as UID 1000. Make sure `./data` on the host is writable by
that UID before the first `up`:

```bash
mkdir -p ./data
sudo chown -R 1000:1000 ./data   # only needed if your host user is not UID 1000
```

## Building with a different example

Any workspace crate under `examples/` with a `[[bin]]` of the same name
works. The Dockerfile passes the build arg through to `cargo build -p
<name> --bin <name>`.

```bash
docker build --build-arg LITHAIR_EXAMPLE=blog -t lithair:blog .

# Or via compose:
LITHAIR_EXAMPLE=blog docker compose build
docker compose up -d
```

The crate name must match a directory under `examples/` and the
`[[bin]] name` in that crate's `Cargo.toml`. As of v0.12.0 the available
canonical examples are `hello-world`, `static-site`, `rest-api`, `blog`,
`ecommerce`, `auth-sessions`, `auth-rbac-mfa`, `schema-migration`,
`replication`, and `blog-distributed`.

## Volume and data persistence

Lithair is event-sourced. Every state change is appended to a log under the
data directory; on startup the engine replays those events to reconstruct
the in-memory state. Losing this directory means losing the database.

```text
/app/data/
├── events.log            # appended event stream (per-store)
├── snapshots/            # periodic snapshots for fast replay
└── raft/                 # cluster-mode WAL + Raft log (cluster examples only)
```

The compose file mounts `./data:/app/data`. Treat that host directory like
you would any database storage:

- **Back it up** before upgrades. See `docs/operations/backup-restore.md`
  (TODO: doc to land in a follow-up PR per issue #106).
- **Don't share it across containers** unless you also coordinate
  Lithair's clustering primitives — the event log is not designed for
  concurrent writers from multiple processes.
- **Snapshot it on a schedule** if you don't already snapshot the host
  filesystem.

## Host bind caveat

The bundled examples call `.with_host("127.0.0.1")` in their `main.rs`,
which binds the listener to the container's loopback interface. With the
default Docker bridge network and a `8080:8080` port mapping, that
listener is unreachable from the host — the kernel can't route a host
packet to a container's loopback.

The compose file works around this by using `network_mode: host`. The
container shares the host's network namespace, so 127.0.0.1 inside the
container *is* the host's loopback. `curl http://localhost:8080/health`
from the host then works as expected.

**Alternative**: rebuild with an example that binds to `0.0.0.0` (e.g.
`examples/07-auth-rbac-mfa` accepts `--host 0.0.0.0` via CLI flag), drop
`network_mode: host`, and use the commented `ports:` block in
`docker-compose.yml`. This is the right shape for multi-host production
deployments where host networking is undesirable.

A future change to make `LT_HOST` override the builder's `with_host()` is
tracked alongside the broader operational hardening work (issue #106).

## Resource sizing

For per-model RAM/disk/CPU estimates, see
`docs/operations/capacity-planning.md` (TODO: doc to land in a follow-up
PR per issue #106). Until then, the rough baseline:

- Idle hello-world: ~15 MiB RSS, negligible CPU.
- Each declarative model with N items at S bytes average uses roughly
  `N * (S + overhead)` of RAM, where overhead is ~200 B for the in-memory
  index plus event-sourcing metadata. Disk usage is dominated by the
  event log, which is append-only; size it for the lifetime write volume,
  not the live working set.

## Common gotchas

**Permission denied writing to `/app/data`.** The container user is UID
1000. If the host `./data` is owned by root (e.g. created by a prior
docker run as root), `chown -R 1000:1000 ./data` fixes it.

**`curl: (7) Failed to connect`** from the host with a bridge network.
You're hitting the localhost-bind issue above. Either keep
`network_mode: host`, or rebuild with a 0.0.0.0-binding example.

**`cargo build` fails with `error: failed to download` in CI.** The
Dockerfile uses `--locked`. If `Cargo.lock` was not copied into the build
context, the build will refuse to resolve fresh versions. Confirm that
`.dockerignore` does *not* list `Cargo.lock`.

**Image rebuild is slow on every change.** Expected — the multi-stage
build does not currently use `cargo-chef` for dependency caching. For
local iteration, prefer `cargo run -p hello-world` directly; the
container image is for deployment, not dev loops.

**Healthcheck stuck `starting` for >30s.** The `start_period` is 5s and
each probe runs every 10s. If the server takes longer than ~35s to bind
(e.g. large cluster replay), bump `start_period` in `docker-compose.yml`.
