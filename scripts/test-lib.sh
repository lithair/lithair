#!/usr/bin/env bash
# Shared test library for Lithair examples
# Source this in example test scripts: source "$(dirname "$0")/../../scripts/test-lib.sh"

set -euo pipefail

# ── Colors ────────────────────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# ── Logging ───────────────────────────────────────────────────────────────────
log_info()  { echo -e "${GREEN}[INFO]${NC}  $1"; }
log_warn()  { echo -e "${YELLOW}[WARN]${NC}  $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }
log_test()  { echo -e "${BLUE}[TEST]${NC}  $1"; }
log_pass()  { echo -e "${GREEN}[PASS]${NC}  $1"; }
log_fail()  { echo -e "${RED}[FAIL]${NC}  $1"; }

# ── Counters ──────────────────────────────────────────────────────────────────
_TESTS_PASSED=0
_TESTS_FAILED=0

# ── Project paths ─────────────────────────────────────────────────────────────
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# ── Server management ─────────────────────────────────────────────────────────
SERVER_PID=""

# Build and start a Lithair example server.
# Usage: start_server <package-name> [extra-args...]
# Port is auto-detected from the server's log output.
# Override with: PORT=9090 start_server <pkg>
start_server() {
    local pkg="$1"; shift
    local log_file="/tmp/lithair-test-${pkg}.log"

    HOST="${HOST:-127.0.0.1}"

    log_info "Building ${pkg}..."
    cargo build -q -p "$pkg"

    PORT="${PORT:-8080}"
    BASE_URL="http://${HOST}:${PORT}"

    log_info "Starting ${pkg} on port ${PORT}..."
    RUST_LOG="${RUST_LOG:-info}" target/debug/"$pkg" "$@" \
        >"$log_file" 2>&1 &
    SERVER_PID=$!

    trap '_cleanup_server' EXIT

    wait_for_server
    log_info "Server ${pkg} ready (PID: ${SERVER_PID})"
}

# Stop the server (called automatically via trap)
_cleanup_server() {
    if [ -n "$SERVER_PID" ]; then
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
        SERVER_PID=""
    fi
}

# Wait for the server to accept connections on BASE_URL
wait_for_server() {
    local url="${BASE_URL:-http://127.0.0.1:${PORT:-8080}}"
    local max_attempts="${WAIT_ATTEMPTS:-40}"
    for i in $(seq 1 "$max_attempts"); do
        # Accept any HTTP response (even 404) — just checking the server is up
        if curl -so /dev/null --connect-timeout 1 "${url}/" 2>/dev/null; then
            return 0
        fi
        sleep 0.25
    done
    log_error "Server did not start within $(( max_attempts / 4 ))s"
    cat /tmp/lithair-test-*.log 2>/dev/null | tail -20
    exit 1
}

# ── Auth support ──────────────────────────────────────────────────────────────
AUTH_HEADER=""

# Set authentication token for subsequent requests.
# Usage: set_auth "Bearer <token>"
set_auth() { AUTH_HEADER="$1"; }

# Clear authentication.
clear_auth() { AUTH_HEADER=""; }

# ── HTTP helpers ──────────────────────────────────────────────────────────────
HTTP_STATUS=""
_HTTP_STATUS_FILE=$(mktemp)
_HTTP_BODY_FILE=$(mktemp)

# Internal: execute curl, write status to shared file (survives subshells).
_curl_req() {
    local method="$1"
    local url="$2"
    local data="${3:-}"
    local curl_args=(-s -X "$method" -w '%{http_code}' -o "$_HTTP_BODY_FILE")
    if [ -n "$AUTH_HEADER" ]; then
        curl_args+=(-H "Authorization: $AUTH_HEADER")
    fi
    if [ -n "$data" ]; then
        curl_args+=(-H "Content-Type: application/json" -d "$data")
    fi
    curl "${curl_args[@]}" "$url" 2>/dev/null > "$_HTTP_STATUS_FILE"
    HTTP_STATUS=$(cat "$_HTTP_STATUS_FILE")
    cat "$_HTTP_BODY_FILE"
}

# GET request, returns body. Sets HTTP_STATUS.
http_get()    { _curl_req GET    "$1"; }

# POST JSON, returns body. Sets HTTP_STATUS.
http_post()   { _curl_req POST   "$1" "$2"; }

# PUT JSON, returns body. Sets HTTP_STATUS.
http_put()    { _curl_req PUT    "$1" "$2"; }

# PATCH JSON, returns body. Sets HTTP_STATUS.
http_patch()  { _curl_req PATCH  "$1" "$2"; }

# DELETE, returns body. Sets HTTP_STATUS.
http_delete() { _curl_req DELETE "$1"; }

# ── Assertions ────────────────────────────────────────────────────────────────

# Assert HTTP status equals expected value.
# Usage: assert_status 200 "create todo"
assert_status() {
    local expected="$1"
    local label="${2:-request}"
    HTTP_STATUS=$(cat "$_HTTP_STATUS_FILE" 2>/dev/null)
    if [ "$HTTP_STATUS" = "$expected" ]; then
        log_pass "$label (HTTP $HTTP_STATUS)"
        (( _TESTS_PASSED++ )) || true
    else
        log_fail "$label (expected HTTP $expected, got $HTTP_STATUS)"
        (( _TESTS_FAILED++ )) || true
    fi
}

# Assert response body contains a string.
# Usage: assert_contains "$body" "Learn Lithair" "body has title"
assert_contains() {
    local body="$1"
    local expected="$2"
    local label="${3:-contains check}"
    if echo "$body" | grep -q "$expected"; then
        log_pass "$label"
        (( _TESTS_PASSED++ )) || true
    else
        log_fail "$label (expected to contain: $expected)"
        (( _TESTS_FAILED++ )) || true
    fi
}

# Assert response body does NOT contain a string.
assert_not_contains() {
    local body="$1"
    local expected="$2"
    local label="${3:-not-contains check}"
    if echo "$body" | grep -q "$expected"; then
        log_fail "$label (should not contain: $expected)"
        (( _TESTS_FAILED++ )) || true
    else
        log_pass "$label"
        (( _TESTS_PASSED++ )) || true
    fi
}

# ── Summary ───────────────────────────────────────────────────────────────────

# Print test summary and exit with appropriate code.
# Call this at the end of your test script.
print_summary() {
    local total=$(( _TESTS_PASSED + _TESTS_FAILED ))
    echo ""
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    if [ "$_TESTS_FAILED" -eq 0 ]; then
        echo -e "${GREEN}  All ${total} tests passed${NC}"
    else
        echo -e "${RED}  ${_TESTS_FAILED}/${total} tests FAILED${NC}"
    fi
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    [ "$_TESTS_FAILED" -eq 0 ]
}
