#!/bin/bash
# Schema Migration Demo - Approve Workflow Test
# Tests the full approve + disk persistence cycle
# Usage: ./test-approve.sh [port]

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

# Configuration
PORT="${1:-8090}"
DATA_DIR="./data/schema_demo"
BASELINE_DIR="examples/08-schema-migration/baseline"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Go to project root
cd "$SCRIPT_DIR/../.."

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Approve + Disk Persistence Test${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "  Port: ${YELLOW}$PORT${NC}"
echo -e "  Mode: ${YELLOW}manual${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""

# Function to cleanup
cleanup() {
    echo -e "\n${YELLOW}Cleaning up...${NC}"
    if [ ! -z "$SERVER_PID" ]; then
        kill $SERVER_PID 2>/dev/null || true
        wait $SERVER_PID 2>/dev/null || true
    fi
    lsof -ti:$PORT | xargs kill -9 2>/dev/null || true
}

trap cleanup EXIT

# Kill any existing process
echo -e "${CYAN}[Step 1]${NC} Killing any process on port $PORT..."
lsof -ti:$PORT | xargs kill -9 2>/dev/null || true
sleep 1

# Setup baseline
echo -e "${CYAN}[Step 2]${NC} Setting up baseline v1 (7 fields)..."
rm -rf "$DATA_DIR"
mkdir -p "$DATA_DIR/.schema"
cp "$BASELINE_DIR/Product_v1.json" "$DATA_DIR/.schema/Product.json"

BEFORE_COUNT=$(cat "$DATA_DIR/.schema/Product.json" | jq '.fields | keys | length')
echo -e "  Schema on disk: ${YELLOW}$BEFORE_COUNT fields${NC}"

# Build and start server
echo -e "${CYAN}[Step 3]${NC} Starting server in Manual mode..."
cargo build -p schema-migration --quiet
cargo run -q -p schema-migration --bin schema_demo -- -p $PORT -m manual 2>&1 &
SERVER_PID=$!

# Wait for server
echo -n "  Waiting for server..."
for i in {1..30}; do
    if curl -s "http://localhost:$PORT/api/products" > /dev/null 2>&1; then
        echo -e " ${GREEN}ready${NC}"
        break
    fi
    echo -n "."
    sleep 0.5
done

# Get pending changes
echo -e "${CYAN}[Step 4]${NC} Getting pending changes..."
PENDING_JSON=$(curl -s "http://localhost:$PORT/_admin/schema/pending")
PENDING_COUNT=$(echo "$PENDING_JSON" | jq '.count')
PENDING_ID=$(echo "$PENDING_JSON" | jq -r '.pending_changes[0].id')

if [ "$PENDING_COUNT" = "0" ] || [ "$PENDING_ID" = "null" ]; then
    echo -e "  ${RED}No pending changes found!${NC}"
    echo -e "  ${RED}Make sure server is in Manual mode${NC}"
    exit 1
fi

echo -e "  Pending changes: ${YELLOW}$PENDING_COUNT${NC}"
echo -e "  Pending ID: ${YELLOW}$PENDING_ID${NC}"

# Show changes
echo -e "${CYAN}[Step 5]${NC} Changes detected:"
echo "$PENDING_JSON" | jq -r '.pending_changes[0].changes[] | "  - \(.type) on \(.field) (\(.strategy))"'

# Approve
echo -e "${CYAN}[Step 6]${NC} Approving schema change..."
APPROVE_RESULT=$(curl -s -X POST "http://localhost:$PORT/_admin/schema/approve/$PENDING_ID")
APPROVE_STATUS=$(echo "$APPROVE_RESULT" | jq -r '.status')

if [ "$APPROVE_STATUS" = "applied" ]; then
    echo -e "  ${GREEN}✓ Schema change approved and applied${NC}"
else
    echo -e "  ${RED}✗ Approve failed: $APPROVE_STATUS${NC}"
    echo "$APPROVE_RESULT" | jq .
    exit 1
fi

# Verify disk persistence
echo -e "${CYAN}[Step 7]${NC} Verifying disk persistence..."
AFTER_COUNT=$(cat "$DATA_DIR/.schema/Product.json" | jq '.fields | keys | length')

echo -e "  Before approve: ${YELLOW}$BEFORE_COUNT fields${NC}"
echo -e "  After approve:  ${YELLOW}$AFTER_COUNT fields${NC}"

if [ "$AFTER_COUNT" = "10" ]; then
    echo ""
    echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${GREEN}  ✅ TEST PASSED${NC}"
    echo -e "${GREEN}  Schema persisted to disk (7 → 10 fields)${NC}"
    echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
else
    echo ""
    echo -e "${RED}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${RED}  ❌ TEST FAILED${NC}"
    echo -e "${RED}  Expected 10 fields, got $AFTER_COUNT${NC}"
    echo -e "${RED}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    exit 1
fi

# Show final schema fields
echo ""
echo -e "${CYAN}Final schema fields:${NC}"
cat "$DATA_DIR/.schema/Product.json" | jq -r '.fields | keys[]' | while read field; do
    echo -e "  - $field"
done
