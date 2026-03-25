#!/bin/bash
# Schema Migration Demo - Show Status
# Shows current schema state, pending changes, and server status
# Usage: ./show-status.sh [port]

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

PORT="${1:-8090}"
DATA_DIR="./data/schema_demo"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

cd "$SCRIPT_DIR/../.."

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Schema Migration Demo - Status${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

# Check server status
echo -e "\n${CYAN}Server Status:${NC}"
if curl -s "http://localhost:$PORT/api/products" > /dev/null 2>&1; then
    echo -e "  ${GREEN}● Running${NC} on port $PORT"
else
    echo -e "  ${RED}○ Not running${NC} on port $PORT"
fi

# Check schema on disk
echo -e "\n${CYAN}Schema on Disk:${NC}"
if [ -f "$DATA_DIR/.schema/Product.json" ]; then
    FIELD_COUNT=$(cat "$DATA_DIR/.schema/Product.json" | jq '.fields | keys | length')
    VERSION=$(cat "$DATA_DIR/.schema/Product.json" | jq '.version')
    echo -e "  Version: ${YELLOW}$VERSION${NC}"
    echo -e "  Fields:  ${YELLOW}$FIELD_COUNT${NC}"
    echo -e "  Path:    ${YELLOW}$DATA_DIR/.schema/Product.json${NC}"
    echo ""
    echo -e "  Field list:"
    cat "$DATA_DIR/.schema/Product.json" | jq -r '.fields | keys[]' | while read field; do
        echo -e "    - $field"
    done
else
    echo -e "  ${RED}No schema file found${NC}"
    echo -e "  Path: $DATA_DIR/.schema/Product.json"
fi

# Check pending changes (if server is running)
if curl -s "http://localhost:$PORT/api/products" > /dev/null 2>&1; then
    echo -e "\n${CYAN}Pending Changes:${NC}"
    PENDING=$(curl -s "http://localhost:$PORT/_admin/schema/pending")
    COUNT=$(echo "$PENDING" | jq '.count')

    if [ "$COUNT" = "0" ]; then
        echo -e "  ${GREEN}No pending changes${NC}"
    else
        echo -e "  ${YELLOW}$COUNT pending change(s)${NC}"
        echo "$PENDING" | jq -r '.pending_changes[] | "  ID: \(.id)\n  Model: \(.model_name)\n  Changes: \(.changes | length)"'
    fi

    # Check lock status
    echo -e "\n${CYAN}Lock Status:${NC}"
    LOCK=$(curl -s "http://localhost:$PORT/_admin/schema/lock")
    LOCKED=$(echo "$LOCK" | jq '.locked')

    if [ "$LOCKED" = "true" ]; then
        REASON=$(echo "$LOCK" | jq -r '.reason // "No reason"')
        echo -e "  ${RED}🔒 LOCKED${NC}: $REASON"
    else
        echo -e "  ${GREEN}🔓 Unlocked${NC}"
    fi

    # Check history
    echo -e "\n${CYAN}History:${NC}"
    HISTORY=$(curl -s "http://localhost:$PORT/_admin/schema/history")
    HISTORY_COUNT=$(echo "$HISTORY" | jq '.count')
    echo -e "  ${YELLOW}$HISTORY_COUNT${NC} change(s) in history"
fi

echo -e "\n${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
