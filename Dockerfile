# syntax=docker/dockerfile:1.7
#
# Lithair production Dockerfile (multi-stage, glibc runtime).
#
# Build stage uses the same Rust toolchain pinned by rust-toolchain.toml so
# local builds and CI stay in lockstep. Runtime stage is a minimal Debian
# image — no Rust toolchain, no source — running as a non-root user.
#
# Default target binary is the `hello-world` example (the canonical "minimal
# Lithair server" used in the getting-started guide). Override with
# `--build-arg LITHAIR_EXAMPLE=<crate>` to bake a different example into the
# image, e.g.:
#
#   docker build --build-arg LITHAIR_EXAMPLE=blog -t lithair:blog .
#
# The crate name must match a `[[bin]]` name under `examples/` (see
# `examples/<dir>/Cargo.toml`). Defaults below match `examples/01-hello-world`.
#
# Persistent state: the event store writes under /app/data. Mount a volume
# there (see docker-compose.yml) so events survive container restarts.

# ───── Build stage ────────────────────────────────────────────────────────────
FROM rust:1.95.0-slim-bookworm AS builder

ARG LITHAIR_EXAMPLE=hello-world

WORKDIR /build

# System deps required to compile lithair-core's transitive native crates
# (aws-lc-sys / ring need a C toolchain + cmake; perl is pulled in by some
# build scripts). Keep this list minimal — runtime image does not need them.
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        build-essential \
        cmake \
        perl \
        pkg-config \
 && rm -rf /var/lib/apt/lists/*

# Copy the whole workspace. We do not split deps/sources for layer caching
# here because Cargo's workspace layout makes that fragile (any Cargo.toml
# touch invalidates the whole dep layer anyway). A future optimization could
# use cargo-chef; for now we trade build time for simplicity.
COPY . .

# Build the requested example in release mode against the workspace.
# `--locked` enforces Cargo.lock fidelity (reproducible builds).
RUN cargo build --release --locked -p "${LITHAIR_EXAMPLE}" --bin "${LITHAIR_EXAMPLE}"

# Stage the binary at a known path so the runtime stage doesn't need to know
# the chosen crate name.
RUN cp "/build/target/release/${LITHAIR_EXAMPLE}" /build/lithair-app

# ───── Runtime stage ──────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

# `curl` is used by the HEALTHCHECK and is small enough to justify in the
# runtime image. `ca-certificates` is required for outbound HTTPS (cluster
# replication, S3 backups). Keep this list lean.
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
 && rm -rf /var/lib/apt/lists/*

# Non-root user. UID 1000 is the conventional first non-system user on
# Debian-derived images and matches what most host filesystems use, so the
# bind-mounted ./data directory in docker-compose.yml is writable without
# extra chown gymnastics on a typical Linux host.
RUN groupadd --system --gid 1000 lithair \
 && useradd --system --uid 1000 --gid lithair --home-dir /app --shell /usr/sbin/nologin lithair

WORKDIR /app

# Persistent event-store directory. Mount a host volume here in production
# (see docker-compose.yml's `./data:/app/data` mapping). The container will
# create subdirectories as needed; the directory itself must be writable by
# UID 1000.
RUN mkdir -p /app/data && chown -R lithair:lithair /app

COPY --from=builder --chown=lithair:lithair /build/lithair-app /app/lithair

USER lithair

# Default Lithair port. Override the bound port via PORT env (the example
# reads it on startup). Examples currently bind to 127.0.0.1 inside the
# container; see docs/operations/deployment-docker.md for how compose
# exposes that to the host.
EXPOSE 8080

# Healthcheck hits the built-in /health endpoint registered by LithairServer.
# This runs INSIDE the container, so 127.0.0.1 reaches the example regardless
# of its bind address.
HEALTHCHECK --interval=10s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -fsS http://localhost:8080/health || exit 1

ENTRYPOINT ["/app/lithair"]
