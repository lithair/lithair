#!/usr/bin/env bash
# Test script for 09-replication example
# Tests a 3-node Raft cluster: create, replication, update, consistency
#
# This script manages its own cluster lifecycle since test-lib.sh
# only handles single servers. It sources test-lib.sh for assertions.
#
# Usage: ./test.sh

source "$(dirname "$0")/../../scripts/test-lib.sh"

# Clean data from previous runs
rm -rf data/node_* data/raft/node_*

LEADER="http://localhost:8080"
FOLLOWER1="http://localhost:8081"
FOLLOWER2="http://localhost:8082"

PIDS_FILE="/tmp/lithair_replication_test_pids"
LOG_DIR="/tmp/lithair_replication_test_logs"
mkdir -p "$LOG_DIR"

# Override the default cleanup to handle our cluster
_cleanup_cluster() {
    if [ -f "$PIDS_FILE" ]; then
        while read -r pid; do
            kill "$pid" 2>/dev/null || true
        done < "$PIDS_FILE"
        rm -f "$PIDS_FILE"
    fi
    pkill -f "replication-declarative-node" 2>/dev/null || true
}
trap _cleanup_cluster EXIT

# ── BUILD AND START CLUSTER ──────────────────────────────────────────────────
log_info "Building replication-declarative-node..."
cargo build -q -p replication --bin replication-declarative-node

log_info "Starting 3-node cluster..."
> "$PIDS_FILE"

for i in 0 1 2; do
    port=$((8080 + i))
    peers=""
    for j in 0 1 2; do
        if [ "$j" -ne "$i" ]; then
            [ -n "$peers" ] && peers="$peers,"
            peers="$peers$((8080 + j))"
        fi
    done
    RUST_LOG=info "$ROOT_DIR/target/debug/replication-declarative-node" \
        --node-id "$i" --port "$port" --peers "$peers" \
        > "$LOG_DIR/node_$i.log" 2>&1 &
    echo "$!" >> "$PIDS_FILE"
    sleep 0.3
done

# Wait for all nodes to be ready
log_info "Waiting for cluster nodes..."
for port in 8080 8081 8082; do
    for attempt in $(seq 1 40); do
        if curl -so /dev/null --connect-timeout 1 "http://localhost:$port/" 2>/dev/null; then
            break
        fi
        sleep 0.25
    done
done
sleep 2

# Warm up cluster with an initial write
curl -s -X POST "$LEADER/api/products" \
    -H "Content-Type: application/json" \
    -d '{"name":"Cluster Init","price":0.01,"category":"System"}' >/dev/null 2>&1
sleep 1

log_info "Cluster ready"

# ── CREATE REPLICATION ───────────────────────────────────────────────────────
log_test "POST leader /api/products - create product"
body=$(http_post "$LEADER/api/products" '{"name":"MacBook Pro","price":2499.99,"category":"Electronics"}')
assert_status 201 "create product on leader"

PRODUCT_ID=$(echo "$body" | sed -n 's/.*"id"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
log_info "Created product ID: ${PRODUCT_ID:-unknown}"

# Wait for replication
sleep 1

log_test "GET follower1 /api/products - replicated data"
body=$(http_get "$FOLLOWER1/api/products")
assert_status 200 "follower1 responds"
assert_contains "$body" "MacBook Pro" "follower1 has replicated product"

log_test "GET follower2 /api/products - replicated data"
body=$(http_get "$FOLLOWER2/api/products")
assert_status 200 "follower2 responds"
assert_contains "$body" "MacBook Pro" "follower2 has replicated product"

# ── CLUSTER HEALTH ───────────────────────────────────────────────────────────
log_test "GET /_raft/health - cluster health"
body=$(http_get "$LEADER/_raft/health")
assert_status 200 "health endpoint responds"

# ── DATA CONSISTENCY ─────────────────────────────────────────────────────────
log_test "Data consistency across nodes"
leader_data=$(curl -s "$LEADER/api/products" 2>/dev/null)
f1_data=$(curl -s "$FOLLOWER1/api/products" 2>/dev/null)
f2_data=$(curl -s "$FOLLOWER2/api/products" 2>/dev/null)

leader_count=$(echo "$leader_data" | grep -o '"id"' | wc -l)
f1_count=$(echo "$f1_data" | grep -o '"id"' | wc -l)
f2_count=$(echo "$f2_data" | grep -o '"id"' | wc -l)

if [ "$leader_count" = "$f1_count" ] && [ "$leader_count" = "$f2_count" ]; then
    log_pass "all nodes have same item count ($leader_count)"
    (( _TESTS_PASSED++ )) || true
else
    log_fail "item counts differ: leader=$leader_count f1=$f1_count f2=$f2_count"
    (( _TESTS_FAILED++ )) || true
fi

print_summary
