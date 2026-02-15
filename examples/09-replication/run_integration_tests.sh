#!/usr/bin/env bash
set -euo pipefail

# Simple E2E integration runner
# - Starts 3 Lithair nodes (pure_declarative_node)
# - Runs bulk HTTP load against leader
# - Verifies basic convergence across nodes
# - Shuts everything down cleanly

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LOG_DIR="$ROOT_DIR/data"
mkdir -p "$LOG_DIR"

# Reset project data directories to ensure clean test runs
PROJ_ROOT="$(cd "$ROOT_DIR/../.." && pwd)"
echo "🗑️  Resetting node data directories under $PROJ_ROOT/data ..."
rm -rf "$PROJ_ROOT/data/pure_node_"{1,2,3} 2>/dev/null || true
rm -f "$LOG_DIR"/*.log 2>/dev/null || true

cleanup() {
  echo "\n🧹 Cleaning up..."
  if [[ -n "${PID1:-}" ]]; then kill "$PID1" 2>/dev/null || true; fi
  if [[ -n "${PID2:-}" ]]; then kill "$PID2" 2>/dev/null || true; fi
  if [[ -n "${PID3:-}" ]]; then kill "$PID3" 2>/dev/null || true; fi
}
trap cleanup EXIT

start_node() {
  local id="$1" port="$2" peers="$3" log="$4"
  echo "🚀 Starting node ${id} on :${port} (peers=${peers})"
  RUST_LOG=info cargo run --release -p replication --bin pure_declarative_node -- \
    --node-id "$id" \
    --port "$port" \
    --peers "$peers" >"$log" 2>&1 &
  echo $!
}

# Start 3 nodes (1 leader + 2 followers)
PID1=$(start_node 1 8080 "8081,8082" "$LOG_DIR/node1_it.log")
PID2=$(start_node 2 8081 "8080,8082" "$LOG_DIR/node2_it.log")
PID3=$(start_node 3 8082 "8080,8081" "$LOG_DIR/node3_it.log")

echo "⏳ Waiting nodes to boot..."
sleep 3

# Run HTTP load generator
TOTAL=${TOTAL:-2000}
CONCURRENCY=${CONCURRENCY:-256}
BULK=${BULK:-100}
MODE=${MODE:-bulk}
LEADER=${LEADER:-http://127.0.0.1:8080}

echo "\n📦 Running loadgen: total=$TOTAL mode=$MODE bulk=$BULK concurrency=$CONCURRENCY leader=$LEADER"
cargo run --release -p replication --bin http_loadgen_demo -- \
  --leader "$LEADER" \
  --total "$TOTAL" \
  --concurrency "$CONCURRENCY" \
  --bulk-size "$BULK" \
  --mode "$MODE"

# Leader-authentication rejection test (send from non-authoritative leader)
echo "\n🔒 Testing leader-auth rejection on follower (expect 409)..."
LA_CODE=$(curl -s -o /dev/null -w "%{http_code}" -X POST \
  -H 'content-type: application/json' \
  -d '{"operation":"create","data":null,"id":"la_test","leader_node_id":999,"timestamp":0}' \
  http://127.0.0.1:8081/internal/replicate)
if [[ "$LA_CODE" != "409" ]]; then
  echo "❌ Leader-auth rejection failed: expected 409, got $LA_CODE"
  exit 1
else
  echo "✅ Leader-auth rejection OK (409)"
fi

# Bulk dedupe test (same batch_id twice → second ignored)
echo "\n🔁 Testing bulk deduplication on follower..."
TMPB=$(mktemp)
UUID1=$(uuidgen 2>/dev/null || cat /proc/sys/kernel/random/uuid)
BATCH_ID="e2e-batch-123"
cat > "$TMPB" <<JSON
{"operation":"create_bulk","items":[{"id":"$UUID1","name":"e2e_dedupe","price":9.99,"category":"Books"}],"leader_node_id":1,"timestamp":0,"batch_id":"$BATCH_ID"}
JSON

R1=$(curl -sS -H 'content-type: application/json' -d @"$TMPB" http://127.0.0.1:8081/internal/replicate_bulk)
R2=$(curl -sS -H 'content-type: application/json' -d @"$TMPB" http://127.0.0.1:8081/internal/replicate_bulk)
echo "First bulk response: $R1"
echo "Second bulk response: $R2"
echo "$R2" | grep -q 'duplicate_ignored' || {
  echo "❌ Bulk dedupe failed: second response should indicate duplicate_ignored"
  exit 1
}

# Restart follower (8081) and re-validate persisted dedupe
echo "\n🔄 Restarting follower 8081 to validate persisted dedupe..."
if [[ -n "${PID2:-}" ]]; then kill "$PID2" 2>/dev/null || true; fi
sleep 1
PID2=$(start_node 2 8081 "8080,8082" "$LOG_DIR/node2_restart_it.log")
sleep 3

R3=$(curl -sS -H 'content-type: application/json' -d @"$TMPB" http://127.0.0.1:8081/internal/replicate_bulk)
echo "Post-restart bulk response: $R3"
echo "$R3" | grep -q 'duplicate_ignored' || {
  echo "❌ Persisted bulk dedupe failed after restart: expected duplicate_ignored"
  exit 1
}

rm -f "$TMPB"

# Allow background reconcile on followers to align storage with leader
RECONCILE_WAIT=${RECONCILE_WAIT:-7}
echo "\n⏳ Waiting ${RECONCILE_WAIT}s for background reconcile before convergence check..."
sleep "$RECONCILE_WAIT"

# Verify convergence after follower restart
"$ROOT_DIR/verify_convergence.sh" || {
  echo "❌ Convergence verification failed"
  exit 1
}

# Rolling restart: restart follower 8082 and verify convergence again
echo "\n🔄 Rolling restart: restarting follower 8082..."
if [[ -n "${PID3:-}" ]]; then kill "$PID3" 2>/dev/null || true; fi
sleep 1
PID3=$(start_node 3 8082 "8080,8081" "$LOG_DIR/node3_restart_it.log")
sleep 3
"$ROOT_DIR/verify_convergence.sh" || {
  echo "❌ Convergence failed after follower 8082 restart"
  exit 1
}

# Leader restart test: kill leader 8080, let cluster elect new leader, run small load via follower (redirect), check convergence
echo "\n🟥 Restarting leader 8080 to validate leader failover..."
if [[ -n "${PID1:-}" ]]; then kill "$PID1" 2>/dev/null || true; fi
sleep 5

echo "\n📦 Post-leader-restart small load (target follower 8081 to follow redirect)"
cargo run --release -p replication --bin http_loadgen_demo -- \
  --leader "http://127.0.0.1:8081" \
  --total 200 \
  --concurrency 64 \
  --bulk-size 20 \
  --mode bulk || true

echo "\n⏳ Waiting ${RECONCILE_WAIT}s for background reconcile after leader restart..."
sleep "$RECONCILE_WAIT"

"$ROOT_DIR/verify_convergence.sh" || {
  echo "❌ Convergence failed after leader restart"
  exit 1
}

echo "\n✅ Integration tests completed successfully"
