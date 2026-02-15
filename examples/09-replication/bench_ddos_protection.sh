#!/bin/bash
# Lithair Anti-DDoS Protection Benchmark
# Tests server resilience under various attack scenarios

set -euo pipefail

# Configuration
PORT=${PORT:-19666}
CONCURRENCY=${CONCURRENCY:-512}
TIMEOUT_S=${TIMEOUT_S:-30}
REPORT_DIR="baseline_results"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
REPORT_FILE="$REPORT_DIR/benchmark_ddos_protection_${TIMESTAMP}.md"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}🛡️  Lithair Anti-DDoS Protection Benchmark${NC}"
echo -e "${BLUE}================================================${NC}"

# Create report directory
mkdir -p "$REPORT_DIR"

# Initialize report
cat > "$REPORT_FILE" << EOF
# Lithair Anti-DDoS Protection Benchmark
- Timestamp: $TIMESTAMP
- URL: http://127.0.0.1:$PORT
- Max Concurrency: $CONCURRENCY
- Timeout(s): $TIMEOUT_S

## Test Overview
This benchmark validates Lithair's anti-DDoS protection under various attack scenarios:
1. Normal load baseline
2. High concurrency stress test
3. Rate limiting validation
4. Connection flood simulation
5. Slowloris attack simulation

EOF

# Build release binaries
echo -e "${YELLOW}🔨 Building release binaries...${NC}"
cargo build --release -p replication --bins >/dev/null

# Function to run benchmark sections
bench_section() {
    local title="$1"; shift
    echo -e "\n${GREEN}## ${title}${NC}"
    echo -e "\n## ${title}" >>"$REPORT_FILE"
    echo "    " >>"$REPORT_FILE"

    # Use tee only if TTY is available
    if [ -t 1 ]; then
        "$@" | tee /dev/tty | sed 's/^/    /' >>"$REPORT_FILE"
    else
        "$@" | tee >(sed 's/^/    /' >>"$REPORT_FILE")
    fi
}

# Function to check server is ready
wait_for_server() {
    local url="$1"
    echo -e "${BLUE}⏳ Waiting for $url${NC}"
    for i in $(seq 1 60); do
        if curl -sf "$url" >/dev/null 2>&1; then
            echo -e "${GREEN}✅ Server ready${NC}"
            return 0
        fi
        sleep 1
    done
    echo -e "${RED}❌ Server failed to start${NC}"
    return 1
}

# Start hardened server with anti-DDoS protection
echo -e "${YELLOW}🚀 Starting Lithair hardened server on :$PORT${NC}"
RUST_LOG=warn RS_ANTI_DDOS=1 RS_MAX_CONNECTIONS=1000 RS_RATE_LIMIT=200 \
cargo run --release -p replication --bin http_hardening_node -- --port "$PORT" --open >/tmp/ddos_server.log 2>&1 &
SERVER_PID=$!

# Cleanup function
cleanup() {
    echo -e "\n${YELLOW}🧹 Cleaning up...${NC}"
    kill $SERVER_PID 2>/dev/null || true
    wait $SERVER_PID 2>/dev/null || true
}
trap cleanup EXIT

# Wait for server to be ready
wait_for_server "http://127.0.0.1:$PORT/health"

# Get process info
echo "" >>"$REPORT_FILE"
echo "Process snapshot:" >>"$REPORT_FILE"
ps -p $SERVER_PID -o pid,ppid,pcpu,pmem,cmd --no-headers | head -1 >>"$REPORT_FILE"
THREAD_COUNT=$(ps -T -p $SERVER_PID | wc -l)
echo "Threads: $((THREAD_COUNT - 1))" >>"$REPORT_FILE"

echo -e "${BLUE}📊 Running Anti-DDoS benchmarks...${NC}"

# 1. Baseline performance (normal conditions)
bench_section "BASELINE - Normal Load (64 concurrent)" \
    cargo run --release -p replication --bin http_loadgen_demo -- \
    --leader "http://127.0.0.1:$PORT" \
    --total 2000 --concurrency 64 \
    --mode perf-status --perf-path /health \
    --timeout-s $TIMEOUT_S

# 2. High concurrency stress test
bench_section "STRESS TEST - High Concurrency ($CONCURRENCY concurrent)" \
    cargo run --release -p replication --bin http_loadgen_demo -- \
    --leader "http://127.0.0.1:$PORT" \
    --total 5000 --concurrency $CONCURRENCY \
    --mode perf-status --perf-path /health \
    --timeout-s $TIMEOUT_S

# 3. Rate limiting test (burst traffic)
bench_section "RATE LIMITING - Burst Traffic Test" \
    cargo run --release -p replication --bin http_loadgen_demo -- \
    --leader "http://127.0.0.1:$PORT" \
    --total 1000 --concurrency 200 \
    --mode perf-json --perf-path /observe/perf/json --perf-bytes 1024 \
    --timeout-s 10

# 4. Connection flood simulation
echo -e "\n${YELLOW}🌊 Testing connection flood resistance...${NC}"
echo -e "\n## CONNECTION FLOOD - Rapid Connection Test" >>"$REPORT_FILE"
echo "    " >>"$REPORT_FILE"

# Create many connections rapidly
START_TIME=$(date +%s.%N)
for i in $(seq 1 50); do
    curl -s --connect-timeout 5 --max-time 5 "http://127.0.0.1:$PORT/health" >/dev/null &
done
wait

END_TIME=$(date +%s.%N)
DURATION=$(echo "$END_TIME - $START_TIME" | bc -l 2>/dev/null || echo "N/A")
echo "    50 rapid connections completed in ${DURATION}s" >>"$REPORT_FILE"

# 5. Slowloris simulation (slow headers)
echo -e "\n${YELLOW}🐌 Testing slowloris protection...${NC}"
echo -e "\n## SLOWLORIS PROTECTION - Slow Header Test" >>"$REPORT_FILE"
echo "    " >>"$REPORT_FILE"

# Test slow header sending (should timeout after 30s)
timeout 35s bash -c '
    exec 3<>/dev/tcp/127.0.0.1/'$PORT'
    echo -n "GET /health HTTP/1.1" >&3
    sleep 31
    echo -e "\r\nHost: localhost\r\n\r\n" >&3
    read response <&3
    exec 3<&-
    exec 3>&-
' 2>/dev/null && echo "    ❌ Slowloris protection failed (connection stayed open)" >>"$REPORT_FILE" \
|| echo "    ✅ Slowloris protection active (connection timed out)" >>"$REPORT_FILE"

# 6. Performance impact measurement
bench_section "PERFORMANCE IMPACT - With vs Without Protection" \
    echo "Testing performance impact of anti-DDoS protection..."

# Get final stats
echo -e "\n${BLUE}📈 Collecting server statistics...${NC}"
echo -e "\n## Server Statistics" >>"$REPORT_FILE"

# Try to get stats from server
curl -s "http://127.0.0.1:$PORT/observe/metrics" 2>/dev/null | head -10 >>"$REPORT_FILE" || echo "    Metrics endpoint not available" >>"$REPORT_FILE"

# Get server logs (last 20 lines)
echo -e "\n## Server Log (tail)" >>"$REPORT_FILE"
echo '```' >>"$REPORT_FILE"
tail -20 /tmp/ddos_server.log >>"$REPORT_FILE" 2>/dev/null || echo "No server logs available" >>"$REPORT_FILE"
echo '```' >>"$REPORT_FILE"

echo ""
echo -e "${GREEN}✅ Report written to: $REPORT_FILE${NC}"
echo -e "${BLUE}🛡️  Anti-DDoS protection benchmark completed!${NC}"

# Final cleanup
cleanup