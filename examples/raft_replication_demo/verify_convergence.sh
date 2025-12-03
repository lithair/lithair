#!/usr/bin/env bash
set -euo pipefail

# Verify data convergence across 3 nodes by comparing /api/products arrays
# Requires: curl, python3

N1=${N1:-http://127.0.0.1:8080}
N2=${N2:-http://127.0.0.1:8081}
N3=${N3:-http://127.0.0.1:8082}
PATH_API=${PATH_API:-/api/products}

tmp1=$(mktemp)
tmp2=$(mktemp)
tmp3=$(mktemp)

cleanup_tmp() {
  rm -f "$tmp1" "$tmp2" "$tmp3" || true
}
trap cleanup_tmp EXIT

TIMEOUT=${CONVERGENCE_TIMEOUT:-30}
INTERVAL=${CONVERGENCE_INTERVAL:-1}
ATTEMPTS=$(( TIMEOUT / INTERVAL ))

for i in $(seq 1 "$ATTEMPTS"); do
  echo "🔍 Fetching data from nodes (attempt $i/$ATTEMPTS)..."
  curl -fsS "$N1$PATH_API" -o "$tmp1" || true
  curl -fsS "$N2$PATH_API" -o "$tmp2" || true
  curl -fsS "$N3$PATH_API" -o "$tmp3" || true

  if python3 - "$tmp1" "$tmp2" "$tmp3" <<'PY'
import json, sys
try:
    with open(sys.argv[1], 'r') as f: j1 = json.load(f)
    with open(sys.argv[2], 'r') as f: j2 = json.load(f)
    with open(sys.argv[3], 'r') as f: j3 = json.load(f)
except Exception:
    sys.exit(2)

def norm(arr):
    return sorted([(str(x.get('id', '')), x.get('name',''), x.get('price',0.0), x.get('category','')) for x in arr])

try:
    n1, n2, n3 = map(norm, (j1, j2, j3))
except Exception:
    sys.exit(2)

if not (n1 == n2 == n3):
    print('Mismatch detected across nodes:')
    print('len1=', len(n1), 'len2=', len(n2), 'len3=', len(n3))
    s1, s2, s3 = set(n1), set(n2), set(n3)
    print('only_in_1:', sorted(s1 - s2 - s3)[:5])
    print('only_in_2:', sorted(s2 - s1 - s3)[:5])
    print('only_in_3:', sorted(s3 - s1 - s2)[:5])
    sys.exit(1)
print('✅ Convergence OK. Items:', len(n1))
sys.exit(0)
PY
  then
    exit 0
  fi

  if [[ "$i" -lt "$ATTEMPTS" ]]; then
    sleep "$INTERVAL"
  fi
done

echo "❌ Convergence verification failed after ${TIMEOUT}s"
exit 1
