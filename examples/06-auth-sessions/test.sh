#!/usr/bin/env bash
# Test script for 06-auth-sessions example
# Tests RBAC with session management, login/logout, and product CRUD
#
# Usage: ./test.sh

source "$(dirname "$0")/../../scripts/test-lib.sh"

# Clean data from previous runs
rm -rf data/sessions* data/products* data/rbac_demo*

PORT=3000
start_server auth-sessions --port "$PORT"

API="${BASE_URL}/api/products"
AUTH="${BASE_URL}/auth"

# ── AUTH: LOGIN ──────────────────────────────────────────────────────────────
log_test "POST /auth/login - admin login"
body=$(http_post "$AUTH/login" '{"username":"admin","password":"password123"}')
assert_status 200 "admin login succeeds"
assert_contains "$body" "session_token" "response has session_token"
assert_contains "$body" "Administrator" "role is Administrator"

ADMIN_TOKEN=$(echo "$body" | sed -n 's/.*"session_token"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
if [ -z "$ADMIN_TOKEN" ]; then
    log_fail "could not extract admin session token"
    print_summary
fi
log_info "Got admin token: ${ADMIN_TOKEN:0:16}..."

log_test "POST /auth/login - alice (Customer) login"
body=$(http_post "$AUTH/login" '{"username":"alice","password":"password123"}')
assert_status 200 "customer login succeeds"
assert_contains "$body" "Customer" "role is Customer"

log_test "POST /auth/login - invalid credentials"
http_post "$AUTH/login" '{"username":"admin","password":"wrong"}' >/dev/null
assert_status 401 "reject invalid credentials"

# ── PRODUCT CRUD (AS ADMIN) ─────────────────────────────────────────────────
set_auth "Bearer $ADMIN_TOKEN"

log_test "POST /api/products - create product (admin)"
body=$(http_post "$API" '{"id":"p1","name":"Laptop","price":999.99}')
assert_status 201 "create product"
assert_contains "$body" "Laptop" "response has product name"

log_test "GET /api/products - list products"
body=$(http_get "$API")
assert_status 200 "list products"

log_test "GET /api/products/p1 - get product by id"
body=$(http_get "$API/p1")
assert_status 200 "get product by id"
assert_contains "$body" "999.99" "product price correct"

log_test "PATCH /api/products/p1 - update price"
body=$(http_patch "$API/p1" '{"price":899.99}')
assert_status 200 "patch product price"
assert_contains "$body" "899.99" "price updated"

log_test "DELETE /api/products/p1 - delete product"
http_delete "$API/p1" >/dev/null
assert_status 204 "delete product"

log_test "GET /api/products/p1 - deleted product returns 404"
http_get "$API/p1" >/dev/null
assert_status 404 "deleted product gone"

# ── LOGOUT ───────────────────────────────────────────────────────────────────
log_test "POST /auth/logout - logout"
body=$(http_post "$AUTH/logout" '{}')
assert_status 200 "logout succeeds"

clear_auth

print_summary
