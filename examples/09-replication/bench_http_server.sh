#!/usr/bin/env bash
set -euo pipefail

# Lithair HTTP server benchmark helper
# - Starts http_hardening_node
# - Runs http_loadgen_demo in BULK and RANDOM modes
# - Writes a Markdown report in baseline_results/

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$PROJECT_ROOT"

PORT=${PORT:-18300}
URL="http://127.0.0.1:${PORT}"
LOG_FILE="/tmp/rs_http_bench_${PORT}.log"
REPORT_DIR="${PROJECT_ROOT}/baseline_results"
TS="$(date +%Y%m%d_%H%M%S)"
REPORT_FILE="${REPORT_DIR}/benchmark_http_${TS}.md"

mkdir -p "$REPORT_DIR"

kill_port_if_listening() {
  local port="$1"
  if command -v lsof >/dev/null 2>&1; then
    if lsof -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1; then
      echo "   Killing process on port $port"
      lsof -tiTCP:"$port" -sTCP:LISTEN | xargs -r kill -9 || true
      sleep 1
    fi
  fi
}

wait_ready() {
  local url="$1"
  echo -n "⏳ Waiting for ${url}/status"
  for i in $(seq 1 80); do
    if curl -sf "${url}/status" >/dev/null; then echo " — ready"; return 0; fi
    echo -n "."; sleep 0.25
  done
  echo "\n❌ Server did not become ready at ${url}/status"
  tail -n 120 "$LOG_FILE" || true
  exit 1
}

start_server() {
  echo "\n🚀 Starting Lithair http_hardening_node on :$PORT"
  RS_HTTP_MAX_BODY_BYTES_BULK=${RS_HTTP_MAX_BODY_BYTES_BULK:-2000000} \
  RS_HTTP_MAX_BODY_BYTES_SINGLE=${RS_HTTP_MAX_BODY_BYTES_SINGLE:-1048576} \
  RS_HTTP_TIMEOUT_MS=${RS_HTTP_TIMEOUT_MS:-10000} \
  RUST_LOG=${RUST_LOG:-error} \
  cargo run --release -p replication --bin http_hardening_node -- --port "$PORT" \
    >"$LOG_FILE" 2>&1 &
  NODE_PID=$!
}

stop_server() {
  if [[ -n "${NODE_PID:-}" ]]; then
    kill "$NODE_PID" 2>/dev/null || true
    sleep 1
  fi
}

run_loadgen_bulk() {
  cargo run --release -p replication --bin http_loadgen_demo -- \
    --leader "$URL" --total ${TOTAL_BULK:-20000} --concurrency ${CONCURRENCY:-512} \
    --mode bulk --bulk-size ${BULK_SIZE:-100} --timeout-s ${TIMEOUT_S:-10}
}

run_loadgen_random() {
  cargo run --release -p replication --bin http_loadgen_demo -- \
    --leader "$URL" --total ${TOTAL_RANDOM:-20000} --concurrency ${CONCURRENCY:-512} \
    --mode random --read-targets "$URL" --read-path /status --timeout-s ${TIMEOUT_S:-10}
}

collect_ps() {
  echo "\nProcess snapshot:" >>"$REPORT_FILE"
  if command -v ps >/dev/null 2>&1; then
    ps -o pid,ppid,%cpu,%mem,cmd -p "$NODE_PID" >>"$REPORT_FILE" || true
    local threads
    threads=$(ps -L -p "$NODE_PID" | wc -l || echo 0)
    echo "Threads: ${threads}" >>"$REPORT_FILE"
  fi
}

main() {
  echo "🔨 Building release binaries..."
  cargo build --release -p replication --bins >/dev/null

  kill_port_if_listening "$PORT"
  start_server
  trap stop_server EXIT
  wait_ready "$URL"

  echo "# Lithair HTTP Benchmark" >"$REPORT_FILE"
  echo "- Timestamp: ${TS}" >>"$REPORT_FILE"
  echo "- URL: ${URL}" >>"$REPORT_FILE"
  echo "- Concurrency: ${CONCURRENCY:-512}" >>"$REPORT_FILE"
  echo "- BulkSize: ${BULK_SIZE:-100}" >>"$REPORT_FILE"
  echo "- Timeout(s): ${TIMEOUT_S:-10}" >>"$REPORT_FILE"

  collect_ps

  echo "\n## BULK run" >>"$REPORT_FILE"
  run_loadgen_bulk | tee /dev/tty | sed 's/^/    /' >>"$REPORT_FILE"

  echo "\n## RANDOM run" >>"$REPORT_FILE"
  run_loadgen_random | tee /dev/tty | sed 's/^/    /' >>"$REPORT_FILE"

  echo "\n## Server log (tail)" >>"$REPORT_FILE"
  echo '```' >>"$REPORT_FILE"
  tail -n 60 "$LOG_FILE" >>"$REPORT_FILE" || true
  echo '```' >>"$REPORT_FILE"

  echo "\n✅ Report written to: $REPORT_FILE"
}

main "$@"
