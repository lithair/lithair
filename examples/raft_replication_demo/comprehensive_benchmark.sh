#!/bin/bash

# Lithair Comprehensive Benchmark Suite
# Automated comparative benchmarking with configurable security features
# Generates markdown reports with complete performance analysis

set -euo pipefail

# Configuration
BASE_PORT=${BASE_PORT:-20000}
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
RESULTS_DIR="./benchmark_results"
REPORT_FILE="${RESULTS_DIR}/comprehensive_benchmark_${TIMESTAMP}.md"
RAW_DATA_DIR="${RESULTS_DIR}/raw_data_${TIMESTAMP}"

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
    ["baseline"]="No protection (baseline performance)"
    ["firewall"]="Firewall only (IP filtering)"
    ["antiddos"]="Anti-DDoS only (rate limiting)"
    ["full"]="Full protection (firewall + anti-DDoS)"
)

echo -e "${BLUE}🚀 Lithair Comprehensive Benchmark Suite${NC}"
echo -e "${BLUE}===========================================${NC}"
echo "Timestamp: ${TIMESTAMP}"
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
        eval "$env_vars cargo run --release --bin http_hardening_node -- $args" > "/tmp/server_${config}_${port}.log" 2>&1 &
    else
        cargo run --release --bin http_hardening_node -- $args > "/tmp/server_${config}_${port}.log" 2>&1 &
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

# Function to detect available endpoints
detect_endpoints() {
    local url="$1"
    local endpoints=()

    # Standard endpoints to check
    local check_endpoints=("/health" "/status" "/info" "/ready" "/api/products" "/observe/metrics" "/observe/perf/json")

    for endpoint in "${check_endpoints[@]}"; do
        if curl -s --max-time 2 "${url}${endpoint}" > /dev/null 2>&1; then
            endpoints+=("$endpoint")
        fi
    done

    echo "${endpoints[@]}"
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

# Function to run benchmark for specific configuration
run_config_benchmark() {
    local config="$1"
    local description="$2"
    local port=$((BASE_PORT + $(date +%s) % 1000))  # Dynamic port to avoid conflicts

    echo -e "${CYAN}📊 Testing configuration: ${config} - ${description}${NC}"
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

    # Detect available endpoints
    local endpoints=($(detect_endpoints "$url"))
    echo "Detected endpoints: ${endpoints[*]}"
    echo "${endpoints[*]}" > "${config_dir}/endpoints.txt"

    # Benchmark each endpoint
    for endpoint in "${endpoints[@]}"; do
        local endpoint_name=$(echo "$endpoint" | sed 's|/|_|g' | sed 's|^_||')
        if [[ -z "$endpoint_name" ]]; then
            endpoint_name="root"
        fi

        echo -e "${YELLOW}🧪 Benchmarking endpoint: ${endpoint}${NC}"

        # Light test
        local test_url="${url}${endpoint}"
        local result_file="${config_dir}/benchmark_${endpoint_name}_light.json"

        if oha -n 100 -c 10 --no-tui --output-format json -o "$result_file" "$test_url" 2>/dev/null; then
            # Add metadata
            jq --arg config "$config" \
               --arg endpoint "$endpoint" \
               --arg test_type "light" \
               --arg timestamp "$(date -u +"%Y-%m-%d %H:%M:%S UTC")" \
               '. + {
                   configuration: $config,
                   endpoint: $endpoint,
                   test_type: $test_type,
                   timestamp: $timestamp
               }' "$result_file" > "${result_file}.tmp" && mv "${result_file}.tmp" "$result_file"

            echo -e "${GREEN}✅ Light test completed for ${endpoint}${NC}"
        else
            echo -e "${RED}❌ Light test failed for ${endpoint}${NC}"
        fi

        # Stress test (only for health endpoints to avoid overwhelming)
        if [[ "$endpoint" == "/health" || "$endpoint" == "/status" ]]; then
            echo -e "${YELLOW}🔥 Running stress test for ${endpoint}${NC}"
            local stress_result="${config_dir}/benchmark_${endpoint_name}_stress.json"

            if oha -n 1000 -c 50 --no-tui --output-format json -o "$stress_result" "$test_url" 2>/dev/null; then
                # Add metadata
                jq --arg config "$config" \
                   --arg endpoint "$endpoint" \
                   --arg test_type "stress" \
                   --arg timestamp "$(date -u +"%Y-%m-%d %H:%M:%S UTC")" \
                   '. + {
                       configuration: $config,
                       endpoint: $endpoint,
                       test_type: $test_type,
                       timestamp: $timestamp
                   }' "$stress_result" > "${stress_result}.tmp" && mv "${stress_result}.tmp" "$stress_result"

                echo -e "${GREEN}✅ Stress test completed for ${endpoint}${NC}"
            else
                echo -e "${RED}❌ Stress test failed for ${endpoint}${NC}"
            fi
        fi
    done

    # Stop server
    stop_server "$config" "$port"

    echo -e "${GREEN}✅ Configuration ${config} testing completed${NC}"
    echo
}

# Function to generate markdown report
generate_report() {
    echo -e "${YELLOW}📝 Generating comprehensive report...${NC}"

    cat > "$REPORT_FILE" << EOF
# Lithair Comprehensive Benchmark Report

**Timestamp:** ${TIMESTAMP}
**Generated:** $(date -u +"%Y-%m-%d %H:%M:%S UTC")
**Tool:** oha (HTTP load generator)
**Framework:** Lithair DeclarativeServer

## Executive Summary

This report compares Lithair server performance across different security configurations:

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

        # Endpoints tested
        if [[ -f "${config_dir}/endpoints.txt" ]]; then
            echo "### Endpoints Tested" >> "$REPORT_FILE"
            echo "" >> "$REPORT_FILE"
            while read -r endpoint; do
                echo "- \`${endpoint}\`" >> "$REPORT_FILE"
            done < "${config_dir}/endpoints.txt"
            echo "" >> "$REPORT_FILE"
        fi

        # Benchmark results
        echo "### Performance Results" >> "$REPORT_FILE"
        echo "" >> "$REPORT_FILE"

        local results_found=false
        for result_file in "${config_dir}"/benchmark_*.json; do
            if [[ -f "$result_file" ]]; then
                results_found=true
                local filename=$(basename "$result_file")
                local test_name=$(echo "$filename" | sed 's/benchmark_//; s/.json//')

                echo "#### Test: ${test_name}" >> "$REPORT_FILE"
                echo "" >> "$REPORT_FILE"

                # Extract key metrics
                if jq -e . "$result_file" > /dev/null 2>&1; then
                    local endpoint=$(jq -r '.endpoint // "unknown"' "$result_file")
                    local test_type=$(jq -r '.test_type // "unknown"' "$result_file")
                    local total_requests=$(jq -r '.statusCodeDistribution."200" // 0' "$result_file")
                    local duration=$(jq -r '.summary.total // 0' "$result_file")
                    local rps=$(jq -r '.summary.requestsPerSec // 0' "$result_file")
                    local p50=$(jq -r '.latencyPercentiles.p50 // 0' "$result_file")
                    local p95=$(jq -r '.latencyPercentiles.p95 // 0' "$result_file")
                    local p99=$(jq -r '.latencyPercentiles.p99 // 0' "$result_file")
                    local success_2xx=$(jq -r '.statusCodeDistribution."200" // 0' "$result_file")
                    local blocked_429=$(jq -r '.statusCodeDistribution."429" // 0' "$result_file")

                    echo "- **Endpoint:** \`${endpoint}\`" >> "$REPORT_FILE"
                    echo "- **Test Type:** ${test_type}" >> "$REPORT_FILE"
                    echo "- **Total Requests:** ${total_requests}" >> "$REPORT_FILE"
                    echo "- **Duration:** ${duration}s" >> "$REPORT_FILE"
                    echo "- **Requests/Second:** ${rps}" >> "$REPORT_FILE"
                    echo "- **Latency P50:** ${p50}ms" >> "$REPORT_FILE"
                    echo "- **Latency P95:** ${p95}ms" >> "$REPORT_FILE"
                    echo "- **Latency P99:** ${p99}ms" >> "$REPORT_FILE"
                    echo "- **Success (200):** ${success_2xx}" >> "$REPORT_FILE"
                    if [[ "$blocked_429" != "0" ]]; then
                        echo "- **Blocked (429):** ${blocked_429}" >> "$REPORT_FILE"
                    fi
                    echo "" >> "$REPORT_FILE"
                fi
            fi
        done

        if [[ "$results_found" == "false" ]]; then
            echo "No benchmark results available for this configuration." >> "$REPORT_FILE"
            echo "" >> "$REPORT_FILE"
        fi
    done

    # Performance comparison summary
    echo "## Performance Comparison Summary" >> "$REPORT_FILE"
    echo "" >> "$REPORT_FILE"
    echo "| Configuration | RPS (avg) | Latency P50 | Latency P95 | Blocked Requests |" >> "$REPORT_FILE"
    echo "|---------------|-----------|-------------|-------------|------------------|" >> "$REPORT_FILE"

    for config in "${!TEST_CONFIGS[@]}"; do
        local config_dir="${RAW_DATA_DIR}/${config}"
        if [[ -d "$config_dir" ]]; then
            # Calculate averages from all benchmark files
            local total_rps=0
            local total_p50=0
            local total_p95=0
            local total_blocked=0
            local count=0

            for result_file in "${config_dir}"/benchmark_*_light.json; do
                if [[ -f "$result_file" ]] && jq -e . "$result_file" > /dev/null 2>&1; then
                    local rps=$(jq -r '.summary.requestsPerSec // 0' "$result_file")
                    local p50=$(jq -r '.latencyPercentiles.p50 // 0' "$result_file")
                    local p95=$(jq -r '.latencyPercentiles.p95 // 0' "$result_file")
                    local blocked=$(jq -r '.statusCodeDistribution."429" // 0' "$result_file")

                    total_rps=$(echo "$total_rps + $rps" | bc -l 2>/dev/null || echo "$total_rps")
                    total_p50=$(echo "$total_p50 + $p50" | bc -l 2>/dev/null || echo "$total_p50")
                    total_p95=$(echo "$total_p95 + $p95" | bc -l 2>/dev/null || echo "$total_p95")
                    total_blocked=$(echo "$total_blocked + $blocked" | bc -l 2>/dev/null || echo "$total_blocked")
                    count=$((count + 1))
                fi
            done

            if [[ $count -gt 0 ]]; then
                local avg_rps=$(echo "scale=0; $total_rps / $count" | bc -l 2>/dev/null || echo "0")
                local avg_p50=$(echo "scale=2; $total_p50 / $count" | bc -l 2>/dev/null || echo "0")
                local avg_p95=$(echo "scale=2; $total_p95 / $count" | bc -l 2>/dev/null || echo "0")

                echo "| $config | $avg_rps | ${avg_p50}ms | ${avg_p95}ms | $total_blocked |" >> "$REPORT_FILE"
            else
                echo "| $config | N/A | N/A | N/A | N/A |" >> "$REPORT_FILE"
            fi
        fi
    done

    echo "" >> "$REPORT_FILE"

    # Analysis and recommendations
    cat >> "$REPORT_FILE" << EOF

## Analysis & Recommendations

### Security Impact Assessment
- **Baseline** shows raw performance without any protection
- **Firewall** impact on internal network performance
- **Anti-DDoS** demonstrates rate limiting effectiveness
- **Full protection** combines all security features

### Key Observations
1. Rate limiting (429 responses) indicates active DDoS protection
2. Latency increases are expected with security features enabled
3. RPS reduction shows the overhead of security processing

### Recommendations
- Use **baseline** for internal/trusted environments only
- Use **full protection** for public-facing production deployments
- Monitor 429 responses to tune rate limiting thresholds
- Consider endpoint-specific protection policies

## Technical Details

**Test Environment:**
- Lithair DeclarativeServer with http_hardening_node
- oha HTTP load generator with JSON output
- Dynamic port allocation to prevent conflicts
- Automatic endpoint discovery

**Data Files:**
All raw benchmark data available in: \`${RAW_DATA_DIR}/\`

---
*Generated by Lithair Comprehensive Benchmark Suite v1.0*
EOF

    echo -e "${GREEN}✅ Report generated: ${REPORT_FILE}${NC}"
}

# Main benchmark execution
main() {
    echo -e "${YELLOW}🏁 Starting comprehensive benchmark suite...${NC}"
    echo

    # Kill any existing servers on potential ports
    for port in $(seq $BASE_PORT $((BASE_PORT + 100))); do
        if lsof -ti :$port >/dev/null 2>&1; then
            echo "Killing existing process on port $port"
            lsof -ti :$port | xargs kill -9 2>/dev/null || true
        fi
    done

    # Run benchmarks for each configuration
    for config in "${!TEST_CONFIGS[@]}"; do
        run_config_benchmark "$config" "${TEST_CONFIGS[$config]}"
    done

    # Generate comprehensive report
    generate_report

    echo
    echo -e "${GREEN}🎉 Comprehensive benchmark suite completed!${NC}"
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