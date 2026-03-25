#!/usr/bin/env bash
# Test script for 02-static-site example
# Tests static file serving from SCC2 memory
#
# Usage: ./test.sh

source "$(dirname "$0")/../../scripts/test-lib.sh"

start_server static-site

# ── STATIC HTML ──────────────────────────────────────────────────────────────
log_test "GET / - serves index.html from memory"
body=$(http_get "$BASE_URL/")
assert_status 200 "root returns 200"
assert_contains "$body" "Lithair" "page contains Lithair branding"
assert_contains "$body" "<!DOCTYPE html>" "response is HTML"

print_summary
