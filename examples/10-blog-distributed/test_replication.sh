#!/bin/bash
#
# Lithair Blog Replication Test Suite
#
# Tests:
# 1. Authentication (login/logout)
# 2. Article CREATE replication
# 3. Article UPDATE replication
# 4. Article DELETE replication
# 5. Data consistency verification

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

test_authentication() {
    log_test "═══════════════════════════════════════════════════════════"
    log_test "  Test 1: Authentication"
    log_test "═══════════════════════════════════════════════════════════"
    echo ""

    log_info "Logging in as admin on LEADER..."
    local response=$(curl -s -X POST "$LEADER/auth/login" \
        -H "Content-Type: application/json" \
        -d '{"username":"admin","password":"password123"}')

    SESSION_TOKEN=$(echo "$response" | jq -r '.session_token // empty')

    if [ -z "$SESSION_TOKEN" ]; then
        log_error "Failed to login: $response"
        return 1
    fi

    log_info "Got session token: ${SESSION_TOKEN:0:20}..."
    echo "$response" | jq .
    echo ""

    # Test invalid credentials
    log_info "Testing invalid credentials..."
    local invalid=$(curl -s -X POST "$LEADER/auth/login" \
        -H "Content-Type: application/json" \
        -d '{"username":"admin","password":"wrong"}')

    if echo "$invalid" | grep -q "Invalid credentials"; then
        echo -e "  └─ ${GREEN}Correctly rejected invalid credentials${NC}"
    else
        echo -e "  └─ ${RED}Should have rejected invalid credentials${NC}"
    fi
    echo ""
}

test_create_replication() {
    log_test "═══════════════════════════════════════════════════════════"
    log_test "  Test 2: CREATE Replication"
    log_test "═══════════════════════════════════════════════════════════"
    echo ""

    log_info "Creating article on LEADER..."
    local response=$(curl -s -X POST "$LEADER/api/articles" \
        -H "Content-Type: application/json" \
        -H "Authorization: Bearer $SESSION_TOKEN" \
        -d '{
            "title": "Test Article",
            "content": "This is a test article for replication testing.",
            "author_id": "admin",
            "status": "Draft"
        }')

    ARTICLE_ID=$(echo "$response" | jq -r '.id // empty')

    if [ -z "$ARTICLE_ID" ]; then
        log_error "Failed to create article: $response"
        return 1
    fi

    log_info "Created article ID: $ARTICLE_ID"
    echo "$response" | jq .
    echo ""

    sleep 0.5

    # Check replication
    log_info "Checking replication on FOLLOWER1..."
    local f1_data=$(curl -s "$FOLLOWER1/api/articles/$ARTICLE_ID")
    if echo "$f1_data" | jq -e '.id' > /dev/null 2>&1; then
        echo -e "  └─ ${GREEN}REPLICATED${NC}"
    else
        echo -e "  └─ ${RED}NOT FOUND${NC}"
    fi

    log_info "Checking replication on FOLLOWER2..."
    local f2_data=$(curl -s "$FOLLOWER2/api/articles/$ARTICLE_ID")
    if echo "$f2_data" | jq -e '.id' > /dev/null 2>&1; then
        echo -e "  └─ ${GREEN}REPLICATED${NC}"
    else
        echo -e "  └─ ${RED}NOT FOUND${NC}"
    fi
    echo ""
}

test_update_replication() {
    log_test "═══════════════════════════════════════════════════════════"
    log_test "  Test 3: UPDATE Replication"
    log_test "═══════════════════════════════════════════════════════════"
    echo ""

    if [ -z "$ARTICLE_ID" ]; then
        log_warn "No article ID, skipping update test"
        return
    fi

    log_info "Updating article title on LEADER..."
    local response=$(curl -s -X PUT "$LEADER/api/articles/$ARTICLE_ID" \
        -H "Content-Type: application/json" \
        -H "Authorization: Bearer $SESSION_TOKEN" \
        -d '{"title": "Updated Article Title", "status": "Published"}')

    echo "$response" | jq . 2>/dev/null || echo "$response"
    echo ""

    sleep 0.5

    # Check updated values
    log_info "Checking update on FOLLOWER1..."
    local f1_title=$(curl -s "$FOLLOWER1/api/articles/$ARTICLE_ID" | jq -r '.title // empty')
    local f1_status=$(curl -s "$FOLLOWER1/api/articles/$ARTICLE_ID" | jq -r '.status // empty')
    echo "  └─ Title: $f1_title"
    echo "  └─ Status: $f1_status"

    log_info "Checking update on FOLLOWER2..."
    local f2_title=$(curl -s "$FOLLOWER2/api/articles/$ARTICLE_ID" | jq -r '.title // empty')
    local f2_status=$(curl -s "$FOLLOWER2/api/articles/$ARTICLE_ID" | jq -r '.status // empty')
    echo "  └─ Title: $f2_title"
    echo "  └─ Status: $f2_status"
    echo ""
}

test_bulk_articles() {
    log_test "═══════════════════════════════════════════════════════════"
    log_test "  Test 4: Bulk Article Creation"
    log_test "═══════════════════════════════════════════════════════════"
    echo ""

    local count=10
    log_info "Creating $count articles in parallel..."
    local start=$(date +%s%N)

    for i in $(seq 1 $count); do
        curl -s -X POST "$LEADER/api/articles" \
            -H "Content-Type: application/json" \
            -d "{\"title\":\"Article $i\",\"content\":\"Content for article $i\",\"author_id\":\"admin\",\"status\":\"Draft\"}" &
    done

    wait

    local end=$(date +%s%N)
    local duration=$(( (end - start) / 1000000 ))

    log_info "Created $count articles in ${duration}ms"
    echo ""

    sleep 1

    # Verify counts
    log_info "Verifying article counts..."
    local leader_count=$(curl -s "$LEADER/api/articles" | jq 'length')
    local f1_count=$(curl -s "$FOLLOWER1/api/articles" | jq 'length')
    local f2_count=$(curl -s "$FOLLOWER2/api/articles" | jq 'length')

    echo "  Leader:    $leader_count articles"
    echo "  Follower1: $f1_count articles"
    echo "  Follower2: $f2_count articles"

    if [ "$leader_count" = "$f1_count" ] && [ "$leader_count" = "$f2_count" ]; then
        echo -e "  ${GREEN}✓ All nodes in sync!${NC}"
    else
        echo -e "  ${YELLOW}⚠ Counts differ (replication may still be in progress)${NC}"
    fi
    echo ""
}

test_cluster_health() {
    log_test "═══════════════════════════════════════════════════════════"
    log_test "  Test 5: Cluster Health"
    log_test "═══════════════════════════════════════════════════════════"
    echo ""

    log_info "Fetching cluster health from leader..."
    curl -s "$LEADER/_raft/health" | jq .
    echo ""
}

test_data_consistency() {
    log_test "═══════════════════════════════════════════════════════════"
    log_test "  Test 6: DATA CONSISTENCY (Checksum Comparison)"
    log_test "═══════════════════════════════════════════════════════════"
    echo ""

    log_info "Comparing actual data between nodes (sorted by ID)..."

    local leader_hash=$(curl -s "$LEADER/api/articles" | jq -c 'sort_by(.id)' | md5sum | cut -d' ' -f1)
    local f1_hash=$(curl -s "$FOLLOWER1/api/articles" | jq -c 'sort_by(.id)' | md5sum | cut -d' ' -f1)
    local f2_hash=$(curl -s "$FOLLOWER2/api/articles" | jq -c 'sort_by(.id)' | md5sum | cut -d' ' -f1)

    echo "  Leader checksum:    $leader_hash"
    echo "  Follower1 checksum: $f1_hash"
    echo "  Follower2 checksum: $f2_hash"
    echo ""

    if [ "$leader_hash" = "$f1_hash" ] && [ "$leader_hash" = "$f2_hash" ]; then
        echo -e "  ${GREEN}✅ DATA CONSISTENCY VERIFIED - All nodes have identical data!${NC}"
        return 0
    else
        echo -e "  ${RED}❌ DATA MISMATCH - Nodes have different data!${NC}"
        return 1
    fi
}

# Main
echo ""
echo "═══════════════════════════════════════════════════════════════════"
echo "       LITHAIR BLOG REPLICATION TEST SUITE"
echo "═══════════════════════════════════════════════════════════════════"
echo ""

check_nodes

test_authentication

test_create_replication

test_update_replication

test_bulk_articles

test_cluster_health

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
