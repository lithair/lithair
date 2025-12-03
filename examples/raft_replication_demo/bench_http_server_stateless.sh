#!/usr/bin/env bash
set -euo pipefail

# Lithair HTTP Stateless benchmark helper
# - Starts http_hardening_node (perf endpoints enabled at /perf)
# - Runs http_loadgen_demo in perf-* modes
# - Writes a Markdown report in baseline_results/

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$PROJECT_ROOT"

PORT=${PORT:-18320}
URL="http://127.0.0.1:${PORT}"
LOG_FILE="/tmp/rs_http_stateless_${PORT}.log"
REPORT_DIR="${PROJECT_ROOT}/baseline_results"
TS="$(date +%Y%m%d_%H%M%S)"
REPORT_FILE="${REPORT_DIR}/benchmark_http_stateless_${TS}.md"

CONCURRENCY=${CONCURRENCY:-512}
TIMEOUT_S=${TIMEOUT_S:-10}

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
  echo -n "⏳ Waiting for ${url}/health"
  for i in $(seq 1 80); do
    if curl -sf "${url}/health" >/dev/null; then echo " — ready"; return 0; fi
    echo -n "."; sleep 0.25
  done
  echo "\n❌ Server did not become ready at ${url}/health"
  tail -n 120 "$LOG_FILE" || true
  exit 1
}

start_server() {
  echo "\n🚀 Starting Lithair http_hardening_node on :$PORT (perf=/perf)"
  RS_PERF_MAX_BYTES=${RS_PERF_MAX_BYTES:-2000000} \
  RUST_LOG=${RUST_LOG:-error} \
  cargo run --release -p raft_replication_demo --bin http_hardening_node -- --port "$PORT" --open \
    >"$LOG_FILE" 2>&1 &
  NODE_PID=$!
}

stop_server() {
  if [[ -n "${NODE_PID:-}" ]]; then
    kill "$NODE_PID" 2>/dev/null || true
    sleep 1
  fi
}

bench_section() {
  local title="$1"; shift
  echo "\n## ${title}" >>"$REPORT_FILE"
  # Use tee only if TTY is available, otherwise just run command directly
  if [ -t 1 ]; then
    "$@" | tee /dev/tty | sed 's/^/    /' >>"$REPORT_FILE"
  else
    "$@" | tee >(sed 's/^/    /' >>"$REPORT_FILE")
  fi
}

run_status() {
  cargo run --release -p raft_replication_demo --bin http_loadgen_demo -- \
    --leader "$URL" --total ${TOTAL_STATUS:-20000} --concurrency "$CONCURRENCY" \
    --mode perf-status --perf-path /health --timeout-s "$TIMEOUT_S"
}

run_perf_json() {
  local bytes="$1"
  cargo run --release -p raft_replication_demo --bin http_loadgen_demo -- \
    --leader "$URL" --total "$2" --concurrency "$CONCURRENCY" \
    --mode perf-json --perf-path /observe/perf/json --perf-bytes "$bytes" --timeout-s "$TIMEOUT_S"
}

run_perf_bytes() {
  local bytes="$1"
  cargo run --release -p raft_replication_demo --bin http_loadgen_demo -- \
    --leader "$URL" --total "$2" --concurrency "$CONCURRENCY" \
    --mode perf-bytes --perf-path /observe/perf/bytes --perf-bytes "$bytes" --timeout-s "$TIMEOUT_S"
}

run_perf_echo() {
  local bytes="$1"
  cargo run --release -p raft_replication_demo --bin http_loadgen_demo -- \
    --leader "$URL" --total "$2" --concurrency "$CONCURRENCY" \
    --mode perf-echo --perf-path /observe/perf/echo --perf-bytes "$bytes" --timeout-s "$TIMEOUT_S"
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
  cargo build --release -p raft_replication_demo --bins >/dev/null

  kill_port_if_listening "$PORT"
  start_server
  trap stop_server EXIT
  wait_ready "$URL"

  echo "# Lithair HTTP Stateless Benchmark" >"$REPORT_FILE"
  echo "- Timestamp: ${TS}" >>"$REPORT_FILE"
  echo "- URL: ${URL}" >>"$REPORT_FILE"
  echo "- Concurrency: ${CONCURRENCY}" >>"$REPORT_FILE"
  echo "- Timeout(s): ${TIMEOUT_S}" >>"$REPORT_FILE"

  collect_ps

  bench_section "STATUS (/status)" run_status

  bench_section "PERF JSON 1KB"   run_perf_json 1024     ${TOTAL_JSON_1KB:-10000}
  bench_section "PERF JSON 100KB" run_perf_json 102400   ${TOTAL_JSON_100KB:-5000}
  bench_section "PERF JSON 1MB"   run_perf_json 1048576  ${TOTAL_JSON_1MB:-2000}

  bench_section "PERF BYTES 1KB"   run_perf_bytes 1024     ${TOTAL_BYTES_1KB:-10000}
  bench_section "PERF BYTES 100KB" run_perf_bytes 102400   ${TOTAL_BYTES_100KB:-5000}
  bench_section "PERF BYTES 1MB"   run_perf_bytes 1048576  ${TOTAL_BYTES_1MB:-2000}

  bench_section "PERF ECHO 1KB" run_perf_echo 1024    ${TOTAL_ECHO_1KB:-5000}
  bench_section "PERF ECHO 1MB" run_perf_echo 1048576 ${TOTAL_ECHO_1MB:-2000}

  echo "\n## Server log (tail)" >>"$REPORT_FILE"
  echo '```' >>"$REPORT_FILE"
  tail -n 60 "$LOG_FILE" >>"$REPORT_FILE" || true
  echo '```' >>"$REPORT_FILE"

  echo "\n✅ Report written to: $REPORT_FILE"
}

main "$@"
