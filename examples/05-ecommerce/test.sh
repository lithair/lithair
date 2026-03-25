#!/usr/bin/env bash
# Test script for 05-ecommerce example
# Tests multi-model CRUD: Categories, Products, Orders

source "$(dirname "$0")/../../scripts/test-lib.sh"

rm -rf data/categories* data/products* data/orders*
PORT=8081

start_server ecommerce

CATS="${BASE_URL}/api/categories"
PRODS="${BASE_URL}/api/products"
ORDERS="${BASE_URL}/api/orders"

# ── CATEGORIES ────────────────────────────────────────────────────────────────
log_test "POST /api/categories"
body=$(http_post "$CATS" '{"id":"c1","name":"Electronics"}')
assert_status 201 "create category"
assert_contains "$body" "Electronics" "category name"

log_test "GET /api/categories"
body=$(http_get "$CATS")
assert_status 200 "list categories"

# ── PRODUCTS ──────────────────────────────────────────────────────────────────
log_test "POST /api/products"
body=$(http_post "$PRODS" '{"id":"p1","name":"Laptop","price":999.99,"stock":50,"category_id":"c1"}')
assert_status 201 "create product"
assert_contains "$body" "Laptop" "product name"

log_test "GET /api/products/p1"
body=$(http_get "$PRODS/p1")
assert_status 200 "get product"
assert_contains "$body" "999.99" "product price"

log_test "PATCH /api/products/p1 - update price"
body=$(http_patch "$PRODS/p1" '{"price":899.99}')
assert_status 200 "patch product price"
assert_contains "$body" "899.99" "price updated"

# ── ORDERS ────────────────────────────────────────────────────────────────────
log_test "POST /api/orders"
body=$(http_post "$ORDERS" '{"id":"o1","product_id":"p1","quantity":2,"status":"pending"}')
assert_status 201 "create order"

log_test "GET /api/orders"
body=$(http_get "$ORDERS")
assert_status 200 "list orders"

# ── CLEANUP ───────────────────────────────────────────────────────────────────
log_test "DELETE /api/products/p1"
http_delete "$PRODS/p1" >/dev/null
assert_status 204 "delete product"

print_summary
