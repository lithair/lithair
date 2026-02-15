#!/bin/bash
# Schema Migration Demo - Test Runner
# Usage: ./run-tests.sh [mode] [port]
#   mode: warn (default), manual, strict, auto
#   port: 8090 (default)

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
MODE="${1:-warn}"
PORT="${2:-8090}"
DATA_DIR="./data/schema_demo"
BASELINE_DIR="examples/08-schema-migration/baseline"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Go to project root
cd "$SCRIPT_DIR/../.."

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Schema Migration Demo - Test Runner${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "  Mode: ${YELLOW}$MODE${NC}"
echo -e "  Port: ${YELLOW}$PORT${NC}"
echo -e "  Data: ${YELLOW}$DATA_DIR${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""

# Function to cleanup
cleanup() {
    echo -e "\n${YELLOW}Cleaning up...${NC}"
    if [ ! -z "$SERVER_PID" ]; then
        kill $SERVER_PID 2>/dev/null || true
        wait $SERVER_PID 2>/dev/null || true
    fi
    # Kill any process on the port
    lsof -ti:$PORT | xargs kill -9 2>/dev/null || true
}

# Set trap for cleanup
trap cleanup EXIT

# Kill any existing process on the port
echo -e "${YELLOW}[1/5]${NC} Killing any process on port $PORT..."
lsof -ti:$PORT | xargs kill -9 2>/dev/null || true
sleep 1

# Setup baseline schema
echo -e "${YELLOW}[2/5]${NC} Setting up baseline schema (v1 - 7 fields)..."
rm -rf "$DATA_DIR"
mkdir -p "$DATA_DIR/.schema"
cp "$BASELINE_DIR/Product_v1.json" "$DATA_DIR/.schema/Product.json"
echo -e "  ${GREEN}✓${NC} Baseline schema copied"

# Build the demo
echo -e "${YELLOW}[3/5]${NC} Building schema-migration..."
cargo build -p schema-migration --quiet
echo -e "  ${GREEN}✓${NC} Build complete"

# Start server in background
echo -e "${YELLOW}[4/5]${NC} Starting server (mode: $MODE, port: $PORT)..."
cargo run -q -p schema-migration --bin schema_demo -- -p $PORT -m $MODE 2>&1 &
SERVER_PID=$!
echo -e "  ${GREEN}✓${NC} Server started (PID: $SERVER_PID)"

# Wait for server to be ready
echo -n "  Waiting for server..."
for i in {1..30}; do
    if curl -s "http://localhost:$PORT/api/products" > /dev/null 2>&1; then
        echo -e " ${GREEN}ready${NC}"
        break
    fi
    echo -n "."
    sleep 0.5
done

# Check if server is ready
if ! curl -s "http://localhost:$PORT/api/products" > /dev/null 2>&1; then
    echo -e " ${RED}failed${NC}"
    echo -e "${RED}Server failed to start!${NC}"
    exit 1
fi

# Run tests
echo -e "${YELLOW}[5/5]${NC} Running tests..."
echo ""
cargo run -q -p schema-migration --bin schema_demo -- -p $PORT --test

# Show final status
echo ""
if [ "$MODE" = "manual" ]; then
    echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${GREEN}  Mode Manual - Test 16 should pass${NC}"
    echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
else
    echo -e "${YELLOW}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${YELLOW}  Mode $MODE - Test 16 skipped (requires -m manual)${NC}"
    echo -e "${YELLOW}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
fi
