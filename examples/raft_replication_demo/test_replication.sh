#!/bin/bash
#
# Lithair Replication Test Script
#
# Tests the full replication pipeline:
# 1. Create data on leader
# 2. Verify replication to followers
# 3. Check cluster health
# 4. Test update/delete replication
#

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

LEADER="http://localhost:8080"
FOLLOWER1="http://localhost:8081"
FOLLOWER2="http://localhost:8082"

log_info() { echo -e "${GREEN}[INFO]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }
log_test() { echo -e "${BLUE}[TEST]${NC} $1"; }

check_nodes() {
    log_info "Checking node availability..."

    for url in "$LEADER" "$FOLLOWER1" "$FOLLOWER2"; do
        if ! curl -s --connect-timeout 2 "$url/status" > /dev/null 2>&1; then
            log_error "Node $url is not responding"
            log_warn "Start the cluster first: ./run_cluster.sh start"
            exit 1
        fi
    done

    log_info "All nodes are up!"
    echo ""
}

test_create_replication() {
    log_test "═══════════════════════════════════════════════════════════"
    log_test "  Test 1: CREATE Replication"
    log_test "═══════════════════════════════════════════════════════════"
    echo ""

    # Create a product on leader
    log_info "Creating product on LEADER..."
    local response=$(curl -s -X POST "$LEADER/api/products" \
        -H "Content-Type: application/json" \
        -d '{"name":"MacBook Pro","price":2499.99,"category":"Electronics"}')

    local product_id=$(echo "$response" | jq -r '.id // .entity_id // empty')

    if [ -z "$product_id" ]; then
        log_error "Failed to create product: $response"
        return 1
    fi

    log_info "Created product ID: $product_id"
    echo "$response" | jq .
    echo ""

    # Wait for replication
    sleep 0.5

    # Check on followers
    log_info "Checking replication on FOLLOWER1..."
    local follower1_data=$(curl -s "$FOLLOWER1/api/products")
    local found1=$(echo "$follower1_data" | jq --arg id "$product_id" '.[] | select(.id == $id)')

    if [ -n "$found1" ]; then
        echo -e "  └─ ${GREEN}REPLICATED${NC}"
    else
        echo -e "  └─ ${RED}NOT FOUND${NC}"
        log_warn "Data on follower1: $follower1_data"
    fi

    log_info "Checking replication on FOLLOWER2..."
    local follower2_data=$(curl -s "$FOLLOWER2/api/products")
    local found2=$(echo "$follower2_data" | jq --arg id "$product_id" '.[] | select(.id == $id)')

    if [ -n "$found2" ]; then
        echo -e "  └─ ${GREEN}REPLICATED${NC}"
    else
        echo -e "  └─ ${RED}NOT FOUND${NC}"
        log_warn "Data on follower2: $follower2_data"
    fi

    echo ""
    echo "$product_id"  # Return for next tests
}

test_update_replication() {
    local product_id="$1"

    log_test "═══════════════════════════════════════════════════════════"
    log_test "  Test 2: UPDATE Replication"
    log_test "═══════════════════════════════════════════════════════════"
    echo ""

    if [ -z "$product_id" ]; then
        log_warn "No product ID provided, skipping update test"
        return
    fi

    # Update on leader
    log_info "Updating product price on LEADER..."
    local response=$(curl -s -X PUT "$LEADER/api/products/$product_id" \
        -H "Content-Type: application/json" \
        -d '{"price":1999.99}')

    echo "$response" | jq . 2>/dev/null || echo "$response"
    echo ""

    # Wait for replication
    sleep 0.5

    # Check updated value on followers
    log_info "Checking update on FOLLOWER1..."
    local price1=$(curl -s "$FOLLOWER1/api/products/$product_id" | jq '.price // empty')
    echo "  └─ Price: $price1"

    log_info "Checking update on FOLLOWER2..."
    local price2=$(curl -s "$FOLLOWER2/api/products/$product_id" | jq '.price // empty')
    echo "  └─ Price: $price2"

    echo ""
}

test_cluster_health() {
    log_test "═══════════════════════════════════════════════════════════"
    log_test "  Test 3: Cluster Health"
    log_test "═══════════════════════════════════════════════════════════"
    echo ""

    log_info "Fetching cluster health from leader..."
    curl -s "$LEADER/_raft/health" | jq .
    echo ""
}

test_bulk_write() {
    log_test "═══════════════════════════════════════════════════════════"
    log_test "  Test 4: Bulk Write Performance"
    log_test "═══════════════════════════════════════════════════════════"
    echo ""

    local count=20

    log_info "Creating $count products in parallel..."
    local start=$(date +%s%N)

    for i in $(seq 1 $count); do
        curl -s -X POST "$LEADER/api/products" \
            -H "Content-Type: application/json" \
            -d "{\"name\":\"Product $i\",\"price\":$((i * 10)).99,\"category\":\"Test\"}" &
    done

    wait  # Wait for all background jobs

    local end=$(date +%s%N)
    local duration=$(( (end - start) / 1000000 ))  # Convert to ms

    log_info "Created $count products in ${duration}ms"
    log_info "Throughput: $(echo "scale=0; $count * 1000 / $duration" | bc) ops/sec"
    echo ""

    # Wait for replication
    sleep 1

    # Count on each node
    log_info "Verifying counts..."
    local leader_count=$(curl -s "$LEADER/api/products" | jq 'length')
    local f1_count=$(curl -s "$FOLLOWER1/api/products" | jq 'length')
    local f2_count=$(curl -s "$FOLLOWER2/api/products" | jq 'length')

    echo "  Leader:    $leader_count products"
    echo "  Follower1: $f1_count products"
    echo "  Follower2: $f2_count products"

    if [ "$leader_count" = "$f1_count" ] && [ "$leader_count" = "$f2_count" ]; then
        echo -e "  ${GREEN}✓ All nodes in sync!${NC}"
    else
        echo -e "  ${YELLOW}⚠ Counts differ (replication may still be in progress)${NC}"
    fi
    echo ""
}

test_read_from_followers() {
    log_test "═══════════════════════════════════════════════════════════"
    log_test "  Test 5: Read from Followers"
    log_test "═══════════════════════════════════════════════════════════"
    echo ""

    log_info "Reading products from each node..."

    echo "Leader:"
    curl -s "$LEADER/api/products" | jq 'length'

    echo "Follower1:"
    curl -s "$FOLLOWER1/api/products" | jq 'length'

    echo "Follower2:"
    curl -s "$FOLLOWER2/api/products" | jq 'length'
    echo ""
}

test_data_consistency() {
    log_test "═══════════════════════════════════════════════════════════"
    log_test "  Test 6: DATA CONSISTENCY (Checksum Comparison)"
    log_test "═══════════════════════════════════════════════════════════"
    echo ""

    log_info "Comparing actual data between nodes (sorted by ID)..."

    # Get and sort data from each node
    local leader_hash=$(curl -s "$LEADER/api/products" | jq -c 'sort_by(.id)' | md5sum | cut -d' ' -f1)
    local f1_hash=$(curl -s "$FOLLOWER1/api/products" | jq -c 'sort_by(.id)' | md5sum | cut -d' ' -f1)
    local f2_hash=$(curl -s "$FOLLOWER2/api/products" | jq -c 'sort_by(.id)' | md5sum | cut -d' ' -f1)

    echo "  Leader checksum:    $leader_hash"
    echo "  Follower1 checksum: $f1_hash"
    echo "  Follower2 checksum: $f2_hash"
    echo ""

    if [ "$leader_hash" = "$f1_hash" ] && [ "$leader_hash" = "$f2_hash" ]; then
        echo -e "  ${GREEN}✅ DATA CONSISTENCY VERIFIED - All nodes have identical data!${NC}"
        return 0
    else
        echo -e "  ${RED}❌ DATA MISMATCH - Nodes have different data!${NC}"

        # Show diff details
        log_warn "Showing ID differences..."
        echo "Leader IDs:"
        curl -s "$LEADER/api/products" | jq -r '.[].id' | sort | head -5
        echo "Follower1 IDs:"
        curl -s "$FOLLOWER1/api/products" | jq -r '.[].id' | sort | head -5
        return 1
    fi
}

# Main
echo ""
echo "═══════════════════════════════════════════════════════════════════"
echo "       LITHAIR REPLICATION TEST SUITE"
echo "═══════════════════════════════════════════════════════════════════"
echo ""

check_nodes

product_id=$(test_create_replication | tail -1)

test_update_replication "$product_id"

test_cluster_health

test_bulk_write

test_read_from_followers

test_data_consistency
consistency_result=$?

echo "═══════════════════════════════════════════════════════════════════"
if [ $consistency_result -eq 0 ]; then
    echo -e "       ${GREEN}✅ ALL TESTS PASSED${NC}"
else
    echo -e "       ${RED}❌ DATA CONSISTENCY FAILED${NC}"
fi
echo "═══════════════════════════════════════════════════════════════════"
echo ""

exit $consistency_result
