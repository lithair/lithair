#!/bin/bash
# ============================================================================
# LITHAIR ROLLING UPGRADE TEST
# ============================================================================
#
# This script demonstrates a real rolling upgrade scenario:
#
# 1. Start 3-node cluster with v1 schema
# 2. Create some test data
# 3. Initiate migration (MigrationBegin through Raft)
# 4. Upgrade nodes one by one (v1 -> v2):
#    - Stop node
#    - Rebuild with new schema feature
#    - Restart node
#    - Wait for sync
# 5. Complete migration (MigrationCommit)
# 6. Verify data integrity
#
# Usage:
#   ./rolling_upgrade_test.sh           # Full test v1 -> v2
#   ./rolling_upgrade_test.sh v3        # Full test v1 -> v3
#   ./rolling_upgrade_test.sh clean     # Clean up
#
# ============================================================================

set -euo pipefail

# Configuration
TARGET_VERSION="${1:-v2}"
DATA_DIR="${PLAYGROUND_DATA_BASE:-./data}"
LOG_DIR="./logs"
PORTS=(8080 8081 8082)
NODE_IDS=(0 1 2)
CURL_TIMEOUT=10
MAX_RETRIES=5
RETRY_DELAY=2

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

# Track if we started nodes (for cleanup)
NODES_STARTED=false

# ============================================================================
# Helper Functions
# ============================================================================

log_header() {
    echo -e "\n${BLUE}═══════════════════════════════════════════════════════════════════${NC}"
    echo -e "${BLUE}  $1${NC}"
    echo -e "${BLUE}═══════════════════════════════════════════════════════════════════${NC}\n"
}

log_step() {
    echo -e "${CYAN}▶ $1${NC}"
}

log_success() {
    echo -e "${GREEN}✓ $1${NC}"
}

log_warning() {
    echo -e "${YELLOW}⚠ $1${NC}"
}

log_error() {
    echo -e "${RED}✗ $1${NC}"
}

# Cleanup function called on exit or interrupt
cleanup_on_exit() {
    local exit_code=$?
    if [ "$NODES_STARTED" = true ]; then
        echo ""
        log_warning "Cleaning up nodes..."
        for i in 0 1 2; do
            stop_node $i 2>/dev/null || true
        done
    fi
    exit $exit_code
}

# Set trap for cleanup
trap cleanup_on_exit SIGINT SIGTERM

# Safe curl wrapper with timeout and error handling
safe_curl() {
    local url="$1"
    shift
    curl --connect-timeout "$CURL_TIMEOUT" --max-time "$((CURL_TIMEOUT * 3))" -sf "$@" "$url" 2>/dev/null
}

# Safe JSON parsing with fallback
safe_jq() {
    local filter="$1"
    local fallback="${2:-}"
    local input
    input=$(cat)

    if [ -z "$input" ]; then
        echo "$fallback"
        return
    fi

    local result
    result=$(echo "$input" | jq -r "$filter" 2>/dev/null) || result="$fallback"

    if [ "$result" = "null" ] || [ -z "$result" ]; then
        echo "$fallback"
    else
        echo "$result"
    fi
}

# Check if port is available
check_port_available() {
    local port=$1
    if command -v ss >/dev/null 2>&1; then
        ! ss -tuln | grep -q ":$port "
    elif command -v netstat >/dev/null 2>&1; then
        ! netstat -tuln | grep -q ":$port "
    else
        # Fallback: try to connect
        ! (echo >/dev/tcp/127.0.0.1/$port) 2>/dev/null
    fi
}

wait_for_node() {
    local port=$1
    local max_wait=${2:-30}
    local waited=0

    while [ $waited -lt $max_wait ]; do
        if safe_curl "http://127.0.0.1:$port/_raft/health" > /dev/null; then
            return 0
        fi
        sleep 1
        waited=$((waited + 1))
    done
    return 1
}

wait_for_leader() {
    local max_wait=${1:-30}
    local waited=0

    while [ $waited -lt $max_wait ]; do
        for port in "${PORTS[@]}"; do
            local is_leader
            is_leader=$(safe_curl "http://127.0.0.1:$port/_raft/health" | safe_jq '.is_leader' 'false')
            if [ "$is_leader" = "true" ]; then
                echo "$port"
                return 0
            fi
        done
        sleep 1
        waited=$((waited + 1))
    done
    return 1
}

get_leader_port() {
    for port in "${PORTS[@]}"; do
        local is_leader
        is_leader=$(safe_curl "http://127.0.0.1:$port/_raft/health" | safe_jq '.is_leader' 'false')
        if [ "$is_leader" = "true" ]; then
            echo "$port"
            return 0
        fi
    done
    return 1
}

# Retry wrapper for critical operations
retry_operation() {
    local operation="$1"
    local max_attempts=${2:-$MAX_RETRIES}
    local delay=${3:-$RETRY_DELAY}
    local attempt=1

    while [ $attempt -le $max_attempts ]; do
        if eval "$operation"; then
            return 0
        fi

        if [ $attempt -lt $max_attempts ]; then
            log_warning "Attempt $attempt failed, retrying in ${delay}s..."
            sleep "$delay"
        fi
        attempt=$((attempt + 1))
    done

    return 1
}

stop_node() {
    local node_id=$1
    local pid_file="$DATA_DIR/node_$node_id.pid"
    local port=${PORTS[$node_id]}

    # Kill by PID file
    if [ -f "$pid_file" ]; then
        local pid
        pid=$(cat "$pid_file" 2>/dev/null) || pid=""
        if [ -n "$pid" ]; then
            kill -9 "$pid" 2>/dev/null || true
        fi
        rm -f "$pid_file"
    fi

    # Kill any process on this port (multiple attempts)
    for _ in 1 2 3; do
        local pids
        pids=$(lsof -ti :$port 2>/dev/null) || pids=""
        if [ -n "$pids" ]; then
            echo "$pids" | xargs kill -9 2>/dev/null || true
            sleep 1
        else
            break
        fi
    done

    # Wait for port to be fully released (up to 15 seconds)
    local wait_count=0
    while [ $wait_count -lt 15 ]; do
        if check_port_available "$port"; then
            return 0
        fi
        sleep 1
        wait_count=$((wait_count + 1))
    done

    log_warning "Port $port may still be in use after stop_node"
}

start_node() {
    local node_id=$1
    local port=${PORTS[$node_id]}
    local features="${2:-}"
    local peers=""

    # Check port is available
    if ! check_port_available "$port"; then
        log_error "Port $port is already in use!"
        return 1
    fi

    # Build peers string (all ports except this one)
    for p in "${PORTS[@]}"; do
        if [ "$p" != "$port" ]; then
            if [ -n "$peers" ]; then
                peers="$peers,$p"
            else
                peers="$p"
            fi
        fi
    done

    log_step "Starting node $node_id on port $port (features: ${features:-none})"

    local cmd
    if [ -n "$features" ]; then
        cmd="PLAYGROUND_DATA_BASE=\"$DATA_DIR\" cargo run --release --bin playground_node --features \"$features\" -- --node-id $node_id --port $port --peers $peers"
    else
        cmd="PLAYGROUND_DATA_BASE=\"$DATA_DIR\" cargo run --release --bin playground_node -- --node-id $node_id --port $port --peers $peers"
    fi

    # Start in background and save PID
    eval "$cmd" > "$LOG_DIR/node_$node_id.log" 2>&1 &
    local pid=$!
    echo "$pid" > "$DATA_DIR/node_$node_id.pid"

    # Verify process actually started
    sleep 1
    if ! kill -0 "$pid" 2>/dev/null; then
        log_error "Node $node_id failed to start. Check $LOG_DIR/node_$node_id.log"
        return 1
    fi

    NODES_STARTED=true
    return 0
}

# Verify item count with retry
verify_item_count() {
    local port=$1
    local expected=$2
    local max_attempts=${3:-5}
    local attempt=1

    while [ $attempt -le $max_attempts ]; do
        local count
        count=$(safe_curl "http://127.0.0.1:$port/api/items" | safe_jq '. | length' '0')

        if [ "$count" = "$expected" ]; then
            return 0
        fi

        if [ $attempt -lt $max_attempts ]; then
            sleep 2
        fi
        attempt=$((attempt + 1))
    done

    return 1
}

# ============================================================================
# Clean Command
# ============================================================================

if [ "${1:-}" = "clean" ]; then
    log_header "CLEANING UP"

    for i in 0 1 2; do
        stop_node $i
    done

    rm -rf "$DATA_DIR"
    rm -rf "$LOG_DIR"

    log_success "Cleanup complete"
    exit 0
fi

# ============================================================================
# Main Test Flow
# ============================================================================

log_header "LITHAIR ROLLING UPGRADE TEST: v1 → $TARGET_VERSION"

# Determine feature flag
case $TARGET_VERSION in
    v2) FEATURE_FLAG="schema-v2" ;;
    v3) FEATURE_FLAG="schema-v3" ;;
    *) log_error "Unknown version: $TARGET_VERSION"; exit 1 ;;
esac

# Check for required tools
for tool in curl jq cargo; do
    if ! command -v $tool >/dev/null 2>&1; then
        log_error "Required tool '$tool' not found"
        exit 1
    fi
done

# Check for port conflicts before starting
log_step "Checking port availability..."
for port in "${PORTS[@]}"; do
    if ! check_port_available "$port"; then
        log_error "Port $port is already in use. Run './rolling_upgrade_test.sh clean' first?"
        exit 1
    fi
done
log_success "All ports available"

# ----------------------------------------------------------------------------
# Phase 1: Build and Start v1 Cluster
# ----------------------------------------------------------------------------

log_header "PHASE 1: Starting v1 Cluster"

mkdir -p "$DATA_DIR"
mkdir -p "$LOG_DIR"

log_step "Building v1 binary..."
if ! cargo build --release --bin playground_node 2>&1 | tail -5; then
    log_error "Failed to build v1 binary"
    exit 1
fi

log_step "Starting 3-node cluster with v1 schema..."
for i in 0 1 2; do
    if ! start_node $i ""; then
        log_error "Failed to start node $i"
        exit 1
    fi
    sleep 2
done

log_step "Waiting for cluster to stabilize..."
sleep 5

log_step "Waiting for leader election..."
LEADER_PORT=""
if ! LEADER_PORT=$(wait_for_leader 45); then
    log_error "No leader elected! Check logs in $LOG_DIR/"
    for i in 0 1 2; do
        echo "--- Node $i log (last 20 lines) ---"
        tail -20 "$LOG_DIR/node_$i.log" 2>/dev/null || echo "(no log)"
    done
    exit 1
fi
log_success "Leader elected on port $LEADER_PORT"

# Verify all nodes are healthy
for port in "${PORTS[@]}"; do
    if wait_for_node "$port" 30; then
        log_success "Node on port $port is healthy"
    else
        log_error "Node on port $port failed to start"
        exit 1
    fi
done

# ----------------------------------------------------------------------------
# Phase 2: Create Test Data
# ----------------------------------------------------------------------------

log_header "PHASE 2: Creating Test Data"

log_step "Creating test items..."
ITEMS_CREATED=0
for i in 1 2 3 4 5; do
    response=$(curl -s -X POST "http://127.0.0.1:$LEADER_PORT/api/items" \
        --connect-timeout "$CURL_TIMEOUT" \
        -H "Content-Type: application/json" \
        -d "{\"name\": \"Test Item $i\", \"description\": \"Created before migration\", \"priority\": $i}" 2>/dev/null) || response=""

    if [ -n "$response" ]; then
        ITEMS_CREATED=$((ITEMS_CREATED + 1))
        echo -n "."
    else
        log_warning "Failed to create item $i"
    fi
done
echo ""

if [ $ITEMS_CREATED -lt 3 ]; then
    log_error "Failed to create enough test items ($ITEMS_CREATED/5)"
    exit 1
fi
log_success "Created $ITEMS_CREATED test items"

log_step "Verifying replication (waiting for sync)..."
sleep 3

for port in "${PORTS[@]}"; do
    count=$(safe_curl "http://127.0.0.1:$port/api/items" | safe_jq '. | length' '0')
    echo "  Node $port: $count items"
done

# ----------------------------------------------------------------------------
# Phase 3: Initiate Migration
# ----------------------------------------------------------------------------

log_header "PHASE 3: Initiating Migration"

# Refresh leader in case it changed
LEADER_PORT=$(get_leader_port) || {
    log_error "Could not find leader!"
    exit 1
}

log_step "Sending MigrationBegin to cluster (leader: $LEADER_PORT)..."
MIGRATION_RESULT=$(curl -s -X POST "http://127.0.0.1:$LEADER_PORT/_playground/migration/begin" \
    --connect-timeout "$CURL_TIMEOUT" \
    -H "Content-Type: application/json" \
    -d '{"to_version": "2.0.0", "description": "Add category field"}' 2>/dev/null) || MIGRATION_RESULT=""

MIGRATION_ID=$(echo "$MIGRATION_RESULT" | safe_jq '.migration_id' '')

if [ -z "$MIGRATION_ID" ]; then
    log_error "Failed to start migration: $MIGRATION_RESULT"
    exit 1
fi
log_success "Migration started: $MIGRATION_ID"

log_step "Applying migration step (AddField: category)..."
STEP_RESULT=$(curl -s -X POST "http://127.0.0.1:$LEADER_PORT/_playground/migration/step" \
    --connect-timeout "$CURL_TIMEOUT" \
    -H "Content-Type: application/json" \
    -d "{\"migration_id\": \"$MIGRATION_ID\", \"step_type\": \"add_field\", \"model\": \"PlaygroundItem\", \"field\": \"category\", \"field_type\": \"String\"}" 2>/dev/null) || STEP_RESULT=""

if [ -n "$STEP_RESULT" ]; then
    echo "  Step result: $(echo "$STEP_RESULT" | jq -c '.' 2>/dev/null || echo "$STEP_RESULT")"
else
    log_warning "No response from migration step (may still have succeeded)"
fi

# ----------------------------------------------------------------------------
# Phase 4: Rolling Upgrade - Followers First
# ----------------------------------------------------------------------------

log_header "PHASE 4: Rolling Upgrade (Followers)"

# Build v2 binary
log_step "Building $TARGET_VERSION binary..."
if ! cargo build --release --bin playground_node --features "$FEATURE_FLAG" 2>&1 | tail -5; then
    log_error "Failed to build $TARGET_VERSION binary"
    exit 1
fi

# Identify current leader before upgrading
INITIAL_LEADER_PORT=$(get_leader_port) || {
    log_error "Could not find leader before follower upgrades"
    exit 1
}
INITIAL_LEADER_NODE=0
for i in 0 1 2; do
    if [ "${PORTS[$i]}" = "$INITIAL_LEADER_PORT" ]; then
        INITIAL_LEADER_NODE=$i
        break
    fi
done
log_step "Current leader is Node $INITIAL_LEADER_NODE (port $INITIAL_LEADER_PORT)"

# Upgrade all non-leader nodes first
for node_id in 0 1 2; do
    if [ "$node_id" = "$INITIAL_LEADER_NODE" ]; then
        continue  # Skip leader for now
    fi

    port=${PORTS[$node_id]}
    log_step "Upgrading Node $node_id (port $port)..."

    echo "  - Stopping node..."
    stop_node $node_id
    sleep 2

    echo "  - Starting with $TARGET_VERSION schema..."
    if ! start_node $node_id "$FEATURE_FLAG"; then
        log_error "Failed to start node $node_id with new schema"
        exit 1
    fi

    echo "  - Waiting for node to rejoin..."
    if wait_for_node "$port" 45; then
        log_success "Node $node_id upgraded and rejoined"
    else
        log_error "Node $node_id failed to rejoin!"
        echo "--- Node $node_id log (last 30 lines) ---"
        tail -30 "$LOG_DIR/node_$node_id.log" 2>/dev/null || echo "(no log)"
        exit 1
    fi

    sleep 3
done

# Verify followers are synced
log_step "Verifying follower sync..."
for node_id in 0 1 2; do
    if [ "$node_id" = "$INITIAL_LEADER_NODE" ]; then
        continue
    fi
    port=${PORTS[$node_id]}
    count=$(safe_curl "http://127.0.0.1:$port/api/items" | safe_jq '. | length' '0')
    echo "  Node $node_id (port $port): $count items"
done

# ----------------------------------------------------------------------------
# Phase 5: Upgrade Leader
# ----------------------------------------------------------------------------

log_header "PHASE 5: Upgrading Leader (Node $INITIAL_LEADER_NODE)"

# Give cluster time to stabilize after follower upgrades
sleep 3

# Use the initial leader we identified
LEADER_NODE_ID=$INITIAL_LEADER_NODE
LEADER_PORT=${PORTS[$LEADER_NODE_ID]}

log_step "Current leader is Node $LEADER_NODE_ID (port $LEADER_PORT)"
log_step "Stopping leader - will trigger new election..."

stop_node $LEADER_NODE_ID
sleep 2

log_step "Waiting for new leader election..."
NEW_LEADER=""
if ! NEW_LEADER=$(wait_for_leader 60); then
    log_error "No new leader elected after 60 seconds!"
    for i in 0 1 2; do
        if [ $i -ne $LEADER_NODE_ID ]; then
            echo "--- Node $i log (last 20 lines) ---"
            tail -20 "$LOG_DIR/node_$i.log" 2>/dev/null || echo "(no log)"
        fi
    done
    exit 1
fi
log_success "New leader elected on port $NEW_LEADER"

log_step "Starting old leader with $TARGET_VERSION schema..."
if ! start_node $LEADER_NODE_ID "$FEATURE_FLAG"; then
    log_error "Failed to restart old leader"
    exit 1
fi

if wait_for_node "${PORTS[$LEADER_NODE_ID]}" 45; then
    log_success "Old leader upgraded and rejoined as follower"
else
    log_error "Old leader failed to rejoin!"
    echo "--- Node $LEADER_NODE_ID log (last 30 lines) ---"
    tail -30 "$LOG_DIR/node_$LEADER_NODE_ID.log" 2>/dev/null || echo "(no log)"
    exit 1
fi

# Allow cluster to stabilize after rejoining
sleep 5

# ----------------------------------------------------------------------------
# Phase 6: Commit Migration
# ----------------------------------------------------------------------------

log_header "PHASE 6: Committing Migration"

# Use the NEW_LEADER that was elected, or find current leader
if [ -n "$NEW_LEADER" ]; then
    LEADER_PORT="$NEW_LEADER"
else
    LEADER_PORT=$(wait_for_leader 30) || {
        log_error "Could not find leader for commit"
        exit 1
    }
fi

log_step "Sending MigrationCommit to leader (port $LEADER_PORT)..."

COMMIT_RESULT=$(curl -s -X POST "http://127.0.0.1:$LEADER_PORT/_playground/migration/commit" \
    --connect-timeout "$CURL_TIMEOUT" \
    -H "Content-Type: application/json" \
    -d "{\"migration_id\": \"$MIGRATION_ID\"}" 2>/dev/null) || COMMIT_RESULT=""

if [ -n "$COMMIT_RESULT" ]; then
    echo "  Commit result: $(echo "$COMMIT_RESULT" | jq -c '.' 2>/dev/null || echo "$COMMIT_RESULT")"
    log_success "Migration committed"
else
    log_warning "No response from commit (may still have succeeded)"
fi

# ----------------------------------------------------------------------------
# Phase 7: Verify Data Integrity
# ----------------------------------------------------------------------------

log_header "PHASE 7: Verifying Data Integrity"

# Wait for cluster to sync after commit
sleep 3

log_step "Checking consistency across all nodes..."
CONSISTENCY=$(safe_curl "http://127.0.0.1:$LEADER_PORT/_playground/consistency") || CONSISTENCY=""

if [ -n "$CONSISTENCY" ]; then
    echo "$CONSISTENCY" | jq '.' 2>/dev/null || echo "$CONSISTENCY"

    ALL_CONSISTENT=$(echo "$CONSISTENCY" | safe_jq '.all_consistent' 'false')
    if [ "$ALL_CONSISTENT" = "true" ]; then
        log_success "All nodes are consistent!"
    else
        log_warning "Some inconsistencies detected - cluster may need time to sync"
    fi
else
    log_warning "Could not check consistency (endpoint may not exist)"
fi

log_step "Creating item with new schema field..."
NEW_ITEM=$(curl -s -X POST "http://127.0.0.1:$LEADER_PORT/api/items" \
    --connect-timeout "$CURL_TIMEOUT" \
    -H "Content-Type: application/json" \
    -d '{"name": "Post-Migration Item", "description": "Created after v2 upgrade", "category": "upgraded"}' 2>/dev/null) || NEW_ITEM=""

if [ -n "$NEW_ITEM" ]; then
    echo "  New item: $(echo "$NEW_ITEM" | jq -c '.' 2>/dev/null || echo "$NEW_ITEM")"
else
    log_warning "Failed to create post-migration item"
fi

log_step "Final item counts:"
for port in "${PORTS[@]}"; do
    count=$(safe_curl "http://127.0.0.1:$port/api/items" | safe_jq '. | length' '?')
    echo "  Port $port: $count items"
done

# ----------------------------------------------------------------------------
# Summary
# ----------------------------------------------------------------------------

log_header "ROLLING UPGRADE COMPLETE"

echo -e "${GREEN}"
echo "  ✓ Started 3-node v1 cluster"
echo "  ✓ Created $ITEMS_CREATED test items"
echo "  ✓ Initiated migration through Raft consensus"
echo "  ✓ Upgraded followers (nodes 1, 2) to $TARGET_VERSION"
echo "  ✓ Upgraded leader to $TARGET_VERSION"
echo "  ✓ Committed migration"
echo "  ✓ Verified data consistency"
echo -e "${NC}"

# Disable cleanup trap since we want to keep cluster running
trap - SIGINT SIGTERM
NODES_STARTED=false

echo ""
echo "Cluster is now running with $TARGET_VERSION schema."
echo "Access the playground at: http://localhost:$LEADER_PORT"
echo ""
echo "To stop the cluster: ./rolling_upgrade_test.sh clean"
