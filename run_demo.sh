#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

OPEN_BROWSER=true
SERVER_PID=""
PROFILE_ARG=""
FAST_ARG=false
TICK_MS_ARG=""
INTERVAL_MIN_ARG=""
INTERVAL_MAX_ARG=""
INJECT_INTERVAL_ARG=""

usage() {
  echo "Usage: ./run_demo.sh [--no-browser|--browser] [--profile random|test] [--fast] [profile options]"
  echo ""
  echo "Profiles:"
  echo "  --profile random   local interactive demo traffic"
  echo "    --interval-min SECONDS     random profile default: 5.00"
  echo "    --interval-max SECONDS     random profile default: 20.00"
  echo "    --inject-interval SECONDS  random profile default: 5"
  echo "    --fast                     use faster local random defaults: 0.10..0.30s, inject 1s"
  echo ""
  echo "  --profile test     deterministic demo traffic for UI tests"
  echo "    --tick-ms MS               test profile default: 100"
  echo "    --fast                     use faster deterministic default: tick 20ms"
  echo ""
  echo "Environment variable fallback:"
  echo "  DEMO_PROFILE            random|test (default: random)"
  echo "  DEMO_UDP_INTERVAL_MIN   random profile default: 5.00"
  echo "  DEMO_UDP_INTERVAL_MAX   random profile default: 20.00"
  echo "  DEMO_INJECT_INTERVAL    random profile default: 5"
  echo "  DEMO_TEST_TICK_MS       test profile default: 100"
  echo "  DEMO_LOG_DIR            optional log directory override"
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --no-browser) OPEN_BROWSER=false ;;
    --browser) OPEN_BROWSER=true ;;
    --profile)
      [ "$#" -ge 2 ] || { echo "ERROR: --profile requires a value"; usage; exit 1; }
      PROFILE_ARG="$2"
      shift
      ;;
    --fast) FAST_ARG=true ;;
    --tick-ms)
      [ "$#" -ge 2 ] || { echo "ERROR: --tick-ms requires a value"; usage; exit 1; }
      TICK_MS_ARG="$2"
      shift
      ;;
    --interval-min)
      [ "$#" -ge 2 ] || { echo "ERROR: --interval-min requires a value"; usage; exit 1; }
      INTERVAL_MIN_ARG="$2"
      shift
      ;;
    --interval-max)
      [ "$#" -ge 2 ] || { echo "ERROR: --interval-max requires a value"; usage; exit 1; }
      INTERVAL_MAX_ARG="$2"
      shift
      ;;
    --inject-interval)
      [ "$#" -ge 2 ] || { echo "ERROR: --inject-interval requires a value"; usage; exit 1; }
      INJECT_INTERVAL_ARG="$2"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1"
      usage
      exit 1
      ;;
  esac
  shift
done

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
  [ -z "$pids" ] && return 0

  # Ask all children to stop gracefully first.
  echo "$pids" | xargs kill 2>/dev/null || true

  # Give embed-log server extra time to handle SIGINT/SIGTERM and export session.html.
  if [ -n "${SERVER_PID:-}" ] && kill -0 "$SERVER_PID" 2>/dev/null; then
    for _ in 1 2 3 4 5 6 7 8 9 10; do
      sleep 0.3
      if ! kill -0 "$SERVER_PID" 2>/dev/null; then
        break
      fi
    done
  fi

  # Short grace for remaining children.
  sleep 0.4

  # Force stop anything still running.
  local still
  still=$(jobs -p || true)
  if [ -n "$still" ]; then
    echo "$still" | xargs kill -9 2>/dev/null || true
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
  [[ "$cmd" == *"utils/inject_log_demo.py"* ]] || \
  [[ "$cmd" == *"utils/deterministic_demo_traffic.py"* ]]
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

_is_demo_traffic_cmd() {
  local cmd="$1"
  case "$cmd" in
    *"utils/udp_log_simulator.py"*)
      [[ "$cmd" == *"127.0.0.1:6000"* ]] || \
      [[ "$cmd" == *"127.0.0.1:6001"* ]] || \
      [[ "$cmd" == *"127.0.0.1:6002"* ]]
      ;;
    *"utils/deterministic_demo_traffic.py"*)
      [[ "$cmd" == *"127.0.0.1:6000"* ]] || \
      [[ "$cmd" == *"127.0.0.1:6001"* ]] || \
      [[ "$cmd" == *"127.0.0.1:6002"* ]]
      ;;
    *"utils/inject_log_demo.py"*)
      [[ "$cmd" == *" 5001"* ]] || \
      [[ "$cmd" == *" 5002"* ]] || \
      [[ "$cmd" == *" 5003"* ]]
      ;;
    *)
      return 1
      ;;
  esac
}

_reap_stale_demo_traffic() {
  local pid cmd
  while IFS= read -r line; do
    pid="${line%% *}"
    cmd="${line#* }"
    [ -n "$pid" ] || continue
    if _is_demo_traffic_cmd "$cmd"; then
      echo "Releasing stale demo traffic process (pid $pid)..."
      _kill_pid_and_wait "$pid"
    fi
  done < <(ps ax -o pid=,command= 2>/dev/null | awk '{$1=$1; print}')
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

echo "Checking demo ports..."
_reap_stale_demo_traffic
for p in 5001 5002 5003; do
  _free_port_if_stale tcp "$p" || exit 1
done
for p in 6000 6001 6002; do
  _free_port_if_stale udp "$p" || exit 1
done

# Use fixed UI port (8080 unless user overrides via CLI/config outside this script).
WS_PORT=8080
_free_port_if_stale tcp "$WS_PORT" || exit 1

DEMO_LOG_DIR="${DEMO_LOG_DIR:-}"
LOG_DIR_ARGS=()
if [ -n "$DEMO_LOG_DIR" ]; then
  LOG_DIR_ARGS=(--log-dir "$DEMO_LOG_DIR")
fi

echo "Starting embed-log server (YAML config) on port $WS_PORT in -v mode..."
if [ -n "$DEMO_LOG_DIR" ]; then
  echo "Demo logs directory override: $DEMO_LOG_DIR"
fi
if [ "$OPEN_BROWSER" = true ]; then
  "$PYTHON" backend/server.py run --config embed-log.demo.yml --ws-port "$WS_PORT" "${LOG_DIR_ARGS[@]}" -v &
else
  "$PYTHON" backend/server.py run --config embed-log.demo.yml --ws-port "$WS_PORT" "${LOG_DIR_ARGS[@]}" --no-open-browser -v &
fi
SERVER_PID=$!

sleep 1
if ! kill -0 "$SERVER_PID" 2>/dev/null; then
  echo "ERROR: embed-log server failed to start."
  echo "Tip: inspect logs above for bind errors."
  exit 1
fi

DEMO_PROFILE="${PROFILE_ARG:-${DEMO_PROFILE:-random}}"

case "$DEMO_PROFILE" in
  random)
    [ -z "$TICK_MS_ARG" ] || {
      echo "ERROR: --tick-ms can only be used with --profile test"
      exit 1
    }

    if [ -n "$INTERVAL_MIN_ARG" ]; then
      DEMO_UDP_INTERVAL_MIN="$INTERVAL_MIN_ARG"
    elif [ -n "${DEMO_UDP_INTERVAL_MIN+x}" ]; then
      DEMO_UDP_INTERVAL_MIN="$DEMO_UDP_INTERVAL_MIN"
    elif [ "$FAST_ARG" = true ]; then
      DEMO_UDP_INTERVAL_MIN="0.10"
    else
      DEMO_UDP_INTERVAL_MIN="5.00"
    fi

    if [ -n "$INTERVAL_MAX_ARG" ]; then
      DEMO_UDP_INTERVAL_MAX="$INTERVAL_MAX_ARG"
    elif [ -n "${DEMO_UDP_INTERVAL_MAX+x}" ]; then
      DEMO_UDP_INTERVAL_MAX="$DEMO_UDP_INTERVAL_MAX"
    elif [ "$FAST_ARG" = true ]; then
      DEMO_UDP_INTERVAL_MAX="0.30"
    else
      DEMO_UDP_INTERVAL_MAX="20.00"
    fi

    if [ -n "$INJECT_INTERVAL_ARG" ]; then
      DEMO_INJECT_INTERVAL="$INJECT_INTERVAL_ARG"
    elif [ -n "${DEMO_INJECT_INTERVAL+x}" ]; then
      DEMO_INJECT_INTERVAL="$DEMO_INJECT_INTERVAL"
    elif [ "$FAST_ARG" = true ]; then
      DEMO_INJECT_INTERVAL="1"
    else
      DEMO_INJECT_INTERVAL="5"
    fi

    echo "Starting UDP simulator (interval ${DEMO_UDP_INTERVAL_MIN}-${DEMO_UDP_INTERVAL_MAX}s)..."
    "$PYTHON" utils/udp_log_simulator.py \
      --target 127.0.0.1:6000 \
      --target 127.0.0.1:6001 \
      --target 127.0.0.1:6002 \
      --interval-min "$DEMO_UDP_INTERVAL_MIN" \
      --interval-max "$DEMO_UDP_INTERVAL_MAX" &

    echo "Starting marker injector (interval ${DEMO_INJECT_INTERVAL}s)..."
    "$PYTHON" utils/inject_log_demo.py \
      --inject SENSOR_A 5001 \
      --inject SENSOR_B 5002 \
      --inject SENSOR_C 5003 \
      --interval "$DEMO_INJECT_INTERVAL" \
      --duration 0 \
      --source demo &
    ;;
  test)
    [ -z "$INTERVAL_MIN_ARG" ] || {
      echo "ERROR: --interval-min can only be used with --profile random"
      exit 1
    }
    [ -z "$INTERVAL_MAX_ARG" ] || {
      echo "ERROR: --interval-max can only be used with --profile random"
      exit 1
    }
    [ -z "$INJECT_INTERVAL_ARG" ] || {
      echo "ERROR: --inject-interval can only be used with --profile random"
      exit 1
    }

    if [ -n "$TICK_MS_ARG" ]; then
      DEMO_TEST_TICK_MS="$TICK_MS_ARG"
    elif [ -n "${DEMO_TEST_TICK_MS+x}" ]; then
      DEMO_TEST_TICK_MS="$DEMO_TEST_TICK_MS"
    elif [ "$FAST_ARG" = true ]; then
      DEMO_TEST_TICK_MS="20"
    else
      DEMO_TEST_TICK_MS="100"
    fi
    echo "Starting deterministic demo traffic (tick ${DEMO_TEST_TICK_MS}ms)..."
    "$PYTHON" utils/deterministic_demo_traffic.py \
      --udp SENSOR_A=127.0.0.1:6000 \
      --udp SENSOR_B=127.0.0.1:6001 \
      --udp SENSOR_C=127.0.0.1:6002 \
      --tick-ms "$DEMO_TEST_TICK_MS" \
      --cycles 0 &
    ;;
  *)
    echo "ERROR: invalid profile: $DEMO_PROFILE (expected random|test)"
    exit 1
    ;;
esac

echo ""
echo "Demo running!"
echo "Open: http://127.0.0.1:${WS_PORT}/"
echo "Press Ctrl+C to stop all processes."

wait
