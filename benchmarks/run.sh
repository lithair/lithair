#!/usr/bin/env bash
# Lithair v1.0 gate G4 — reproducible CRUD benchmark: Lithair vs a conventional
# baseline (Axum + SQLite), driven by the SAME load harness (tools/loadgen,
# which already reports throughput + p50/p95/p99). Write-heavy mix (the workload
# where event-sourcing vs SQL actually differs); one command, both servers.
#
#   ./benchmarks/run.sh            # defaults
#   BENCH_TOTAL=50000 TIERS="64 256 1024" ./benchmarks/run.sh
#
# ponytail: single dimension (CRUD throughput + latency, write-heavy) vs one
# baseline. Memory-growth, cold-start-replay and Actix+Postgres are named as
# deferred in docs/performance/baselines.md — add when someone needs them.
set -euo pipefail
cd "$(dirname "$0")/.."

TOTAL="${BENCH_TOTAL:-20000}"
TIERS="${TIERS:-32 128 512}"
CREATE_PCT="${CREATE_PCT:-85}"
READ_PCT="${READ_PCT:-15}"
LITHAIR_PORT=18380
BASELINE_PORT=18390

echo "== building (release) =="
cargo build --release -p loadgen -q
cargo build --release -p replication --bin lithair-cluster-node -q
cargo build --release --manifest-path benchmarks/baseline-axum-sqlite/Cargo.toml -q
LOADGEN=target/release/loadgen
BASELINE_BIN=benchmarks/baseline-axum-sqlite/target/release/baseline-axum-sqlite

SRV_PID=""
WORK=""
cleanup() { [ -n "$SRV_PID" ] && kill "$SRV_PID" 2>/dev/null || true; [ -n "$WORK" ] && rm -rf "$WORK" 2>/dev/null || true; }
trap cleanup EXIT

wait_health() {
  for _ in $(seq 1 50); do
    curl -fsS "http://127.0.0.1:$1/health" >/dev/null 2>&1 && return 0
    sleep 0.2
  done
  echo "server on $1 failed to start" >&2; exit 1
}

# bench_target <label> <port> <start-cmd...>
bench_target() {
  local label="$1" port="$2"; shift 2
  WORK="$(mktemp -d)"
  ( cd "$WORK" && "$@" ) >/dev/null 2>&1 &
  SRV_PID=$!
  wait_health "$port"
  local url="http://127.0.0.1:$port"
  # warmup (not measured)
  "$LOADGEN" --leader "$url" --total 2000 --concurrency 64 --mode random \
    --create-pct "$CREATE_PCT" --read-pct "$READ_PCT" --update-pct 0 --delete-pct 0 \
    --read-path /api/products >/dev/null 2>&1 || true
  for c in $TIERS; do
    echo "---- $label | concurrency=$c | total=$TOTAL | mix=${CREATE_PCT}C/${READ_PCT}R ----"
    "$LOADGEN" --leader "$url" --total "$TOTAL" --concurrency "$c" --mode random \
      --create-pct "$CREATE_PCT" --read-pct "$READ_PCT" --update-pct 0 --delete-pct 0 \
      --read-path /api/products 2>&1 | grep -E "p50=|throughput=" || true
  done
  kill "$SRV_PID" 2>/dev/null || true
  wait "$SRV_PID" 2>/dev/null || true  # reap before rm so no file handles linger
  SRV_PID=""
  rm -rf "$WORK"; WORK=""
}

echo "== Lithair (single node, event-sourced) =="
bench_target "lithair" "$LITHAIR_PORT" \
  "$PWD/target/release/lithair-cluster-node" --node-id 0 --port "$LITHAIR_PORT"

echo "== Baseline (Axum + SQLite, file-backed, WAL+NORMAL) =="
BASELINE_PORT="$BASELINE_PORT" bench_target "axum-sqlite" "$BASELINE_PORT" "$PWD/$BASELINE_BIN"

echo "== done — record numbers in docs/performance/baselines.md =="
