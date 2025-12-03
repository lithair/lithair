#!/usr/bin/env bash
set -euo pipefail

# HTTP Firewall Demo Orchestrator (v1)
# - Starts a single declarative node on :8080
# - Demonstrates IP deny/allow and rate limits (global/per-IP)

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

PORT=${PORT:-8080}
BASE_URL="http://127.0.0.1:${PORT}"
LOG_DIR="examples/http_firewall_demo"
mkdir -p "$LOG_DIR"

HEALTH_URL="$BASE_URL/health"
PRODUCT_URL="$BASE_URL/api/products"

# Aggregated results
PASSES=0
FAILS=0
SUMMARY=()

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

derive_product_url() {
  local base_path
  base_path=$(curl -s "$HEALTH_URL" | sed -n 's/.*"base_path":"\([^\"]*\)".*/\1/p')
  if [[ -z "$base_path" ]]; then
    echo "⚠️  Could not derive base_path from /health"
    return 1
  fi
  PRODUCT_URL="$BASE_URL$base_path"
  echo "   Detected API base_path: $base_path"
}

start_node() {
  echo "\n🚀 Starting declarative node on :${PORT} (firewall demo)"
  # Baseline: firewall disabled
  RUST_LOG=${RUST_LOG:-warn} cargo run --release -p raft_replication_demo --bin http_firewall_node -- \
    --port "$PORT" --fw-enable false >"$LOG_DIR/node_demo.log" 2>&1 &
  NODE_PID=$!

  echo -n "⏳ Waiting for /health"
  for i in $(seq 1 40); do
    if curl -s "$HEALTH_URL" >/dev/null; then echo " — ready"; return 0; fi
    echo -n "."; sleep 0.25
  done
  echo "\n❌ Node did not become ready"
  echo "--- Tail of node log ---"
  tail -n 80 "$LOG_DIR/node_demo.log" || true
  exit 1
}

start_node_with_cfg() {
  echo "\n🚀 Starting node with FW cfg: $*"
  # Start with CLI-based declarative config (overrides env)
  RUST_LOG=${RUST_LOG:-warn} cargo run --release -p raft_replication_demo --bin http_firewall_node -- \
    --port "$PORT" $* >"$LOG_DIR/node_demo.log" 2>&1 &
  NODE_PID=$!

  echo -n "⏳ Waiting for /health"
  for i in $(seq 1 40); do
    if curl -s "$HEALTH_URL" >/dev/null; then echo " — ready"; return 0; fi
    echo -n "."; sleep 0.25
  done
  echo "\n❌ Node did not become ready"; exit 1
}

assert_code() {
  local expect="$1"; shift
  local desc="$1"; shift
  local code
  # shellcheck disable=SC2068
  code=$(curl -s -o /dev/null -w "%{http_code}" $@)
  local status="FAIL"
  if [[ "$code" == "$expect" ]]; then
    status="PASS"; PASSES=$((PASSES+1))
  else
    FAILS=$((FAILS+1))
  fi
  echo "[$code] $desc => $status (expected $expect)"
  SUMMARY+=("$status - $desc (got $code, expected $expect)")
}

rate_flood() {
  local url="$1"; shift
  local n="$1"; shift
  local expect_min_429="$1"; shift || true
  local desc="$1"; shift || true
  echo "Spamming $n requests to $url..."
  local ok=0; local too_many=0; local other=0
  for _ in $(seq 1 "$n"); do
    local c
    c=$(curl -s -o /dev/null -w "%{http_code}" "$url") || true
    case "$c" in
      200) ok=$((ok+1));;
      429) too_many=$((too_many+1));;
      *) other=$((other+1));;
    esac
  done
  echo "  200=$ok 429=$too_many other=$other"
  local status="FAIL"
  if [[ "$too_many" -ge "$expect_min_429" ]]; then
    status="PASS"; PASSES=$((PASSES+1))
  else
    FAILS=$((FAILS+1))
  fi
  echo "$desc => $status (need >=$expect_min_429 of 429)"
  SUMMARY+=("$status - $desc (200=$ok 429=$too_many other=$other)")
}

main() {
  echo "🧹 Freeing port $PORT if used..."
  kill_port_if_listening "$PORT"

  echo "🔨 Building demo binary..."
  cargo build --release -p raft_replication_demo --bin http_firewall_node >/dev/null

  # Baseline: no firewall (disabled)
  echo "\n===== Baseline: firewall disabled ====="
  cleanup || true
  kill_port_if_listening "$PORT"
  start_node
  # Using hardcoded /api/products path - matches DeclarativeModel Product
  assert_code 200 "Baseline: GET /health should be OK" "$HEALTH_URL"
  assert_code 200 "Baseline: GET /api/<base> should be OK" "$PRODUCT_URL"

  # Deny list: deny localhost
  echo "\n===== Deny list (protect products only) ====="
  cleanup || true
  kill_port_if_listening "$PORT"
  start_node_with_cfg --fw-enable true --fw-deny 127.0.0.1 --fw-protected-prefixes "/api/products" --fw-exempt-prefixes "/health,/health"
  # Using hardcoded /api/products path - matches DeclarativeModel Product
  assert_code 200 "Deny list: /health remains open" "$HEALTH_URL"
  assert_code 403 "Deny list: /api/products should be forbidden" "$PRODUCT_URL"

  # Allow list: allow only a different IP (should block), then allow localhost
  echo "\n===== Allow list mismatch (protect products only) ====="
  cleanup || true
  kill_port_if_listening "$PORT"
  start_node_with_cfg --fw-enable true --fw-allow 192.0.2.10 --fw-protected-prefixes "/api/products" --fw-exempt-prefixes "/health,/health"
  # Using hardcoded /api/products path - matches DeclarativeModel Product
  assert_code 200 "Allow list mismatch: /health remains open" "$HEALTH_URL"
  assert_code 403 "Allow list mismatch: /api/products should be forbidden" "$PRODUCT_URL"

  echo "\n===== Allow list match (protect products only) ====="
  cleanup || true
  kill_port_if_listening "$PORT"
  start_node_with_cfg --fw-enable true --fw-allow 127.0.0.1 --fw-protected-prefixes "/api/products" --fw-exempt-prefixes "/health,/health"
  # Using hardcoded /api/products path - matches DeclarativeModel Product
  assert_code 200 "Allow list match: /health remains open" "$HEALTH_URL"
  assert_code 200 "Allow list match: /api/products should be OK" "$PRODUCT_URL"

  # Global rate limit
  echo "\n===== Global rate limit on product routes only (QPS=3) ====="
  cleanup || true
  kill_port_if_listening "$PORT"
  start_node_with_cfg --fw-enable true --fw-global-qps 3 --fw-protected-prefixes "/api/products" --fw-exempt-prefixes "/health,/health"
  # Using hardcoded /api/products path - matches DeclarativeModel Product
  rate_flood "$PRODUCT_URL" 8 1 "Global rate limit on products: expect some 429"

  # Per-IP rate limit
  echo "\n===== Per-IP rate limit on product routes only (QPS=2) ====="
  cleanup || true
  kill_port_if_listening "$PORT"
  start_node_with_cfg --fw-enable true --fw-perip-qps 2 --fw-protected-prefixes "/api/products" --fw-exempt-prefixes "/health,/health"
  # Using hardcoded /api/products path - matches DeclarativeModel Product
  rate_flood "$PRODUCT_URL" 6 1 "Per-IP rate limit on products: expect some 429"

  echo "\n===== Summary ====="
  for line in "${SUMMARY[@]}"; do echo " - $line"; done
  echo
  if [[ $FAILS -eq 0 ]]; then
    echo "✅ Firewall demo PASS ($PASSES scenarios)"
  else
    echo "❌ Firewall demo FAIL ($FAILS failed, $PASSES passed)"
    echo "\nLast 30 log lines for context:"
    tail -n 30 "$LOG_DIR/node_demo.log" || true
  fi
}

main "$@"
