#!/usr/bin/env bash
set -euo pipefail

# Frontend Benchmark Demo Runner (Phase A)
# - Serves static frontend via RS_STATIC_DIR
# - Uses http_firewall_declarative API for /api/products
# - Verifies index, assets, status, and CRUD basic flow

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROJECT_ROOT="$(cd "$ROOT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

PORT=${PORT:-18090}
BASE="http://127.0.0.1:${PORT}"
STATUS_URL="$BASE/health"
API="$BASE/api/products"
STATIC_DIR="$ROOT_DIR/frontend_benchmark/dist"
LOG_FILE="$ROOT_DIR/frontend_demo.log"
KEEP_ALIVE=${KEEP_ALIVE:-0}
OPEN_BROWSER=${OPEN_BROWSER:-0}

PASSES=0
FAILS=0
SUMMARY=()
NODE_PID=""

cleanup() {
  if [[ "$KEEP_ALIVE" == "1" ]]; then return 0; fi
  echo "\n🧹 Cleaning up frontend demo node..."
  if [[ -n "$NODE_PID" ]]; then kill "$NODE_PID" 2>/dev/null || true; fi
  sleep 1
}
trap cleanup EXIT

kill_port_if_listening() {
  local port="$1"
  if lsof -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1; then
    echo "   Killing process on port $port"
    lsof -tiTCP:"$port" -sTCP:LISTEN | xargs -r kill -9 || true
    sleep 1
  fi
}

assert_code() {
  local expect="$1"; shift
  local desc="$1"; shift
  local url="$1"
  local code
  code=$(curl -s -o /dev/null -w "%{http_code}" "$url" || true)
  if [[ "$code" == "$expect" ]]; then
    echo "[$code] $desc => PASS"
    PASSES=$((PASSES+1)); SUMMARY+=("PASS - $desc ($code)")
  else
    echo "[$code] $desc => FAIL (expected $expect)"
    FAILS=$((FAILS+1)); SUMMARY+=("FAIL - $desc (got $code, expected $expect)")
  fi
}

start_node() {
  echo "\n🚀 Starting Lithair (static + API) on :$PORT"
  RS_STATIC_DIR="$STATIC_DIR" RUST_LOG=${RUST_LOG:-warn} \
  cargo run --release -p raft_replication_demo --bin http_firewall_declarative -- --port "$PORT" \
    >"$LOG_FILE" 2>&1 &
  NODE_PID=$!

  echo -n "⏳ Waiting for /health"
  for i in $(seq 1 40); do
    if curl -s "$STATUS_URL" >/dev/null; then echo " — ready"; return 0; fi
    echo -n "."; sleep 0.25
  done
  echo "\n❌ Node did not become ready"
  tail -n 120 "$LOG_FILE" || true
  exit 1
}

main() {
  echo "🔨 Building example binary..."
  cargo build --release -p raft_replication_demo --bin http_firewall_declarative >/dev/null

  kill_port_if_listening "$PORT"
  start_node

  echo "\n🔗 Open in browser: $BASE/"
  if [[ "$OPEN_BROWSER" == "1" ]]; then
    if command -v xdg-open >/dev/null 2>&1; then xdg-open "$BASE/" >/dev/null 2>&1 &
    else echo "(tip) Install xdg-open or open manually: $BASE/"; fi
  fi

  echo "\n===== Static checks ====="
  assert_code 200 "GET / (index.html)" "$BASE/"
  assert_code 200 "GET /assets/main.js" "$BASE/assets/main.js"

  echo "\n===== API checks ====="
  assert_code 200 "GET /health" "$STATUS_URL"
  assert_code 200 "GET /api/products (list)" "$API"

  echo "\n===== CRUD basic ====="
  code=$(curl -s -o /dev/null -w "%{http_code}" -X POST "$API" \
    -H 'content-type: application/json' \
    -d '{"name":"Demo","price":12.34}') || true
  if [[ "$code" == "201" ]]; then
    echo "[201] POST /api/products => PASS"; PASSES=$((PASSES+1)); SUMMARY+=("PASS - POST /api/products (201)")
  else
    echo "[$code] POST /api/products => FAIL (expected 201)"; FAILS=$((FAILS+1)); SUMMARY+=("FAIL - POST /api/products ($code, exp 201)")
  fi
  # Avoid immediate re-hit of QPS window from model-level firewall
  sleep 1.2
  assert_code 200 "GET /api/products after create" "$API"

  echo "\n===== Summary ====="
  for line in "${SUMMARY[@]}"; do echo " - $line"; done
  echo
  if [[ $FAILS -eq 0 ]]; then
    echo "✅ Frontend demo PASS ($PASSES scenarios)"
  else
    echo "❌ Frontend demo FAIL ($FAILS failed, $PASSES passed)"
    echo "\nLast 60 log lines for context:"
    tail -n 60 "$LOG_FILE" || true
    exit 1
  fi

  if [[ "$KEEP_ALIVE" == "1" ]]; then
    echo "\n🟢 Server is running at $BASE/ (KEEP_ALIVE=1). Press Ctrl+C to stop."
    # Wait indefinitely until user stops the script; keep background server alive
    tail -f /dev/null & wait
  fi
}

main "$@"
