#!/usr/bin/env bash
# Test script for 04-blog example
# Tests DeclarativeModel CRUD with RBAC and session authentication
#
# Usage: ./test.sh

source "$(dirname "$0")/../../scripts/test-lib.sh"

# Clean data from previous runs
rm -rf data/blog*

PORT=3000
start_server blog --port "$PORT"

API="${BASE_URL}/api/articles"
AUTH="${BASE_URL}/auth"

# ── AUTH: LOGIN ──────────────────────────────────────────────────────────────
log_test "POST /auth/login - admin login"
body=$(http_post "$AUTH/login" '{"username":"admin","password":"password123"}')
assert_status 200 "admin login succeeds"
assert_contains "$body" "session_token" "response has session_token"

TOKEN=$(echo "$body" | sed -n 's/.*"session_token"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
if [ -z "$TOKEN" ]; then
    log_fail "could not extract session token"
    print_summary
fi
log_info "Got token: ${TOKEN:0:16}..."
set_auth "Bearer $TOKEN"

log_test "POST /auth/login - invalid credentials"
clear_auth
http_post "$AUTH/login" '{"username":"admin","password":"wrong"}' >/dev/null
assert_status 401 "reject invalid credentials"

# ── ARTICLE CRUD (AUTHENTICATED) ────────────────────────────────────────────
set_auth "Bearer $TOKEN"

log_test "POST /api/articles - create published article (authed)"
body=$(http_post "$API" '{"id":"1","title":"Public Post","content":"This is public.","author_id":"admin","status":"Published"}')
assert_status 201 "create published article"
assert_contains "$body" "Public Post" "response has title"

log_test "POST /api/articles - create draft article (authed)"
body=$(http_post "$API" '{"id":"2","title":"Draft Post","content":"This is draft.","author_id":"admin","status":"Draft"}')
assert_status 201 "create draft article"

# ── RBAC: ANONYMOUS ACCESS ──────────────────────────────────────────────────
clear_auth

log_test "GET /api/articles - anonymous list (published only)"
body=$(http_get "$API")
assert_status 200 "anonymous list succeeds"
assert_contains "$body" "Public Post" "anonymous sees published article"

log_test "GET /api/articles/2 - anonymous access to draft (expect 403)"
http_get "$API/2" >/dev/null
assert_status 403 "anonymous cannot read draft"

# ── RBAC: AUTHENTICATED ACCESS ──────────────────────────────────────────────
set_auth "Bearer $TOKEN"

log_test "GET /api/articles/2 - authed access to draft (expect 200)"
body=$(http_get "$API/2")
assert_status 200 "authed user can read draft"
assert_contains "$body" "Draft Post" "correct draft content"

# ── UPDATE ───────────────────────────────────────────────────────────────────
log_test "PUT /api/articles/1 - update article (authed)"
body=$(http_put "$API/1" '{"id":"1","title":"Updated Post","content":"Updated content.","author_id":"admin","status":"Published"}')
assert_status 200 "update article"
assert_contains "$body" "Updated Post" "title updated"

# ── DELETE ───────────────────────────────────────────────────────────────────
log_test "DELETE /api/articles/1 - delete article (authed)"
http_delete "$API/1" >/dev/null
assert_status 204 "delete article"

log_test "GET /api/articles/1 - deleted article returns 404"
http_get "$API/1" >/dev/null
assert_status 404 "deleted article gone"

# ── LOGOUT ───────────────────────────────────────────────────────────────────
log_test "POST /auth/logout - logout"
body=$(http_post "$AUTH/logout" '{}')
assert_status 200 "logout succeeds"

clear_auth

print_summary
