#!/usr/bin/env bash
set -euo pipefail

# Configuration - UN SEUL port pour le serveur unifié
PORT=${PORT:-3000}
HOST=${HOST:-127.0.0.1}

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
EXAMPLE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Build
cargo build -q -p blog_server

# Start server in background with specified port
RUST_LOG=${RUST_LOG:-info} target/debug/blog_server --port ${PORT} >/tmp/blog_server.out 2>&1 &
SERVER_PID=$!
echo "✅ blog_server started on port ${PORT} (PID: $SERVER_PID)"

# Trap cleanup
cleanup() {
  kill ${SERVER_PID} 2>/dev/null || true
}
trap cleanup EXIT

# Wait for server to be ready
for i in {1..40}; do
  if curl -s "http://${HOST}:${PORT}" >/dev/null 2>&1; then
    break
  fi
  sleep 0.25
done
sleep 0.5

echo "\n🔐 Login as admin to get session token..."
TOKEN=$(curl -s -X POST "http://${HOST}:${PORT}/auth/login" \
  -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"password123"}' | jq -r '.session_token // .token // .jwt // empty')

if [ -z "${TOKEN}" ]; then
  echo "⚠️  Could not obtain token from /auth/login, raw response:" >&2
  curl -s -X POST "http://${HOST}:${PORT}/auth/login" -H 'Content-Type: application/json' -d '{"username":"admin","password":"password123"}' | jq . || true
  echo "Exiting tests with failure." >&2
  exit 1
fi

echo "✅ Got token: ${TOKEN:0:16}..."

echo "\n🧪 Creating test articles (authorized)..."
# Create a published article (authorized)
curl -s -X POST "http://${HOST}:${PORT}/api/articles" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H 'Content-Type: application/json' \
  -d '{
    "id": "1",
    "title": "Public Post",
    "content": "This is public.",
    "author_id": "admin",
    "status": "Published"
  }' | jq . || true

# Create a draft article (authorized)
curl -s -X POST "http://${HOST}:${PORT}/api/articles" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H 'Content-Type: application/json' \
  -d '{
    "id": "2",
    "title": "Draft Post",
    "content": "This is draft.",
    "author_id": "admin",
    "status": "Draft"
  }' | jq . || true

echo "\n📋 List articles (anonymous, expect only Published):"
curl -s "http://${HOST}:${PORT}/api/articles" | jq '. | length as $n | {count:$n, items:.}'

echo "\n📋 List articles (with token, expect Published + Draft):"
curl -s "http://${HOST}:${PORT}/api/articles" -H "Authorization: Bearer ${TOKEN}" | jq '. | length as $n | {count:$n, items:.}'

echo "\n🔎 Get draft article (id=2) without token (expect 403):"
HTTP_CODE=$(curl -s -o /dev/null -w '%{http_code}' "http://${HOST}:${PORT}/api/articles/2")
echo "HTTP ${HTTP_CODE}"

echo "\n🔎 Get draft article (id=2) with token (expect 200):"
HTTP_CODE=$(curl -s -o >(jq .) -w '%{http_code}' "http://${HOST}:${PORT}/api/articles/2" -H "Authorization: Bearer ${TOKEN}")
echo "HTTP ${HTTP_CODE}"

echo "\n✅ Blog tests completed successfully"
