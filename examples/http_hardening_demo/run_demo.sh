#!/usr/bin/env bash
set -euo pipefail

# HTTP Hardening Demo Orchestrator
# - Starts a single declarative node on :8080
# - Exercises OPTIONS, headers, 415/413/405, and (optionally) 504

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

PORT=${PORT:-8080}
LEADER_URL="http://127.0.0.1:${PORT}"
LOG_DIR="examples/http_hardening_demo"
mkdir -p "$LOG_DIR"

cleanup() {
  echo "\n🧹 Cleaning up demo node..."
  if [[ -n "${NODE_PID:-}" ]]; then kill "$NODE_PID" 2>/dev/null || true; fi
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

start_node() {
  echo "\n🚀 Starting declarative node on :${PORT} (http_hardening_node)"
  # Tighten bulk limit to make 413 easy to demonstrate
  export RS_HTTP_MAX_BODY_BYTES_BULK=${RS_HTTP_MAX_BODY_BYTES_BULK:-2000}
  export RS_HTTP_MAX_BODY_BYTES_SINGLE=${RS_HTTP_MAX_BODY_BYTES_SINGLE:-2048}
  export RS_HTTP_TIMEOUT_MS=${RS_HTTP_TIMEOUT_MS:-10000}

  RUST_LOG=${RUST_LOG:-warn} cargo run --release -p raft_replication_demo --bin http_hardening_node -- \
    --port "$PORT" >"$LOG_DIR/node_demo.log" 2>&1 &
  NODE_PID=$!

  echo -n "⏳ Waiting for /status"
  for i in $(seq 1 80); do
    if curl -sf "$LEADER_URL/status" >/dev/null; then echo " — ready"; return 0; fi
    echo -n "."; sleep 0.25
  done
  echo "\n❌ Node did not become ready"; exit 1
}

assert_code() {
  local expect="$1"; shift
  local desc="$1"; shift
  local code
  code=$(curl -s -o /dev/null -w "%{http_code}" "$@")
  echo "[$code] $desc"
  if [[ "$code" != "$expect" ]]; then
    echo "  ⚠️ Expected $expect, got $code"
  fi
}

print_headers() {
  local url="$1"
  echo "\n🔎 Response headers for: $url"
  curl -s -D - -o /dev/null "$url" | sed 's/^/  /'
}

main() {
  echo "🧹 Freeing port $PORT if used..."
  kill_port_if_listening "$PORT"

  echo "🔨 Building demo (http_hardening_node)..."
  cargo build --release -p raft_replication_demo --bin http_hardening_node >/dev/null

  start_node

  echo "\n===== OPTIONS preflight ====="
  assert_code 204 "OPTIONS /status" -X OPTIONS "$LEADER_URL/status"
  print_headers "$LEADER_URL/status"

  echo "\n===== CORS/security headers on /status ====="
  print_headers "$LEADER_URL/status"

  echo "\n===== 415 Unsupported Media Type (missing JSON) ====="
  assert_code 415 "POST /api/products without Content-Type" -X POST "$LEADER_URL/api/products" -d '{"name":"no_ct"}'

  echo "\n===== 405 Method Not Allowed (wrong verb) ====="
  assert_code 405 "POST /api/products/random-id (only GET allowed)" -X POST "$LEADER_URL/api/products/random-id" -H 'Content-Type: application/json' -d '{}'

  echo "\n===== 413 Payload Too Large (bulk) ====="
  # Build a tiny-bulk payload that exceeds RS_HTTP_MAX_BODY_BYTES_BULK=2000
  PAYLOAD_FILE="$(mktemp)"
  python3 - "$PAYLOAD_FILE" <<'PY'
import json, sys
# create a single item with very long name to exceed 2KB bulk limit
item = {"name": "x" * 5000, "price": 1.0, "category": "Demo"}
json.dump([item], open(sys.argv[1], 'w'))
PY
  assert_code 413 "POST /api/products/_bulk with oversized payload" -X POST "$LEADER_URL/api/products/_bulk" \
    -H 'Content-Type: application/json' --data-binary @"$PAYLOAD_FILE"
  rm -f "$PAYLOAD_FILE"

  if [[ "${DEMO_TRY_TIMEOUT:-0}" == "1" ]]; then
    echo "\n===== 504 Gateway Timeout (best‑effort) ====="
    echo "Restarting node with RS_HTTP_TIMEOUT_MS=1 to try provoking 504..."
    cleanup || true
    kill_port_if_listening "$PORT"
    RS_HTTP_TIMEOUT_MS=1 start_node
    # A quick GET may still succeed; this is best‑effort depending on host speed.
    assert_code 504 "Best‑effort: GET /api/products under 1ms timeout" "$LEADER_URL/api/products" || true
  fi

  echo "\n✅ Demo finished. See $LOG_DIR/node_demo.log for node logs."
}

main "$@"
