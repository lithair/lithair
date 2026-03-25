#!/usr/bin/env bash
# Test script for 01-hello-world example
# Tests the minimal LithairServer with admin panel
#
# Usage: ./test.sh

source "$(dirname "$0")/../../scripts/test-lib.sh"

start_server hello-world

# ── ROOT ──────────────────────────────────────────────────────────────────────
log_test "GET / - root endpoint"
body=$(http_get "$BASE_URL/")
assert_status 200 "root returns 200"

# ── ADMIN PANEL ──────────────────────────────────────────────────────────────
log_test "GET /_admin - admin panel"
body=$(http_get "$BASE_URL/_admin")
assert_status 200 "admin panel accessible"
assert_contains "$body" "doctype" "admin page is HTML"

# ── METRICS ──────────────────────────────────────────────────────────────────
log_test "GET /_admin/metrics - metrics endpoint"
body=$(http_get "$BASE_URL/_admin/metrics")
assert_status 200 "metrics endpoint accessible"

print_summary
