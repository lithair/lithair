#!/usr/bin/env bash
set -euo pipefail

PORT="${PORT:-18321}"
HOST="${HOST:-127.0.0.1}"
LEADER="http://${HOST}:${PORT}"

# Build binaries
echo "🔧 Building scc2_server_demo and http_loadgen_demo..."
cargo build -q -p scc2_server_demo
cargo build -q -p raft_replication_demo --bin http_loadgen_demo

# Start server
echo "🚀 Starting SCC2 server on ${LEADER} ..."
RUST_LOG=${RUST_LOG:-info} target/debug/scc2_server_demo --port ${PORT} --host ${HOST} &
SERVER_PID=$!
trap 'echo "🛑 Stopping server ${SERVER_PID}"; kill ${SERVER_PID} 2>/dev/null || true' EXIT

# Wait for readiness
for i in {1..30}; do
  if curl -fsS "${LEADER}/health" >/dev/null; then
    echo "✅ Server ready"
    break
  fi
  sleep 0.2
done

# Stateless benchmarks
echo "\n📈 perf-status"
cargo run -q -p raft_replication_demo --bin http_loadgen_demo -- \
  --leader "${LEADER}" --total 20000 --concurrency 512 --mode perf-status --perf-path /health

echo "\n📈 perf-json 1KB"
cargo run -q -p raft_replication_demo --bin http_loadgen_demo -- \
  --leader "${LEADER}" --total 20000 --concurrency 512 --mode perf-json --perf-path /perf/json --perf-bytes 1024

echo "\n📈 perf-echo 1MB"
cargo run -q -p raft_replication_demo --bin http_loadgen_demo -- \
  --leader "${LEADER}" --total 10000 --concurrency 256 --mode perf-echo --perf-path /perf/echo --perf-bytes 1048576

# SCC2 KV tests
echo "\n🗄️ SCC2 put/get"
curl -fsS -X POST "${LEADER}/scc2/put?key=k1&n=4096" && echo
curl -fsS "${LEADER}/scc2/get?key=k1" && echo

# Bulk KV
cat > /tmp/scc2_put_bulk.json <<EOF
[
  {"key":"k2","n":2048},
  {"key":"k3","n":8192},
  {"key":"k4","value":"custom-payload"}
]
EOF

echo "\n🗄️ SCC2 put_bulk"
curl -fsS -H 'content-type: application/json' -d @/tmp/scc2_put_bulk.json "${LEADER}/scc2/put_bulk" && echo

echo "\n🗄️ SCC2 get_bulk"
curl -fsS -H 'content-type: application/json' -d '["k1","k2","k3","k4","missing"]' "${LEADER}/scc2/get_bulk" && echo

echo "\n✅ Demo completed. Server will stop now."
