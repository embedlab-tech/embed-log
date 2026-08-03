#!/bin/bash
# demo.sh — launch embed-log with the extended demo config.
#
# In another terminal, run:
#   python3 demo_traffic.py
#
# This development helper will be replaced by test-only fixtures during the
# MVP overhaul.

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

. "$HOME/.cargo/env" 2>/dev/null || true

cargo build -p embed-log-cli 2>&1 | tail -1
echo ""
echo "Starting CLI server with demo config…"
echo "Open http://127.0.0.1:8080/ in your browser."
echo "Run 'python3 demo_traffic.py' in another terminal to send test traffic."
echo ""
exec target/debug/embed-log run --config demo.yml --frontend-dir frontend
