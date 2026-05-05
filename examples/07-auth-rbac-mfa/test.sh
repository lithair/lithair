#!/usr/bin/env bash
# Test script for 07-auth-rbac-mfa example
# Tests LithairServer with RBAC (password-based auth)
#
# Usage: ./test.sh

source "$(dirname "$0")/../../scripts/test-lib.sh"

# Clean data from previous runs
rm -rf data/rbac_products*

start_server auth-rbac-mfa

API="${BASE_URL}/api/products"

# ── PUBLIC ACCESS ────────────────────────────────────────────────────────────
log_test "GET /api/products - public list (no auth)"
body=$(http_get "$API")
assert_status 200 "public list succeeds"

# ── PRODUCT CRUD ─────────────────────────────────────────────────────────────
log_test "POST /api/products - create product"
body=$(http_post "$API" '{"name":"Test Widget","description":"A test product","price":29.99,"stock":100,"cost":15.0}')
assert_status 201 "create product"
assert_contains "$body" "Test Widget" "response has product name"

# Extract the product ID (UUID format)
PRODUCT_ID=$(echo "$body" | sed -n 's/.*"id"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
if [ -n "$PRODUCT_ID" ]; then
    log_info "Created product ID: ${PRODUCT_ID:0:8}..."

    log_test "GET /api/products/$PRODUCT_ID - get by id"
    body=$(http_get "$API/$PRODUCT_ID")
    assert_status 200 "get product by id"
    assert_contains "$body" "Test Widget" "correct product"

    log_test "DELETE /api/products/$PRODUCT_ID - delete product"
    http_delete "$API/$PRODUCT_ID" >/dev/null
    assert_status 204 "delete product"

    log_test "GET /api/products/$PRODUCT_ID - deleted returns 404"
    http_get "$API/$PRODUCT_ID" >/dev/null
    assert_status 404 "deleted product gone"
fi

print_summary
