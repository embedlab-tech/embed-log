#!/usr/bin/env bash
# Run embed-log backend (Python) tests.
# Usage: ./scripts/test-backend.sh [pytest/unittest args...]
set -euo pipefail

cd "$(dirname "$0")/.."

ARGS=("$@")
if [ ${#ARGS[@]} -eq 0 ]; then
  ARGS=("--verbose")
fi

exec python3 -m unittest discover -s tests "${ARGS[@]}"
