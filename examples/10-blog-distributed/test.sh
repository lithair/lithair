#!/usr/bin/env bash
# Test script for 10-blog-distributed example
# Tests a 3-node replicated blog cluster with auth and article replication
#
# This script manages its own cluster lifecycle since test-lib.sh
# only handles single servers. It sources test-lib.sh for assertions.
#
# Usage: ./test.sh

source "$(dirname "$0")/../../scripts/test-lib.sh"

# Clean data from previous runs
rm -rf data/blog_node_*

LEADER="http://localhost:8080"
FOLLOWER1="http://localhost:8081"
FOLLOWER2="http://localhost:8082"

PIDS_FILE="/tmp/lithair_blog_dist_test_pids"
LOG_DIR="/tmp/lithair_blog_dist_test_logs"
mkdir -p "$LOG_DIR"

# Override the default cleanup to handle our cluster
_cleanup_cluster() {
    if [ -f "$PIDS_FILE" ]; then
        while read -r pid; do
            kill "$pid" 2>/dev/null || true
        done < "$PIDS_FILE"
        rm -f "$PIDS_FILE"
    fi
    pkill -f "blog-cluster-node" 2>/dev/null || true
}
trap _cleanup_cluster EXIT

# ── BUILD AND START CLUSTER ──────────────────────────────────────────────────
log_info "Building blog-distributed-node..."
cargo build -q -p blog-distributed --bin blog-distributed-node

log_info "Starting 3-node blog cluster..."
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
    RUST_LOG=info "$ROOT_DIR/target/debug/blog-distributed-node" \
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

# Warm up cluster
curl -s -X POST "$LEADER/api/articles" \
    -H "Content-Type: application/json" \
    -d '{"title":"Init","content":"Cluster init","author_id":"system","status":"Published"}' >/dev/null 2>&1
sleep 1

log_info "Cluster ready"

# ── AUTHENTICATION ───────────────────────────────────────────────────────────
log_test "POST /auth/login - admin login on leader"
body=$(http_post "$LEADER/auth/login" '{"username":"admin","password":"password123"}')
assert_status 200 "admin login succeeds"
assert_contains "$body" "session_token" "response has session_token"

TOKEN=$(echo "$body" | sed -n 's/.*"session_token"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
if [ -z "$TOKEN" ]; then
    log_fail "could not extract session token"
    print_summary
fi
log_info "Got token: ${TOKEN:0:16}..."
set_auth "Bearer $TOKEN"

log_test "POST /auth/login - reject invalid credentials"
clear_auth
http_post "$LEADER/auth/login" '{"username":"admin","password":"wrong"}' >/dev/null
assert_status 401 "reject invalid credentials"

# ── CREATE REPLICATION ───────────────────────────────────────────────────────
set_auth "Bearer $TOKEN"

log_test "POST /api/articles - create article on leader"
body=$(http_post "$LEADER/api/articles" '{"title":"Replicated Article","content":"Testing replication.","author_id":"admin","status":"Published"}')
assert_status 201 "create article on leader"
assert_contains "$body" "Replicated Article" "response has title"

# Wait for replication
sleep 1

clear_auth

log_test "GET follower1 /api/articles - replicated data"
body=$(http_get "$FOLLOWER1/api/articles")
assert_status 200 "follower1 responds"
assert_contains "$body" "Replicated Article" "follower1 has replicated article"

log_test "GET follower2 /api/articles - replicated data"
body=$(http_get "$FOLLOWER2/api/articles")
assert_status 200 "follower2 responds"
assert_contains "$body" "Replicated Article" "follower2 has replicated article"

# ── CLUSTER HEALTH ───────────────────────────────────────────────────────────
log_test "GET /_raft/health - cluster health"
body=$(http_get "$LEADER/_raft/health")
assert_status 200 "health endpoint responds"

# ── DATA CONSISTENCY ─────────────────────────────────────────────────────────
log_test "Data consistency across nodes"
leader_data=$(curl -s "$LEADER/api/articles" 2>/dev/null)
f1_data=$(curl -s "$FOLLOWER1/api/articles" 2>/dev/null)
f2_data=$(curl -s "$FOLLOWER2/api/articles" 2>/dev/null)

leader_count=$(echo "$leader_data" | grep -o '"id"' | wc -l)
f1_count=$(echo "$f1_data" | grep -o '"id"' | wc -l)
f2_count=$(echo "$f2_data" | grep -o '"id"' | wc -l)

if [ "$leader_count" = "$f1_count" ] && [ "$leader_count" = "$f2_count" ]; then
    log_pass "all nodes have same article count ($leader_count)"
    (( _TESTS_PASSED++ )) || true
else
    log_fail "article counts differ: leader=$leader_count f1=$f1_count f2=$f2_count"
    (( _TESTS_FAILED++ )) || true
fi

print_summary
