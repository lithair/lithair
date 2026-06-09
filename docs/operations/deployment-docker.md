# Deploying Lithair with Docker

Production-grade Docker artifacts ship at the repo root:

- [`Dockerfile`](../../Dockerfile) — multi-stage build, `rust:1.95.0-slim-bookworm`
  builder, `debian:bookworm-slim` runtime, non-root `lithair` user (UID 1000),
  healthcheck on `/health`.
- [`docker-compose.yml`](../../docker-compose.yml) — single-node example using
  bridge networking and a named `lithair-data` volume for event-store
  persistence.
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
docker compose down                         # stop (lithair-data persists)
```

The compose file uses bridge networking with an `8080:8080` port mapping and
sets `HOST=0.0.0.0` so the containerized server binds all interfaces and the
published port is reachable from the host. This is cross-platform (Linux,
macOS, Windows/Docker Desktop). The named `lithair-data` volume is created
with the correct ownership automatically, so no host `chown` is needed.

## Configurable binding (HOST / PORT)

The `hello-world` example reads its listen address from two env vars:

| Var    | Default     | Notes                                         |
|--------|-------------|-----------------------------------------------|
| `HOST` | `127.0.0.1` | Set `0.0.0.0` to bind all interfaces.         |
| `PORT` | `8080`      | TCP port to listen on.                        |

For local runs the defaults bind loopback only. The compose file overrides
`HOST=0.0.0.0` (and `PORT=8080`) in its `environment:` block so the bridge
port mapping works. Binding `0.0.0.0` is safe inside a container: it is
scoped to the container's isolated network namespace, not the host.

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

The compose file mounts the named volume `lithair-data` at `/app/data`. It is
created with the image mountpoint's ownership (`lithair:lithair`, UID 1000),
so it sidesteps the root-owned bind-mount permission problem. Find its
on-disk location with:

```bash
docker volume inspect lithair-data
```

Treat that volume like you would any database storage:

- **Back it up** before upgrades. See
  [`docs/operations/backup-restore.md`](backup-restore.md) for the
  backup/restore/PITR runbook.
- **Don't share it across containers** unless you also coordinate
  Lithair's clustering primitives — the event log is not designed for
  concurrent writers from multiple processes.
- **Snapshot it on a schedule** if you don't already snapshot the host
  filesystem.

**Bind-mount alternative.** If you want the data visible on the host
filesystem, swap the named volume for a bind mount (`./data:/app/data`, the
commented line in `docker-compose.yml`). Caveat: the host directory must be
writable by UID 1000, e.g. `mkdir -p ./data && sudo chown -R 1000:1000
./data`.

## Other examples

Only `hello-world` currently reads `HOST` from the env; the other bundled
examples still hardcode `127.0.0.1` in their `main.rs`. The `HOST`/`PORT`
pattern shown in `examples/01-hello-world` is the recommended approach — any
example can adopt it the same way to work cleanly under bridge networking.

## Resource sizing

For per-model RAM/disk/CPU estimates, see
[`docs/operations/capacity-planning.md`](capacity-planning.md). Rough
baseline:

- Idle hello-world: ~15 MiB RSS, negligible CPU.
- Each declarative model with N items at S bytes average uses roughly
  `N * (S + overhead)` of RAM, where overhead is ~200 B for the in-memory
  index plus event-sourcing metadata. Disk usage is dominated by the
  event log, which is append-only; size it for the lifetime write volume,
  not the live working set.

## Common gotchas

**Permission denied writing to `/app/data`.** Only happens with the
bind-mount alternative: the container user is UID 1000, so a root-owned host
`./data` needs `chown -R 1000:1000 ./data`. The default named volume avoids
this.

**`curl: (7) Failed to connect`** from the host. Confirm the container is
binding `0.0.0.0` — the compose file sets `HOST=0.0.0.0`. If you run the
container directly, pass `-e HOST=0.0.0.0`; the example defaults to
`127.0.0.1`, which a bridge port mapping cannot reach.

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
