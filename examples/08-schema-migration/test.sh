#!/usr/bin/env bash
# Test script for 08-schema-migration example
# Tests schema migration detection, lock/unlock, history, and CRUD
#
# Usage: ./test.sh

source "$(dirname "$0")/../../scripts/test-lib.sh"

# Clean data from previous runs
rm -rf data/schema_demo*

PORT=8090
start_server schema-migration -p "$PORT" -m warn

API="${BASE_URL}/api/products"
ADMIN="${BASE_URL}/_admin/schema"

# ── PRODUCT CRUD ─────────────────────────────────────────────────────────────
log_test "GET /api/products - list products"
body=$(http_get "$API")
assert_status 200 "list products"

log_test "POST /api/products - create product"
body=$(http_post "$API" "{\"name\":\"Test Product\",\"description\":\"Created during test\",\"price_cents\":1999,\"stock\":100,\"active\":true,\"created_at\":\"2026-01-01T00:00:00Z\",\"priority\":0,\"category\":\"test\",\"featured\":false}")
assert_status 201 "create product"
assert_contains "$body" "Test Product" "response has product name"

# ── SCHEMA ADMIN ENDPOINTS ──────────────────────────────────────────────────
log_test "GET /_admin/schema - list schemas"
body=$(http_get "$ADMIN")
assert_status 200 "list schemas"
assert_contains "$body" "schemas" "response has schemas key"

log_test "GET /_admin/schema/lock/status - lock status"
body=$(http_get "$ADMIN/lock/status")
assert_status 200 "lock status endpoint"
assert_contains "$body" "locked" "response has locked field"

log_test "POST /_admin/schema/lock - lock schema"
body=$(http_post "$ADMIN/lock" '{"reason":"Test lock"}')
assert_status 200 "lock schema"
assert_contains "$body" "locked" "response confirms lock"

log_test "GET /_admin/schema/lock/status - verify locked"
body=$(http_get "$ADMIN/lock/status")
assert_status 200 "lock status after lock"
assert_contains "$body" "true" "schema is locked"

log_test "POST /_admin/schema/unlock - unlock schema"
body=$(http_post "$ADMIN/unlock" '{"reason":"Test unlock","duration_seconds":300,"unlocked_by":"test@example.com"}')
assert_status 200 "unlock schema"
assert_contains "$body" "unlocked" "response confirms unlock"

log_test "GET /_admin/schema/history - schema history"
body=$(http_get "$ADMIN/history")
assert_status 200 "schema history endpoint"
assert_contains "$body" "count" "response has count"

log_test "GET /_admin/schema/diff - schema diff"
body=$(http_get "$ADMIN/diff")
assert_status 200 "schema diff endpoint"

log_test "GET /_admin/schema/pending - pending changes"
body=$(http_get "$ADMIN/pending")
assert_status 200 "pending changes endpoint"

log_test "POST /_admin/schema/sync - sync endpoint (standalone)"
body=$(http_post "$ADMIN/sync" '{}')
# In standalone mode, sync returns 400 (not in cluster mode) which is expected
if [ "$HTTP_STATUS" = "200" ] || [ "$HTTP_STATUS" = "400" ] || [ "$HTTP_STATUS" = "503" ]; then
    log_pass "sync endpoint responds correctly (HTTP $HTTP_STATUS)"
    (( _TESTS_PASSED++ )) || true
else
    log_fail "sync endpoint unexpected status (HTTP $HTTP_STATUS)"
    (( _TESTS_FAILED++ )) || true
fi

print_summary
