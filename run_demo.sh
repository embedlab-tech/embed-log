#!/usr/bin/env bash
# Thin wrapper — delegates to `embed-log demo`.
set -euo pipefail

cd "$(dirname "$0")"

# Prefer project venv interpreter when available
for CAND in .venv/bin/python3.14 .venv/bin/python3 .venv/bin/python python3 python; do
  if [ -x "$CAND" ] || command -v "$CAND" >/dev/null 2>&1; then
    PYTHON="$CAND"
    break
  fi
done

if [ -z "${PYTHON:-}" ]; then
  echo "ERROR: no working python interpreter found"
  exit 1
fi

"$PYTHON" -m backend.server demo "$@"
