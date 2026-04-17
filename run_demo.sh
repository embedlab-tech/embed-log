#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

# Prefer project venv interpreter when available (pick one that actually works)
for CAND in .venv/bin/python3.14 .venv/bin/python3 .venv/bin/python python3 python; do
  if [ -x "$CAND" ] || command -v "$CAND" >/dev/null 2>&1; then
    if "$CAND" - <<'PY' >/dev/null 2>&1
import sys
print(sys.version)
PY
    then
      PYTHON="$CAND"
      break
    fi
  fi
done

if [ -z "${PYTHON:-}" ]; then
  echo "ERROR: no working python interpreter found"
  exit 1
fi

# Ensure runtime deps for YAML demo mode exist (for the SAME interpreter)
if ! "$PYTHON" - <<'PY' >/dev/null 2>&1
import yaml, aiohttp, serial
PY
then
  echo "Installing/updating Python dependencies for $PYTHON ..."

  # Prefer pip bound to this interpreter.
  if ! "$PYTHON" -m pip --version >/dev/null 2>&1; then
    "$PYTHON" -m ensurepip --upgrade >/dev/null 2>&1 || true
  fi

  if "$PYTHON" -m pip --version >/dev/null 2>&1; then
    "$PYTHON" -m pip install -r requirements.txt
  elif [ -x .venv/bin/pip3.14 ]; then
    .venv/bin/pip3.14 install -r requirements.txt
  elif [ -x .venv/bin/pip3 ]; then
    .venv/bin/pip3 install -r requirements.txt
  else
    echo "ERROR: pip is unavailable for $PYTHON"
    echo "Run manually with a matching interpreter, e.g.: .venv/bin/python3.14 -m pip install -r requirements.txt"
    exit 1
  fi
fi

cleanup() {
  echo ""
  echo "Stopping demo..."
  local pids
  pids=$(jobs -p || true)
  if [ -n "$pids" ]; then
    # Graceful stop first
    echo "$pids" | xargs kill 2>/dev/null || true
    sleep 0.6
    # Force stop anything still running
    echo "$pids" | xargs kill -9 2>/dev/null || true
  fi
}
trap cleanup EXIT INT TERM

# -----------------------------------------------------------------------------
# Preflight: free demo ports from stale embed-log/demo processes.
# If a port is occupied by a non-embed-log process, abort with a clear message.
# -----------------------------------------------------------------------------
_is_embedlog_demo_pid() {
  local pid="$1"
  local cmd
  cmd=$(ps -p "$pid" -o command= 2>/dev/null || true)
  [[ "$cmd" == *"backend/server.py"* ]] || \
  [[ "$cmd" == *"utils/udp_log_simulator.py"* ]] || \
  [[ "$cmd" == *"utils/inject_log_demo.py"* ]]
}

_port_pids() {
  local proto="$1"   # tcp|udp
  local port="$2"
  if [ "$proto" = "tcp" ]; then
    lsof -tiTCP:"$port" -sTCP:LISTEN 2>/dev/null || true
  else
    lsof -tiUDP:"$port" 2>/dev/null || true
  fi
}

_kill_pid_and_wait() {
  local pid="$1"
  kill "$pid" 2>/dev/null || true
  for _ in 1 2 3 4 5; do
    sleep 0.15
    if ! kill -0 "$pid" 2>/dev/null; then
      return 0
    fi
  done
  kill -9 "$pid" 2>/dev/null || true
}

_free_port_if_stale() {
  local proto="$1"   # tcp|udp
  local port="$2"
  local pids
  pids=$(_port_pids "$proto" "$port")

  [ -z "$pids" ] && return 0

  local blocked=0
  for pid in $pids; do
    if _is_embedlog_demo_pid "$pid"; then
      echo "Releasing stale $proto port $port (pid $pid)..."
      _kill_pid_and_wait "$pid"
    else
      echo "ERROR: $proto port $port is in use by non-demo process (pid $pid)."
      ps -p "$pid" -o command= 2>/dev/null || true
      blocked=1
    fi
  done

  if [ "$blocked" -ne 0 ]; then
    return 1
  fi

  # verify free
  if [ -n "$(_port_pids "$proto" "$port")" ]; then
    echo "ERROR: could not free $proto port $port"
    return 1
  fi
  return 0
}

_find_free_tcp_port() {
  local start="$1"
  local end="$2"
  local p
  for ((p=start; p<=end; p++)); do
    if [ -z "$(_port_pids tcp "$p")" ]; then
      echo "$p"
      return 0
    fi
  done
  return 1
}

echo "Checking demo ports..."
for p in 5001 5002 5003; do
  _free_port_if_stale tcp "$p" || exit 1
done
for p in 6000 6001 6002; do
  _free_port_if_stale udp "$p" || exit 1
done

# Prefer 8080, but auto-fallback to next free port for better UX.
WS_PORT=8080
if ! _free_port_if_stale tcp "$WS_PORT"; then
  echo "Port 8080 unavailable; searching fallback port..."
  WS_PORT=$(_find_free_tcp_port 8081 8099 || true)
  if [ -z "$WS_PORT" ]; then
    echo "ERROR: no free fallback port in range 8081-8099"
    exit 1
  fi
fi

echo "Starting embed-log server (YAML config) on port $WS_PORT..."
"$PYTHON" backend/server.py run --config embed-log.demo.yml --ws-port "$WS_PORT" &
SERVER_PID=$!

sleep 1
if ! kill -0 "$SERVER_PID" 2>/dev/null; then
  echo "ERROR: embed-log server failed to start."
  echo "Tip: inspect logs above for bind errors."
  exit 1
fi

echo "Starting UDP simulator..."
"$PYTHON" utils/udp_log_simulator.py \
  --target 127.0.0.1:6000 \
  --target 127.0.0.1:6001 \
  --target 127.0.0.1:6002 \
  --interval-min 5.00 \
  --interval-max 20.00 &

echo "Starting marker injector..."
"$PYTHON" utils/inject_log_demo.py \
  --inject SENSOR_A 5001 \
  --inject SENSOR_B 5002 \
  --inject SENSOR_C 5003 \
  --interval 5 \
  --duration 0 \
  --source demo &

echo ""
echo "Demo running!"
echo "Open: http://127.0.0.1:${WS_PORT}/"
echo "Press Ctrl+C to stop all processes."

wait
