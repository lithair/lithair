#!/bin/bash
#
# Lithair Cluster Launcher - Spawns a 3-node cluster on localhost
#
# Usage:
#   ./run_cluster.sh          # Start 3-node cluster
#   ./run_cluster.sh stop     # Stop all nodes
#   ./run_cluster.sh status   # Check cluster status
#   ./run_cluster.sh clean    # Clean data and stop
#

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/../.."  # Go to workspace root

# Configuration
PORTS=(8080 8081 8082)
PIDS_FILE="/tmp/lithair_cluster_pids"
LOG_DIR="/tmp/lithair_cluster_logs"
DATA_DIR="${EXPERIMENT_DATA_BASE:-data}"  # Default to ./data if not set

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

log_info() { echo -e "${GREEN}[INFO]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }

build_binary() {
    log_info "Building pure_declarative_node..."
    cargo build --release --bin pure_declarative_node -p replication 2>&1 | tail -5
    log_info "Build complete!"
}

start_cluster() {
    mkdir -p "$LOG_DIR"
    > "$PIDS_FILE"

    # Build first
    build_binary

    echo ""
    log_info "═══════════════════════════════════════════════════════════"
    log_info "  Starting Lithair 3-Node Cluster"
    log_info "  Data directory: $DATA_DIR"
    log_info "═══════════════════════════════════════════════════════════"
    echo ""

    for i in 0 1 2; do
        local port=${PORTS[$i]}
        local peers=""

        # Build peers list (all ports except current)
        for j in 0 1 2; do
            if [ $j -ne $i ]; then
                if [ -n "$peers" ]; then
                    peers="$peers,${PORTS[$j]}"
                else
                    peers="${PORTS[$j]}"
                fi
            fi
        done

        local log_file="$LOG_DIR/node_$i.log"

        log_info "Starting Node $i on port $port (peers: $peers)"

        RUST_LOG=info EXPERIMENT_DATA_BASE="$DATA_DIR" ./target/release/pure_declarative_node \
            --node-id $i \
            --port $port \
            --peers $peers \
            > "$log_file" 2>&1 &

        local pid=$!
        echo "$pid" >> "$PIDS_FILE"

        log_info "  └─ PID: $pid, Log: $log_file"

        # Small delay to let nodes start in order (leader first)
        sleep 0.5
    done

    echo ""
    log_info "═══════════════════════════════════════════════════════════"
    log_info "  Cluster Started!"
    log_info "═══════════════════════════════════════════════════════════"
    echo ""
    log_info "Leader:    http://localhost:8080"
    log_info "Follower1: http://localhost:8081"
    log_info "Follower2: http://localhost:8082"
    echo ""
    log_info "Health:    curl http://localhost:8080/_raft/health | jq"
    log_info "Logs:      tail -f $LOG_DIR/node_*.log"
    echo ""

    # Warmup: create initial data to establish cluster health
    log_info "Warming up cluster..."
    sleep 2
    curl -s -X POST "http://localhost:8080/api/products" \
        -H "Content-Type: application/json" \
        -d '{"name":"Cluster Init","price":0.01,"category":"System"}' > /dev/null 2>&1
    sleep 1  # Wait for replication

    if curl -s "http://localhost:8080/_raft/health" | grep -q '"health".*"healthy"'; then
        log_info "✅ Cluster healthy and synchronized!"
    else
        log_warn "Cluster warming up, run './run_cluster.sh status' to check"
    fi
    echo ""
    log_info "Stop with: $0 stop"
    echo ""
}

stop_cluster() {
    log_info "Stopping cluster..."

    if [ -f "$PIDS_FILE" ]; then
        while read pid; do
            if kill -0 "$pid" 2>/dev/null; then
                log_info "Stopping PID $pid"
                kill "$pid" 2>/dev/null || true
            fi
        done < "$PIDS_FILE"
        rm -f "$PIDS_FILE"
    fi

    # Also kill any orphaned processes
    pkill -f "pure_declarative_node" 2>/dev/null || true

    log_info "Cluster stopped"
}

cluster_status() {
    echo ""
    log_info "═══════════════════════════════════════════════════════════"
    log_info "  Lithair Cluster Status"
    log_info "═══════════════════════════════════════════════════════════"
    echo ""

    for i in 0 1 2; do
        local port=${PORTS[$i]}
        local status_url="http://localhost:$port/status"
        local health_url="http://localhost:$port/_raft/health"

        echo -n "Node $i (port $port): "

        if curl -s --connect-timeout 1 "$status_url" > /dev/null 2>&1; then
            local is_leader=$(curl -s "$status_url" 2>/dev/null | jq -r '.raft.is_leader // false')
            if [ "$is_leader" = "true" ]; then
                echo -e "${GREEN}LEADER${NC}"
            else
                echo -e "${BLUE}FOLLOWER${NC}"
            fi
        else
            echo -e "${RED}DOWN${NC}"
        fi
    done

    echo ""

    # Try to get health from leader
    log_info "Cluster Health (from leader):"
    curl -s "http://localhost:8080/_raft/health" 2>/dev/null | jq . || log_warn "Could not fetch health"
    echo ""
}

clean_cluster() {
    stop_cluster

    log_info "Cleaning data directories..."
    rm -rf "$DATA_DIR/pure_node_"*
    rm -rf "$DATA_DIR/raft/node_"*
    rm -rf "$LOG_DIR"

    log_info "Clean complete"
}

# Main
case "${1:-start}" in
    start)
        stop_cluster 2>/dev/null || true
        start_cluster
        ;;
    stop)
        stop_cluster
        ;;
    status)
        cluster_status
        ;;
    clean)
        clean_cluster
        ;;
    restart)
        stop_cluster
        sleep 1
        start_cluster
        ;;
    *)
        echo "Usage: $0 {start|stop|status|clean|restart}"
        exit 1
        ;;
esac
