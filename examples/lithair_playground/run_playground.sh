#!/bin/bash
# Lithair Playground - Cluster Management Script
#
# Usage:
#   ./run_playground.sh start   - Start 3-node cluster
#   ./run_playground.sh stop    - Stop all nodes
#   ./run_playground.sh clean   - Clean data directories
#   ./run_playground.sh restart - Stop, clean, and start
#   ./run_playground.sh status  - Check cluster status

set -e

ACTION=${1:-start}
DATA_DIR="${PLAYGROUND_DATA_BASE:-./data}"
LOG_DIR="./logs"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

print_header() {
    echo -e "${BLUE}"
    echo "═══════════════════════════════════════════════════════════════════"
    echo "  LITHAIR PLAYGROUND - Interactive Showcase"
    echo "═══════════════════════════════════════════════════════════════════"
    echo -e "${NC}"
}

start_cluster() {
    print_header
    echo -e "${GREEN}Starting 3-node cluster...${NC}"
    echo ""

    mkdir -p "$DATA_DIR"
    mkdir -p "$LOG_DIR"

    # Build first
    echo -e "${YELLOW}Building playground_node...${NC}"
    cargo build --release --bin playground_node 2>&1 | tail -5

    echo ""
    echo -e "${GREEN}Launching nodes...${NC}"

    # Node 0 (initial leader)
    echo -e "  ${BLUE}Starting Node 0 on port 8080...${NC}"
    PLAYGROUND_DATA_BASE="$DATA_DIR" cargo run --release --bin playground_node -- \
        --node-id 0 --port 8080 --peers 8081,8082 \
        > "$LOG_DIR/node_0.log" 2>&1 &
    echo $! > "$DATA_DIR/node_0.pid"

    sleep 1

    # Node 1
    echo -e "  ${BLUE}Starting Node 1 on port 8081...${NC}"
    PLAYGROUND_DATA_BASE="$DATA_DIR" cargo run --release --bin playground_node -- \
        --node-id 1 --port 8081 --peers 8080,8082 \
        > "$LOG_DIR/node_1.log" 2>&1 &
    echo $! > "$DATA_DIR/node_1.pid"

    sleep 1

    # Node 2
    echo -e "  ${BLUE}Starting Node 2 on port 8082...${NC}"
    PLAYGROUND_DATA_BASE="$DATA_DIR" cargo run --release --bin playground_node -- \
        --node-id 2 --port 8082 --peers 8080,8081 \
        > "$LOG_DIR/node_2.log" 2>&1 &
    echo $! > "$DATA_DIR/node_2.pid"

    echo ""
    echo -e "${GREEN}Cluster started!${NC}"
    echo ""
    echo "  Nodes:"
    echo -e "    ${BLUE}Node 0:${NC} http://localhost:8080"
    echo -e "    ${BLUE}Node 1:${NC} http://localhost:8081"
    echo -e "    ${BLUE}Node 2:${NC} http://localhost:8082"
    echo ""
    echo "  Logs:"
    echo "    tail -f $LOG_DIR/node_0.log"
    echo "    tail -f $LOG_DIR/node_1.log"
    echo "    tail -f $LOG_DIR/node_2.log"
    echo ""
    echo -e "${GREEN}Open http://localhost:8080 for the Playground UI${NC}"
    echo ""

    # Wait a bit and check health
    sleep 3
    echo -e "${YELLOW}Checking cluster health...${NC}"
    curl -s http://localhost:8080/_raft/health | jq . 2>/dev/null || echo "Waiting for cluster to initialize..."
}

stop_cluster() {
    print_header
    echo -e "${YELLOW}Stopping Lithair Playground...${NC}"

    # Kill by PID files
    for i in 0 1 2; do
        if [ -f "$DATA_DIR/node_$i.pid" ]; then
            pid=$(cat "$DATA_DIR/node_$i.pid")
            if kill -0 "$pid" 2>/dev/null; then
                echo -e "  Stopping Node $i (PID: $pid)..."
                kill "$pid" 2>/dev/null || true
            fi
            rm -f "$DATA_DIR/node_$i.pid"
        fi
    done

    # Also try pkill as fallback
    pkill -f "playground_node" 2>/dev/null || true

    echo -e "${GREEN}Cluster stopped.${NC}"
}

clean_data() {
    print_header
    echo -e "${YELLOW}Cleaning data directories...${NC}"

    rm -rf "$DATA_DIR"
    rm -rf "$LOG_DIR"

    echo -e "${GREEN}Data cleaned.${NC}"
}

check_status() {
    print_header
    echo -e "${BLUE}Checking cluster status...${NC}"
    echo ""

    for port in 8080 8081 8082; do
        echo -e "${YELLOW}Node on port $port:${NC}"
        curl -s "http://localhost:$port/_raft/health" | jq . 2>/dev/null || echo -e "  ${RED}Not responding${NC}"
        echo ""
    done
}

case $ACTION in
    start)
        start_cluster
        ;;

    stop)
        stop_cluster
        ;;

    clean)
        stop_cluster
        clean_data
        ;;

    restart)
        stop_cluster
        clean_data
        sleep 1
        start_cluster
        ;;

    status)
        check_status
        ;;

    *)
        echo "Usage: $0 {start|stop|clean|restart|status}"
        echo ""
        echo "Commands:"
        echo "  start   - Start 3-node cluster"
        echo "  stop    - Stop all nodes"
        echo "  clean   - Stop and clean data directories"
        echo "  restart - Stop, clean, and start fresh"
        echo "  status  - Check cluster health"
        exit 1
        ;;
esac
