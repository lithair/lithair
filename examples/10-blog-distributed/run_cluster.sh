#!/bin/bash
#
# Lithair Blog Replicated Cluster Manager
#
# Usage:
#   ./run_cluster.sh start    - Build and start 3-node cluster
#   ./run_cluster.sh stop     - Stop all nodes
#   ./run_cluster.sh status   - Check cluster status
#   ./run_cluster.sh clean    - Stop and delete data
#   ./run_cluster.sh restart  - Stop then start
#
# Custom data directory:
#   EXPERIMENT_DATA_BASE=/path ./run_cluster.sh start

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Configuration
BINARY_NAME="blog_replicated_node"
DATA_DIR="${EXPERIMENT_DATA_BASE:-data}"
LOG_DIR="/tmp/lithair_blog_cluster_logs"
PORTS=(8080 8081 8082)
PIDS_FILE="/tmp/lithair_blog_cluster_pids"

log_info() { echo -e "${GREEN}[INFO]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }

get_peers() {
    local node_id=$1
    local peers=""
    for i in "${!PORTS[@]}"; do
        if [ "$i" != "$node_id" ]; then
            [ -n "$peers" ] && peers="$peers,"
            peers="$peers${PORTS[$i]}"
        fi
    done
    echo "$peers"
}

build() {
    log_info "Building $BINARY_NAME..."
    cargo build --release --bin "$BINARY_NAME" -p blog-distributed 2>&1 | tail -5
    log_info "Build complete!"
}

start_cluster() {
    stop_cluster 2>/dev/null || true

    build

    mkdir -p "$LOG_DIR"
    mkdir -p "$DATA_DIR"

    echo ""
    log_info "═══════════════════════════════════════════════════════════"
    log_info "  Starting Lithair Blog 3-Node Cluster"
    log_info "  Data directory: $DATA_DIR"
    log_info "═══════════════════════════════════════════════════════════"
    echo ""

    > "$PIDS_FILE"

    for i in "${!PORTS[@]}"; do
        local port=${PORTS[$i]}
        local peers=$(get_peers $i)
        local log_file="$LOG_DIR/node_$i.log"

        log_info "Starting Node $i on port $port (peers: $peers)"

        EXPERIMENT_DATA_BASE="$DATA_DIR" \
        RUST_LOG=info \
        ../../target/release/$BINARY_NAME \
            --node-id "$i" \
            --port "$port" \
            --peers "$peers" \
            > "$log_file" 2>&1 &

        local pid=$!
        echo "$pid" >> "$PIDS_FILE"
        log_info "  └─ PID: $pid, Log: $log_file"
    done

    echo ""
    log_info "═══════════════════════════════════════════════════════════"
    log_info "  Blog Cluster Started!"
    log_info "═══════════════════════════════════════════════════════════"
    echo ""
    log_info "Leader:    http://localhost:8080"
    log_info "Follower1: http://localhost:8081"
    log_info "Follower2: http://localhost:8082"
    echo ""
    log_info "Health:    curl http://localhost:8080/_raft/health | jq"
    log_info "Logs:      tail -f $LOG_DIR/node_*.log"
    echo ""

    # Warmup
    log_info "Warming up cluster..."
    sleep 2

    # Create initial article to establish replication
    local response=$(curl -s -X POST http://localhost:8080/api/articles \
        -H "Content-Type: application/json" \
        -d '{"title":"Welcome","content":"First replicated article!","author_id":"system","status":"Published"}' 2>/dev/null || echo "")

    sleep 1

    # Check health
    local health=$(curl -s http://localhost:8080/_raft/health 2>/dev/null || echo "")
    if echo "$health" | grep -q '"health".*"healthy"'; then
        log_info "Cluster healthy and synchronized!"
    else
        log_warn "Cluster may still be synchronizing..."
    fi

    echo ""
    log_info "Demo Users:"
    log_info "  admin/password123       -> Admin (full access)"
    log_info "  reporter/password123    -> Reporter (can publish)"
    log_info "  contributor/password123 -> Contributor (own articles)"
    echo ""
    log_info "Login:  curl -X POST http://localhost:8080/auth/login -H 'Content-Type: application/json' -d '{\"username\":\"admin\",\"password\":\"password123\"}'"
    echo ""
    log_info "Stop with: ./run_cluster.sh stop"
}

stop_cluster() {
    log_info "Stopping cluster..."

    if [ -f "$PIDS_FILE" ]; then
        while read -r pid; do
            if kill -0 "$pid" 2>/dev/null; then
                log_info "Stopping PID $pid"
                kill "$pid" 2>/dev/null || true
            fi
        done < "$PIDS_FILE"
        rm -f "$PIDS_FILE"
    fi

    # Also kill any remaining processes
    pkill -f "$BINARY_NAME" 2>/dev/null || true

    log_info "Cluster stopped"
}

cluster_status() {
    echo ""
    log_info "═══════════════════════════════════════════════════════════"
    log_info "  Blog Cluster Status"
    log_info "═══════════════════════════════════════════════════════════"
    echo ""

    for i in "${!PORTS[@]}"; do
        local port=${PORTS[$i]}
        local role="Follower"
        [ "$i" -eq 0 ] && role="Leader"

        echo -n "Node $i ($role) port $port: "
        if curl -s --connect-timeout 1 "http://localhost:$port/status" > /dev/null 2>&1; then
            echo -e "${GREEN}UP${NC}"

            local count=$(curl -s "http://localhost:$port/api/articles" 2>/dev/null | jq 'length' 2>/dev/null || echo "?")
            echo "  └─ Articles: $count"
        else
            echo -e "${RED}DOWN${NC}"
        fi
    done

    echo ""
    log_info "Leader Health:"
    curl -s http://localhost:8080/_raft/health 2>/dev/null | jq . || echo "  Leader not responding"
    echo ""
}

clean_cluster() {
    stop_cluster

    log_info "Cleaning data directories..."
    rm -rf "$DATA_DIR"/blog_node_*
    rm -rf "$LOG_DIR"

    log_info "Clean complete"
}

# Main
case "${1:-}" in
    start)
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
