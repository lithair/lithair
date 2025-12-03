#!/bin/bash

# Lithair HTTP Benchmark with oha
# Automated benchmark script using oha HTTP load generator

set -euo pipefail

# Configuration
PORT=${PORT:-19998}
URL="http://127.0.0.1:${PORT}"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
RESULTS_DIR="./benchmark_results"
RESULT_FILE="${RESULTS_DIR}/oha_benchmark_${TIMESTAMP}.json"

# Create results directory
mkdir -p "${RESULTS_DIR}"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}🚀 Lithair HTTP Benchmark with oha${NC}"
echo -e "${BLUE}=================================${NC}"
echo "Target URL: ${URL}"
echo "Results will be saved to: ${RESULT_FILE}"
echo

# Function to check if server is responding
check_server() {
    echo -e "${YELLOW}🔍 Checking server availability...${NC}"
    if curl -s --max-time 3 "${URL}/health" > /dev/null; then
        echo -e "${GREEN}✅ Server is responding${NC}"
        return 0
    else
        echo -e "${RED}❌ Server is not responding at ${URL}${NC}"
        echo "Make sure the server is running with:"
        echo "  cargo run --release -p raft_replication_demo --bin http_hardening_node -- --port ${PORT} --open"
        return 1
    fi
}

# Function to run oha benchmark and save results
run_benchmark() {
    local test_name="$1"
    local requests="$2"
    local concurrency="$3"
    local endpoint="$4"
    local description="$5"

    echo -e "${YELLOW}🧪 Running: ${test_name}${NC}"
    echo "  Requests: ${requests}, Concurrency: ${concurrency}, Endpoint: ${endpoint}"
    echo "  Description: ${description}"

    local test_url="${URL}${endpoint}"
    local temp_result="/tmp/oha_${test_name}_${TIMESTAMP}.json"

    # Run oha with JSON output
    if oha -n "${requests}" -c "${concurrency}" --no-tui --output-format json -o "${temp_result}" "${test_url}"; then
        echo -e "${GREEN}✅ ${test_name} completed${NC}"

        # Add metadata to the result
        jq --arg name "$test_name" \
           --arg desc "$description" \
           --arg url "$test_url" \
           --arg timestamp "$(date -u +"%Y-%m-%d %H:%M:%S UTC")" \
           '. + {
               test_name: $name,
               description: $desc,
               target_url: $url,
               timestamp: $timestamp,
               config: {
                   requests: '"$requests"',
                   concurrency: '"$concurrency"'
               }
           }' "${temp_result}" > "${temp_result}.enriched"

        mv "${temp_result}.enriched" "${temp_result}"
        echo "  Results saved to: ${temp_result}"
    else
        echo -e "${RED}❌ ${test_name} failed${NC}"
        return 1
    fi
}

# Function to merge all results into final JSON
merge_results() {
    echo -e "${YELLOW}📊 Merging all benchmark results...${NC}"

    local temp_files=(/tmp/oha_*_${TIMESTAMP}.json)
    if [[ ${#temp_files[@]} -eq 0 ]]; then
        echo -e "${RED}❌ No benchmark results found${NC}"
        return 1
    fi

    # Create final JSON structure
    {
        echo "{"
        echo "  \"benchmark_suite\": \"Lithair HTTP Benchmark with oha\","
        echo "  \"timestamp\": \"$(date -u +"%Y-%m-%d %H:%M:%S UTC")\","
        echo "  \"target_server\": \"${URL}\","
        echo "  \"tests\": ["

        local first=true
        for file in "${temp_files[@]}"; do
            if [[ -f "$file" ]]; then
                if [[ "$first" == "true" ]]; then
                    first=false
                else
                    echo ","
                fi
                cat "$file"
            fi
        done

        echo ""
        echo "  ]"
        echo "}"
    } > "${RESULT_FILE}"

    # Cleanup temp files
    rm -f /tmp/oha_*_${TIMESTAMP}.json

    echo -e "${GREEN}✅ Final results saved to: ${RESULT_FILE}${NC}"
}

# Function to show summary
show_summary() {
    echo -e "${BLUE}📊 Benchmark Summary${NC}"
    echo -e "${BLUE}===================${NC}"

    if [[ -f "${RESULT_FILE}" ]]; then
        jq -r '.tests[] | "
Test: \(.test_name)
  Requests: \(.config.requests), Concurrency: \(.config.concurrency)
  Total Time: \(.summary.duration_total) seconds
  RPS: \(.summary.rps) requests/sec
  Latency p50: \(.latency.p50) ms
  Latency p95: \(.latency.p95) ms
  Latency p99: \(.latency.p99) ms
  Success Rate: \(.status_codes."200" // 0) / \(.summary.total_requests)
"' "${RESULT_FILE}"
    fi
}

# Main benchmark execution
main() {
    # Check if server is running
    if ! check_server; then
        exit 1
    fi

    echo -e "${YELLOW}🏁 Starting benchmark suite...${NC}"
    echo

    # Benchmark 1: Health endpoint (baseline)
    run_benchmark "health_baseline" 1000 10 "/health" "Basic health check endpoint performance"

    # Benchmark 2: Info endpoint (our phpinfo-style endpoint)
    run_benchmark "info_endpoint" 500 10 "/info" "Server diagnostics endpoint with JSON response"

    # Benchmark 3: Light load test
    run_benchmark "light_load" 2000 32 "/health" "Light load test with moderate concurrency"

    # Benchmark 4: Status endpoint (comparison with /health)
    run_benchmark "status_endpoint" 1000 10 "/status" "Legacy status endpoint for comparison"

    # Benchmark 5: Performance endpoint (JSON 1KB)
    run_benchmark "perf_json_1kb" 1000 16 "/observe/perf/json?bytes=1024" "JSON throughput test (1KB payload)"

    # Benchmark 6: High concurrency stress test
    run_benchmark "stress_test" 5000 64 "/health" "High concurrency stress test"

    # Merge all results
    merge_results

    # Show summary
    show_summary

    echo
    echo -e "${GREEN}🎉 Benchmark suite completed!${NC}"
    echo -e "${GREEN}Results available at: ${RESULT_FILE}${NC}"
    echo
    echo "To view detailed results:"
    echo "  jq . ${RESULT_FILE}"
    echo
    echo "To compare with previous benchmarks:"
    echo "  jq '.tests[] | select(.test_name == \"light_load\") | .summary.rps' ${RESULT_FILE}"
}

# Handle Ctrl+C gracefully
trap 'echo -e "\n${YELLOW}⚠️ Benchmark interrupted${NC}"; exit 130' INT

# Run main function
main "$@"