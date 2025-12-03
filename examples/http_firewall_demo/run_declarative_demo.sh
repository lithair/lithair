#!/usr/bin/env bash
set -euo pipefail

# Fully declarative firewall demo runner for http_firewall_declarative
# - Starts server with model-level #[firewall(...)] config only
# - Verifies readiness and base path
# - Runs baseline and rate-limit checks, prints PASS/FAIL summary

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

PORT=${PORT:-8081}
BASE_URL="http://127.0.0.1:${PORT}"
LOG_DIR="examples/http_firewall_demo"
mkdir -p "$LOG_DIR"
LOG_FILE="$LOG_DIR/node_decl_demo.log"

HEALTH_URL="$BASE_URL/health"
PRODUCT_URL="$BASE_URL/api/products"

PASSES=0
FAILS=0
SUMMARY=()

cleanup() {
  echo "\n🧹 Cleaning up declarative node..."
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

assert_code() {
  local expect="$1"; shift
  local label="$1"; shift
  local url="$1"
  local code
  code=$(curl -s -o /dev/null -w "%{http_code}" "$url" || true)
  if [[ "$code" == "$expect" ]]; then
    echo "[$code] $label => PASS (expected $expect)"
    PASSES=$((PASSES+1)); SUMMARY+=("PASS - $label (got $code, expected $expect)")
  else
    echo "[$code] $label => FAIL (expected $expect)"
    FAILS=$((FAILS+1)); SUMMARY+=("FAIL - $label (got $code, expected $expect)")
  fi
}

rate_flood() {
  local url="$1"; shift
  local n="$1"; shift
  local sleep_s="$1"; shift
  local label="$1"
  local ok429=0
  local ok200=0
  for i in $(seq 1 "$n"); do
    local code
    code=$(curl -s -o /dev/null -w "%{http_code}" "$url" || true)
    if [[ "$code" == "429" ]]; then ok429=$((ok429+1)); fi
    if [[ "$code" == "200" ]]; then ok200=$((ok200+1)); fi
    sleep "$sleep_s"
  done
  echo "  200=$ok200 429=$ok429 other=$((n-ok200-ok429))"
  if [[ $ok429 -ge 1 ]]; then
    echo "$label => PASS (need >=1 of 429)"
    PASSES=$((PASSES+1)); SUMMARY+=("PASS - $label (200=$ok200 429=$ok429 other=$((n-ok200-ok429)))")
  else
    echo "$label => FAIL (no 429 observed)"
    FAILS=$((FAILS+1)); SUMMARY+=("FAIL - $label (200=$ok200 429=$ok429 other=$((n-ok200-ok429)))")
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
  echo "\n🚀 Starting declarative model-level firewall node on :${PORT}"
  RUST_LOG=${RUST_LOG:-warn} cargo run --release -p raft_replication_demo --bin http_firewall_declarative -- \
    --port "$PORT" >"$LOG_FILE" 2>&1 &
  NODE_PID=$!

  echo -n "⏳ Waiting for /health"
  for i in $(seq 1 40); do
    if curl -s "$HEALTH_URL" >/dev/null; then echo " — ready"; return 0; fi
    echo -n "."; sleep 0.25
  done
  echo "\n❌ Node did not become ready"
  echo "--- Tail of node log ---"
  tail -n 120 "$LOG_FILE" || true
  exit 1
}

main() {
  cleanup || true
  kill_port_if_listening "$PORT"

  echo "🔨 Building declarative firewall demo binary..."
  cargo build --release -p raft_replication_demo --bin http_firewall_declarative >/dev/null

  start_node
  # Using hardcoded /api/products path - matches DeclarativeModel Product

  echo "\n===== Baseline ====="
  assert_code 200 "Baseline: GET /health should be OK" "$HEALTH_URL"
  assert_code 200 "Baseline: GET /api/<base> should be OK" "$PRODUCT_URL"

  echo "\n===== Rate limit (from model attribute) ====="
  rate_flood "$PRODUCT_URL" 8 0.10 "Rate limit burst (expect some 429)"

  echo "\n===== Summary ====="
  for line in "${SUMMARY[@]}"; do echo " - $line"; done
  echo
  if [[ $FAILS -eq 0 ]]; then
    echo "✅ Declarative firewall demo PASS ($PASSES scenarios)"
  else
    echo "❌ Declarative firewall demo FAIL ($FAILS failed, $PASSES passed)"
    echo "\nLast 60 log lines for context:"
    tail -n 60 "$LOG_FILE" || true
    exit 1
  fi
}

main "$@"
