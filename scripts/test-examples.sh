#!/usr/bin/env bash
# Run all example test scripts
# Usage: ./scripts/test-examples.sh

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m'

cd "$(dirname "$0")/.."

PASSED=0
FAILED=0
SKIPPED=0

for test_script in examples/*/test.sh; do
    example=$(dirname "$test_script" | sed 's|examples/||')

    if [ ! -x "$test_script" ]; then
        echo -e "${BLUE}[SKIP]${NC} ${example} (no test.sh)"
        (( SKIPPED++ )) || true
        continue
    fi

    echo -e "\n${BLUE}━━━ ${example} ━━━${NC}"
    if "$test_script"; then
        (( PASSED++ )) || true
    else
        echo -e "${RED}[FAIL]${NC} ${example}"
        (( FAILED++ )) || true
    fi
done

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "  Examples: ${GREEN}${PASSED} passed${NC}, ${RED}${FAILED} failed${NC}, ${BLUE}${SKIPPED} skipped${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
[ "$FAILED" -eq 0 ]
