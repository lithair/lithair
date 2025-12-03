#!/usr/bin/env bash
set -euo pipefail

PORT="${PORT:-18321}"
HOST="${HOST:-127.0.0.1}"
LEADER="http://${HOST}:${PORT}"
BYTES=${BYTES:-65536}
TOTAL=${TOTAL:-20000}
CONC=${CONC:-512}

# Ensure server is up
if ! curl -fsS "${LEADER}/health" >/dev/null; then
  echo "Starting server..."
  cargo run -q -p scc2_server_demo -- --port ${PORT} --host ${HOST} &
  PID=$!
  trap 'kill ${PID} 2>/dev/null || true' EXIT
  for i in {1..30}; do
    if curl -fsS "${LEADER}/health" >/dev/null; then break; fi
    sleep 0.2
  done
fi

# No gzip (force)
echo "\n🚫 GZIP OFF"
TIME_NG=$(TIMEFORMAT=%R; { time cargo run -q -p raft_replication_demo --bin http_loadgen_demo -- \
  --leader "${LEADER}" --total ${TOTAL} --concurrency ${CONC} --mode perf-json --perf-path /perf/json --perf-bytes ${BYTES} ; } 2>&1 >/dev/null)

echo "Duration (no-gzip): ${TIME_NG}s"

# With gzip using curl (direct)
echo "\n✅ GZIP ON (curl latency sample)"
START=$(date +%s%3N)
for i in {1..50}; do
  curl -fsSH 'Accept-Encoding: gzip' "${LEADER}/perf/json?bytes=${BYTES}&gzip=1" -o /dev/null
done
END=$(date +%s%3N)
MS=$((END-START))
echo "50 requests with gzip: ${MS} ms (~$((50*1000/MS)) rps)"

echo "\nTip: compare CPU usage and network throughput with and without gzip for large payloads."
