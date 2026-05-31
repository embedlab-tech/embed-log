#!/usr/bin/env bash
# Run embed-log UI tests (Playwright).
# Usage: ./scripts/test-ui.sh [--debug] [--headed] [test-name-pattern]
set -euo pipefail

cd "$(dirname "$0")/.."

# Clean any leftover demo state
rm -rf tests-ui/.tmp/logs
mkdir -p tests-ui/.tmp

ARGS=("$@")
if [ ${#ARGS[@]} -eq 0 ]; then
  ARGS=("--reporter=list" "--timeout=30000")
fi

cd tests-ui
exec npx playwright test "${ARGS[@]}"
