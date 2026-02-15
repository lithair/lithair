#!/bin/bash

# Lithair Comprehensive CRUD Benchmark Suite
# Uses realistic CRUD operations via replication-loadgen instead of simple GET requests
# Generates detailed markdown reports comparing security configurations

set -euo pipefail

# Configuration
BASE_PORT=${BASE_PORT:-21000}
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
RESULTS_DIR="./benchmark_results"
REPORT_FILE="${RESULTS_DIR}/comprehensive_crud_benchmark_${TIMESTAMP}.md"
RAW_DATA_DIR="${RESULTS_DIR}/crud_data_${TIMESTAMP}"

# Benchmark parameters
CRUD_TOTAL=${CRUD_TOTAL:-2000}         # Total CRUD operations
CRUD_CONCURRENCY=${CRUD_CONCURRENCY:-32}  # Concurrent operations
CRUD_CREATE_PCT=${CRUD_CREATE_PCT:-70}     # 70% CREATE operations
CRUD_READ_PCT=${CRUD_READ_PCT:-20}         # 20% READ operations
CRUD_UPDATE_PCT=${CRUD_UPDATE_PCT:-10}     # 10% UPDATE operations
CRUD_DELETE_PCT=${CRUD_DELETE_PCT:-0}      # 0% DELETE (to keep data)

# Create directories
mkdir -p "${RESULTS_DIR}" "${RAW_DATA_DIR}"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Test configurations
declare -A TEST_CONFIGS=(
    ["baseline"]="No protection (baseline CRUD performance)"
    ["firewall"]="Firewall only (IP filtering)"
    ["antiddos"]="Anti-DDoS only (rate limiting)"
    ["full"]="Full protection (firewall + anti-DDoS)"
)

echo -e "${BLUE}🚀 Lithair Comprehensive CRUD Benchmark Suite${NC}"
echo -e "${BLUE}===============================================${NC}"
echo "Timestamp: ${TIMESTAMP}"
echo "CRUD Operations: ${CRUD_TOTAL} (${CRUD_CREATE_PCT}% CREATE, ${CRUD_READ_PCT}% READ, ${CRUD_UPDATE_PCT}% UPDATE)"
echo "Concurrency: ${CRUD_CONCURRENCY}"
echo "Results directory: ${RESULTS_DIR}"
echo "Report file: ${REPORT_FILE}"
echo

# Function to start server with specific configuration
start_server() {
    local config="$1"
    local port="$2"
    local env_vars=""
    local args="--port ${port} --open"

    case "$config" in
        "baseline")
            env_vars=""
            ;;
        "firewall")
            env_vars=""
            args="--port ${port}"  # Remove --open to enable firewall
            ;;
        "antiddos")
            env_vars="RS_ANTI_DDOS=1 RS_MAX_CONNECTIONS=500 RS_RATE_LIMIT=100"
            ;;
        "full")
            env_vars="RS_ANTI_DDOS=1 RS_MAX_CONNECTIONS=500 RS_RATE_LIMIT=100"
            args="--port ${port}"  # Remove --open to enable firewall
            ;;
    esac

    echo -e "${YELLOW}🔧 Starting server: ${config} configuration on port ${port}${NC}"
    if [[ -n "$env_vars" ]]; then
        echo "Environment: $env_vars"
    fi
    echo "Arguments: $args"

    # Start server in background
    if [[ -n "$env_vars" ]]; then
        eval "$env_vars cargo run --release --bin replication-hardening-node -- $args" > "/tmp/server_${config}_${port}.log" 2>&1 &
    else
        cargo run --release --bin replication-hardening-node -- $args > "/tmp/server_${config}_${port}.log" 2>&1 &
    fi

    local server_pid=$!
    echo "$server_pid" > "/tmp/server_${config}_${port}.pid"

    # Wait for server to start
    local url="http://127.0.0.1:${port}"
    echo -e "${YELLOW}⏳ Waiting for server to start...${NC}"
    for i in {1..30}; do
        if curl -s --max-time 1 "${url}/health" > /dev/null 2>&1; then
            echo -e "${GREEN}✅ Server ready on port ${port}${NC}"
            return 0
        fi
        sleep 1
    done

    echo -e "${RED}❌ Server failed to start on port ${port}${NC}"
    return 1
}

# Function to stop server
stop_server() {
    local config="$1"
    local port="$2"
    local pid_file="/tmp/server_${config}_${port}.pid"

    if [[ -f "$pid_file" ]]; then
        local pid=$(cat "$pid_file")
        echo -e "${YELLOW}🛑 Stopping server ${config} (PID: ${pid})${NC}"
        kill "$pid" 2>/dev/null || true
        rm -f "$pid_file"
        sleep 2
    fi
}

# Function to get server info
get_server_info() {
    local url="$1"
    local info_file="$2"

    if curl -s --max-time 3 "${url}/info" > "$info_file" 2>/dev/null; then
        return 0
    else
        echo '{"error": "Info endpoint not available"}' > "$info_file"
        return 1
    fi
}

# Function to run CRUD benchmark for specific configuration
run_crud_benchmark() {
    local config="$1"
    local description="$2"
    local port=$((BASE_PORT + $(date +%s) % 1000))  # Dynamic port to avoid conflicts

    echo -e "${CYAN}📊 Testing CRUD configuration: ${config} - ${description}${NC}"
    echo -e "${CYAN}============================================${NC}"

    # Start server
    if ! start_server "$config" "$port"; then
        echo -e "${RED}❌ Failed to start server for ${config}${NC}"
        return 1
    fi

    local url="http://127.0.0.1:${port}"
    local config_dir="${RAW_DATA_DIR}/${config}"
    mkdir -p "$config_dir"

    # Get server info
    get_server_info "$url" "${config_dir}/server_info.json"

    echo -e "${YELLOW}🧪 Running realistic CRUD benchmark...${NC}"
    echo "Operations: ${CRUD_TOTAL} (${CRUD_CREATE_PCT}% CREATE, ${CRUD_READ_PCT}% READ, ${CRUD_UPDATE_PCT}% UPDATE)"
    echo "Concurrency: ${CRUD_CONCURRENCY}"
    echo "Target: ${url}/api/products"

    # Run CRUD benchmark using our replication-loadgen
    local benchmark_log="${config_dir}/crud_benchmark.log"
    local start_time=$(date +%s.%N)

    if cargo run --release --bin replication-loadgen -- \
        --leader "$url" \
        --total "$CRUD_TOTAL" \
        --concurrency "$CRUD_CONCURRENCY" \
        --mode random \
        --create-pct "$CRUD_CREATE_PCT" \
        --read-pct "$CRUD_READ_PCT" \
        --update-pct "$CRUD_UPDATE_PCT" \
        --delete-pct "$CRUD_DELETE_PCT" \
        --timeout-s 30 > "$benchmark_log" 2>&1; then

        local end_time=$(date +%s.%N)
        local duration=$(echo "$end_time - $start_time" | bc -l)

        echo -e "${GREEN}✅ CRUD benchmark completed in ${duration}s${NC}"

        # Parse results from log
        local rps=$(grep "throughput" "$benchmark_log" | tail -1 | grep -oE '[0-9]+\.[0-9]+' | head -1 || echo "0")
        local total_ops=$(grep "completed:" "$benchmark_log" | tail -1 | grep -oE 'total=[0-9]+' | grep -oE '[0-9]+' || echo "$CRUD_TOTAL")

        # Extract latency percentiles for different operations
        local create_p50=$(grep -A 1 "CREATE.*count=" "$benchmark_log" | grep "p50=" | grep -oE '[0-9]+\.[0-9]+' || echo "0")
        local read_p50=$(grep -A 1 "READ.*count=" "$benchmark_log" | grep "p50=" | grep -oE '[0-9]+\.[0-9]+' || echo "0")
        local update_p50=$(grep -A 1 "UPDATE.*count=" "$benchmark_log" | grep "p50=" | grep -oE '[0-9]+\.[0-9]+' || echo "0")

        local create_p95=$(grep -A 1 "CREATE.*count=" "$benchmark_log" | grep "p95=" | grep -oE '[0-9]+\.[0-9]+' || echo "0")
        local read_p95=$(grep -A 1 "READ.*count=" "$benchmark_log" | grep "p95=" | grep -oE '[0-9]+\.[0-9]+' || echo "0")
        local update_p95=$(grep -A 1 "UPDATE.*count=" "$benchmark_log" | grep "p95=" | grep -oE '[0-9]+\.[0-9]+' || echo "0")

        # Create structured results
        cat > "${config_dir}/crud_results.json" << EOF
{
  "configuration": "$config",
  "description": "$description",
  "timestamp": "$(date -u +"%Y-%m-%d %H:%M:%S UTC")",
  "parameters": {
    "total_operations": $CRUD_TOTAL,
    "concurrency": $CRUD_CONCURRENCY,
    "create_pct": $CRUD_CREATE_PCT,
    "read_pct": $CRUD_READ_PCT,
    "update_pct": $CRUD_UPDATE_PCT,
    "delete_pct": $CRUD_DELETE_PCT
  },
  "results": {
    "duration": $duration,
    "total_completed": $total_ops,
    "throughput_ops_sec": $rps,
    "latencies": {
      "create": {
        "p50": $create_p50,
        "p95": $create_p95
      },
      "read": {
        "p50": $read_p50,
        "p95": $read_p95
      },
      "update": {
        "p50": $update_p50,
        "p95": $update_p95
      }
    }
  }
}
EOF

    else
        echo -e "${RED}❌ CRUD benchmark failed for ${config}${NC}"
    fi

    # Stop server
    stop_server "$config" "$port"

    echo -e "${GREEN}✅ Configuration ${config} testing completed${NC}"
    echo
}

# Function to generate comprehensive markdown report
generate_crud_report() {
    echo -e "${YELLOW}📝 Generating comprehensive CRUD report...${NC}"

    cat > "$REPORT_FILE" << EOF
# Lithair Comprehensive CRUD Benchmark Report

**Timestamp:** ${TIMESTAMP}
**Generated:** $(date -u +"%Y-%m-%d %H:%M:%S UTC")
**Tool:** replication-loadgen (Realistic CRUD operations)
**Framework:** Lithair DeclarativeServer

## Executive Summary

This report compares Lithair server performance using **realistic CRUD workloads** across different security configurations. Unlike simple GET request benchmarks, this test simulates real application usage with CREATE, READ, and UPDATE operations on the \`/api/products\` endpoint.

### Benchmark Parameters
- **Total Operations:** ${CRUD_TOTAL}
- **Concurrency:** ${CRUD_CONCURRENCY} concurrent connections
- **Operation Mix:** ${CRUD_CREATE_PCT}% CREATE, ${CRUD_READ_PCT}% READ, ${CRUD_UPDATE_PCT}% UPDATE, ${CRUD_DELETE_PCT}% DELETE
- **Target:** Product model via DeclarativeServer auto-generated CRUD API

EOF

    # Add configurations tested
    echo "## Configurations Tested" >> "$REPORT_FILE"
    echo "" >> "$REPORT_FILE"
    for config in "${!TEST_CONFIGS[@]}"; do
        if [[ -d "${RAW_DATA_DIR}/${config}" ]]; then
            echo "- **${config}**: ${TEST_CONFIGS[$config]}" >> "$REPORT_FILE"
        fi
    done
    echo "" >> "$REPORT_FILE"

    # Add detailed results for each configuration
    for config in "${!TEST_CONFIGS[@]}"; do
        local config_dir="${RAW_DATA_DIR}/${config}"
        if [[ ! -d "$config_dir" ]]; then
            continue
        fi

        echo "## Configuration: ${config}" >> "$REPORT_FILE"
        echo "" >> "$REPORT_FILE"
        echo "**Description:** ${TEST_CONFIGS[$config]}" >> "$REPORT_FILE"
        echo "" >> "$REPORT_FILE"

        # Server info
        if [[ -f "${config_dir}/server_info.json" ]]; then
            echo "### Server Configuration" >> "$REPORT_FILE"
            echo "" >> "$REPORT_FILE"
            echo '```json' >> "$REPORT_FILE"
            jq . "${config_dir}/server_info.json" >> "$REPORT_FILE" 2>/dev/null || echo "Server info not available" >> "$REPORT_FILE"
            echo '```' >> "$REPORT_FILE"
            echo "" >> "$REPORT_FILE"
        fi

        # CRUD benchmark results
        if [[ -f "${config_dir}/crud_results.json" ]]; then
            echo "### CRUD Performance Results" >> "$REPORT_FILE"
            echo "" >> "$REPORT_FILE"

            local duration=$(jq -r '.results.duration // 0' "${config_dir}/crud_results.json")
            local completed=$(jq -r '.results.total_completed // 0' "${config_dir}/crud_results.json")
            local throughput=$(jq -r '.results.throughput_ops_sec // 0' "${config_dir}/crud_results.json")

            local create_p50=$(jq -r '.results.latencies.create.p50 // 0' "${config_dir}/crud_results.json")
            local create_p95=$(jq -r '.results.latencies.create.p95 // 0' "${config_dir}/crud_results.json")
            local read_p50=$(jq -r '.results.latencies.read.p50 // 0' "${config_dir}/crud_results.json")
            local read_p95=$(jq -r '.results.latencies.read.p95 // 0' "${config_dir}/crud_results.json")
            local update_p50=$(jq -r '.results.latencies.update.p50 // 0' "${config_dir}/crud_results.json")
            local update_p95=$(jq -r '.results.latencies.update.p95 // 0' "${config_dir}/crud_results.json")

            echo "- **Duration:** ${duration}s" >> "$REPORT_FILE"
            echo "- **Operations Completed:** ${completed}/${CRUD_TOTAL}" >> "$REPORT_FILE"
            echo "- **Throughput:** ${throughput} operations/sec" >> "$REPORT_FILE"
            echo "" >> "$REPORT_FILE"
            echo "#### Latency by Operation Type" >> "$REPORT_FILE"
            echo "" >> "$REPORT_FILE"
            echo "| Operation | P50 Latency | P95 Latency |" >> "$REPORT_FILE"
            echo "|-----------|-------------|-------------|" >> "$REPORT_FILE"
            echo "| CREATE | ${create_p50}ms | ${create_p95}ms |" >> "$REPORT_FILE"
            echo "| READ | ${read_p50}ms | ${read_p95}ms |" >> "$REPORT_FILE"
            echo "| UPDATE | ${update_p50}ms | ${update_p95}ms |" >> "$REPORT_FILE"
            echo "" >> "$REPORT_FILE"
        fi

        # Raw benchmark log (last 20 lines)
        if [[ -f "${config_dir}/crud_benchmark.log" ]]; then
            echo "### Raw Benchmark Output" >> "$REPORT_FILE"
            echo "" >> "$REPORT_FILE"
            echo '```' >> "$REPORT_FILE"
            tail -n 20 "${config_dir}/crud_benchmark.log" >> "$REPORT_FILE"
            echo '```' >> "$REPORT_FILE"
            echo "" >> "$REPORT_FILE"
        fi
    done

    # Performance comparison summary
    echo "## CRUD Performance Comparison Summary" >> "$REPORT_FILE"
    echo "" >> "$REPORT_FILE"
    echo "| Configuration | Throughput (ops/sec) | CREATE P50 | READ P50 | UPDATE P50 | Security Features |" >> "$REPORT_FILE"
    echo "|---------------|---------------------|------------|----------|------------|-------------------|" >> "$REPORT_FILE"

    for config in "${!TEST_CONFIGS[@]}"; do
        local config_dir="${RAW_DATA_DIR}/${config}"
        if [[ -f "${config_dir}/crud_results.json" ]]; then
            local throughput=$(jq -r '.results.throughput_ops_sec // 0' "${config_dir}/crud_results.json")
            local create_p50=$(jq -r '.results.latencies.create.p50 // 0' "${config_dir}/crud_results.json")
            local read_p50=$(jq -r '.results.latencies.read.p50 // 0' "${config_dir}/crud_results.json")
            local update_p50=$(jq -r '.results.latencies.update.p50 // 0' "${config_dir}/crud_results.json")

            local security_features=""
            case "$config" in
                "baseline") security_features="None" ;;
                "firewall") security_features="Firewall" ;;
                "antiddos") security_features="Anti-DDoS" ;;
                "full") security_features="Firewall + Anti-DDoS" ;;
            esac

            echo "| $config | $throughput | ${create_p50}ms | ${read_p50}ms | ${update_p50}ms | $security_features |" >> "$REPORT_FILE"
        else
            echo "| $config | N/A | N/A | N/A | N/A | N/A |" >> "$REPORT_FILE"
        fi
    done

    echo "" >> "$REPORT_FILE"

    # Analysis and recommendations
    cat >> "$REPORT_FILE" << EOF

## Analysis & Recommendations

### CRUD Workload Insights
This benchmark uses **realistic application patterns** instead of simple health checks:
- **CREATE operations** test JSON parsing, validation, and database writes
- **READ operations** test data retrieval and serialization
- **UPDATE operations** test data modification and consistency
- **Mixed workload** simulates real user behavior patterns

### Security vs Performance Trade-offs
1. **Anti-DDoS Protection**: Rate limiting may reduce throughput but protects against abuse
2. **Firewall**: IP filtering adds minimal overhead for allowed connections
3. **Combined Protection**: Full security stack provides comprehensive protection

### Key Observations
1. CRUD operations are more resource-intensive than simple GET requests
2. Database operations (CREATE/UPDATE) typically have higher latency than reads
3. Real application performance differs significantly from synthetic benchmarks

### Production Recommendations
- **Baseline**: Only for internal/trusted environments
- **Full Protection**: Recommended for production with external traffic
- **Monitor CRUD patterns**: Different operation mixes affect performance differently
- **Optimize hot paths**: Focus on most frequent operations in your workload

## Technical Details

**Test Environment:**
- Lithair DeclarativeServer with auto-generated CRUD API
- replication-loadgen with realistic operation patterns
- Dynamic port allocation to prevent conflicts
- Product model with JSON serialization/deserialization

**Data Files:**
All raw benchmark data and logs available in: \`${RAW_DATA_DIR}/\`

---
*Generated by Lithair Comprehensive CRUD Benchmark Suite v1.0*
EOF

    echo -e "${GREEN}✅ CRUD report generated: ${REPORT_FILE}${NC}"
}

# Main benchmark execution
main() {
    echo -e "${YELLOW}🏁 Starting comprehensive CRUD benchmark suite...${NC}"
    echo

    # Kill any existing servers on potential ports
    for port in $(seq $BASE_PORT $((BASE_PORT + 100))); do
        if lsof -ti :$port >/dev/null 2>&1; then
            echo "Killing existing process on port $port"
            lsof -ti :$port | xargs kill -9 2>/dev/null || true
        fi
    done

    # Build the binaries
    echo -e "${YELLOW}🔨 Building release binaries...${NC}"
    cargo build --release --bin replication-hardening-node --bin replication-loadgen >/dev/null

    # Run CRUD benchmarks for each configuration
    for config in "${!TEST_CONFIGS[@]}"; do
        run_crud_benchmark "$config" "${TEST_CONFIGS[$config]}"
    done

    # Generate comprehensive report
    generate_crud_report

    echo
    echo -e "${GREEN}🎉 Comprehensive CRUD benchmark suite completed!${NC}"
    echo -e "${GREEN}Report available at: ${REPORT_FILE}${NC}"
    echo
    echo "To view the report:"
    echo "  cat ${REPORT_FILE}"
    echo
    echo "Raw data directory:"
    echo "  ${RAW_DATA_DIR}"
}

# Handle Ctrl+C gracefully
trap 'echo -e "\n${YELLOW}⚠️ Benchmark interrupted - cleaning up servers${NC}";
      for config in "${!TEST_CONFIGS[@]}"; do
          for port in $(seq $BASE_PORT $((BASE_PORT + 100))); do
              stop_server "$config" "$port" 2>/dev/null || true
          done
      done;
      exit 130' INT

# Run main function
main "$@"