#!/bin/bash
set -euo pipefail

echo "🧪 Testing Docker CI configurations locally..."

# Test 1: Debian/Bookworm (main CI)
echo "📦 Testing rust:1-bookworm (main CI)..."
docker run --rm -v "$(pwd):/workspace" -w /workspace rust:1-bookworm bash -c "
  set -euo pipefail
  echo '🔧 Installing system deps...'
  apt-get update -qq
  apt-get install -y -qq curl jq lsof python3 build-essential

  echo '🛠️ Installing Task...'
  sh -c \"\$(curl --location https://taskfile.dev/install.sh)\" -- -d -b /usr/local/bin >/dev/null 2>&1

  echo '🚀 Running CI pipeline...'
  task ci:full
"

echo "✅ rust:1-bookworm test passed!"

# Test 2: Alpine (fast CI)
echo "🏔️ Testing rust:1-alpine (fast CI)..."
docker run --rm -v "$(pwd):/workspace" -w /workspace rust:1-alpine sh -c "
  set -euo pipefail
  echo '🔧 Installing system deps...'
  apk add --no-cache curl jq lsof python3 bash musl-dev gcc

  echo '🛠️ Installing Task...'
  sh -c \"\$(curl --location https://taskfile.dev/install.sh)\" -- -d -b /usr/local/bin >/dev/null 2>&1

  echo '🚀 Running CI pipeline...'
  task ci:full
"

echo "✅ rust:1-alpine test passed!"

echo "🎉 All Docker CI configurations work locally!"