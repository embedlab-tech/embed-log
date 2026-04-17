# embed-log

Serial log server for embedded device CI. Reads UART output from one or more devices, writes it to timestamped log files, exposes a TCP socket per device so that test code (pytest, Robot Framework) can inject markers and send serial TX commands, and streams everything live to a browser UI over WebSocket.

---

## Quick backend audit (short)

### Implemented now
- **Multi-source ingestion** via `backend/server.py`: `uart:` and `udp:` sources.
- **Per-source manager pipeline**: source reader thread → ordered queue → writer thread → file + console + live streams.
- **Inject/TX channel per source** (`--inject NAME PORT`): accepts newline-delimited JSON for marker logs and TX commands.
- **Bidirectional inject sockets**: clients can both send messages and receive streamed entries on the same TCP connection.
- **Web UI transport** with `aiohttp` WebSocket (`/ws`) and static UI serving (`/`).
- **Client SDKs**:
  - `backend/log_client.py` for marker injection + background subscription.
  - `backend/tx_client.py` for TX-only flows.

### Good next backend improvements (possible)
- **Backpressure/retention controls** (bounded queues, drop counters, source health stats).
- **Operational endpoints** (`/health`, `/metrics`, `/stats`) for CI monitoring.
- **Auth/network hardening** for non-local deployments (token, stricter bind model).
- **Replay/search APIs** (server-side history window, regex search across buffered logs).

---

## Documentation index

- [INSTALL.md](INSTALL.md) — setup instructions
- [FRONTEND.md](FRONTEND.md) — frontend architecture and behavior
- [MERGE.md](MERGE.md) — offline merge/export tooling
- [DIRECTORY_GUIDE.md](DIRECTORY_GUIDE.md) — short explanation of each project directory
- [AGENTS.md](AGENTS.md) — quick onboarding notes for future coding agents/humans

---

## Project structure

```
embed-log/
├── README.md          this file
├── FRONTEND.md        browser UI architecture
├── MERGE.md           offline log merging with merge_logs.py
├── requirements.txt
│
├── frontend/          browser UI (HTML, CSS, JS — no build step)
│   ├── index.html
│   ├── viewer.css
│   ├── state.js  ansi.js  lines.js  tabs.js  ui.js  ws.js  export.js
│
├── backend/           server and client library
│   ├── server.py      log server (serial + TCP + WebSocket)
│   ├── log_client.py  marker + subscribe client API
│   └── tx_client.py   TX-only client API
│
└── utils/
    ├── merge_logs.py        offline HTML viewer generator
    ├── udp_log_simulator.py UDP traffic simulator for udp:PORT sources
    ├── sim_messages.txt     message corpus for simulator
    ├── inject_messages.txt  message corpus for inject demo
    └── inject_log_demo.py   inject-port marker demo client
```

---

## Problem it solves

When running tests against embedded hardware it is hard to correlate what the test did with what the device logged. embed-log merges both streams into one ordered log file and one live browser view:

```
[2026-03-25T11:49:50.100+01:00] boot complete, heap free: 62832
[2026-03-25T11:49:59.870+01:00] [demo] sending 'heap stat' command (cycle #1)
[2026-03-25T11:49:59.872+01:00] [TX::demo] heap stat
[2026-03-25T11:50:00.011+01:00] free: 62832, used: 93976
[2026-03-25T11:50:05.456+01:00] [test_reboot] resetting board
[2026-03-25T11:50:05.460+01:00] [TX::test_reboot] reboot
[2026-03-25T11:50:05.891+01:00] Restarting...
```

---

## Architecture

```
┌──────────────────────────────────────────────────────────────────────┐
│                             LogServer                                │
│                                                                      │
│  ┌─────────────────────────┐    ┌─────────────────────────┐         │
│  │    SourceManager A      │    │    SourceManager B      │         │
│  │    name = "READER"      │    │    name = "CONTROLLER"  │         │
│  │                         │    │                         │         │
│  │  LogSource (uart/udp)   │    │  LogSource (uart/udp)   │         │
│  │  udp) reader thread     │    │  udp) reader thread     │         │
│  │                         │    │                         │         │
│  │  inject server thread   │    │  inject server thread   │         │
│  │  TCP :5001 (RX+TX)      │    │  TCP :5002 (RX+TX)      │         │
│  │          │              │    │          │              │         │
│  │        queue            │    │        queue            │         │
│  │          │              │    │          │              │         │
│  │  writer thread          │    │  writer thread          │         │
│  │  → logs/<session>/...   │    │  → logs/<session>/...   │         │
│  └──────────┬──────────────┘    └──────────┬──────────────┘         │
│             │                              │                         │
│             └──────────────┬───────────────┘                         │
│                            │ broadcast(LogEntry)                     │
│                 ┌──────────▼──────────────┐                          │
│                 │   WebSocketBroadcaster  │                          │
│                 │   GET /  → index.html   │                          │
│                 │   GET /ws → WebSocket   │                          │
│                 │   HTTP + WS :8080       │                          │
│                 └─────────────────────────┘                          │
└──────────────────────────────────────────────────────────────────────┘
        ▲                              ▲
        │  TCP JSON lines (inject+RX)  │  TCP JSON lines (inject+RX)
  ┌─────┴──────┐                ┌──────┴─────┐
  │  pytest /  │                │  pytest /  │
  │  robot /   │                │  robot /   │
  │ inject_log_demo.py │         │ inject_log_demo.py │
  └────────────┘                └────────────┘
                          ▲
                          │  WebSocket (ws://:8080/ws)
                   ┌──────┴──────┐
                   │   Browser   │
                   │  index.html │
                   └─────────────┘
```

**Log sources are pluggable** — each named source can be a UART serial port (`uart:`) or a UDP listener (`udp:`). The rest of the pipeline is identical regardless of source type.

**One write queue per source** — source RX and injected markers are always in chronological order with no interleaving.

**UART auto-reconnects** — boards reset during flashing; the serial reader retries every 3 seconds silently.

**Inject port is bidirectional** — clients can inject log markers and TX commands (send JSON lines) and simultaneously receive a stream of all log entries for that source. Use `LogClient.subscribe()` for marker/stream workflows and `TxClient` for TX-only workflows.

**WebSocket broadcaster** — aiohttp serves the browser UI and a WebSocket endpoint on the same port. On connect it sends a `config` message with the tab layout so the browser builds the pane structure before any log data arrives. TX commands typed in the browser are sent back to the source via the same WebSocket.

See [FRONTEND.md](FRONTEND.md) for the browser UI architecture.
See [MERGE.md](MERGE.md) for offline log merging with `merge_logs.py`.

Session-related HTTP endpoints:
- `GET /api/session/current` — current session metadata and URLs
- `GET /api/sessions` — list available sessions
- `GET /sessions/<session_id>/<filename>` — open `session.html`, `manifest.json`, or raw logs

---

## Installation

```bash
pip install -r requirements.txt
```

Requirements: `pyserial`, `aiohttp`, `PyYAML`

---

## Running the server

All commands are run from the project root directory.

### Recommended: YAML config

```bash
# packaged entrypoint
embed-log run --config examples/embed-log.yml

# or plain python
python3 backend/server.py run --config examples/embed-log.yml
```

A full demo config is included in the repo root:

```bash
python3 backend/server.py run --config embed-log.demo.yml
```

### Legacy CLI (still supported)

```bash
python3 backend/server.py \
  --source DEVICE_A uart:/dev/ttyUSB0 \
  --source DEVICE_B uart:/dev/ttyUSB1 \
  --inject DEVICE_A 5001 \
  --inject DEVICE_B 5002 \
  --tab "Devices" DEVICE_A DEVICE_B \
  --ws-port 8080
```

Log files are written into a new per-run session directory under `logs.dir` / `--log-dir`, for example:

`logs/2026-04-17_23-02-25/<TAB>__<SOURCE>__2026-04-17_23-02-25.log`

Each session directory also contains:
- `manifest.json` (session metadata + file mapping)
- `session.html` (auto-exported when the server gets SIGINT/SIGTERM, and when the last WS client disconnects)

### YAML schema (v1)

```yaml
version: 1

server:
  host: 127.0.0.1
  ws_port: 8080
  ws_ui: frontend/index.html
  verbose: false

logs:
  dir: logs/

# optional default UART baudrate
baudrate: 115200

sources:
  - name: DUT_UART
    type: uart
    port: /dev/ttyUSB0
    inject_port: 5001

  - name: SENSOR_A
    type: udp
    port: 6000
    inject_port: 5002
    forward_ports: [7001]

tabs:
  - label: Devices
    panes: [DUT_UART, SENSOR_A]
```

### Source types

| Spec | Description |
|---|---|
| `uart:/dev/path` | UART serial port at default baud rate |
| `uart:/dev/path@9600` | UART with explicit baud rate |
| `udp:PORT` | Listen for UDP datagrams on PORT |

### All CLI options

```
  run                     optional subcommand alias (both styles work)
  --config, -c FILE       YAML config file (version: 1)

  --source NAME TYPE      NAME  uart:/dev/path[@baud] | udp:PORT
                          repeat for multiple sources
  --inject NAME PORT      TCP inject/stream port for a source (optional, repeat)
  --forward NAME PORT     read-only TCP forward port for raw RX lines (optional, repeat)
  --tab LABEL S1 [S2]     group 1 or 2 sources into a UI tab (optional, repeat)

  --baudrate BAUD         default baud rate for uart sources without @baud
  --log-dir DIR           log file directory
  --host HOST             bind host for inject ports and WebSocket UI
  --ws-port PORT          HTTP/WebSocket port for the browser UI (0 = disabled)
  --ws-ui FILE            path to the UI HTML file served at GET /
  -v, --verbose           prefix every line with [name][source]
  -h, --help
```

### Browser UI

When `--ws-port` is set (example):

```
http://127.0.0.1:8080/
```

`run_demo.sh` prefers 8080 but may auto-select another free port in `8081..8099`.

The UI streams both device panes live, supports per-pane filtering, cross-pane timestamp sync, and HTML export. It also includes:
- **Current HTML** toolbar button (open current session export)
- **Sessions** popup (browse/open saved session HTML files and manifests)

See [FRONTEND.md](FRONTEND.md).

---

## Log format

### Compact (default, no `-v`)

Serial lines are plain timestamped text. Injected markers and TX commands keep their `[source]` label.

```
[2026-03-25T11:50:09.900+01:00] free: 62832, used: 93976
[2026-03-25T11:49:59.870+01:00] [demo] sending 'heap stat' command (cycle #1)
[2026-03-25T11:49:59.872+01:00] [TX::demo] heap stat
```

### Verbose (`-v`)

Every line carries `[device][source]`:

```
[2026-03-25T11:50:09.900+01:00] [DEVICE_A] [SERIAL] free: 62832, used: 93976
[2026-03-25T11:49:59.870+01:00] [DEVICE_A] [demo] sending 'heap stat' command
[2026-03-25T11:49:59.872+01:00] [DEVICE_A] [TX::demo] heap stat
```

Timestamps are ISO 8601 with milliseconds and timezone offset.

ANSI color codes are embedded in the log file, so `tail -f` in a terminal shows colored output.

---

## Client APIs (`backend/log_client.py`, `backend/tx_client.py`)

### pytest

```python
from backend.log_client import LogClient

@pytest.fixture(scope="session")
def dut():
    with LogClient("127.0.0.1", 5001, source="pytest", connect_timeout=30) as client:
        yield client

def test_boot(dut):
    dut.step("flashing firmware")        # cyan marker in log
    # ... flash ...
    dut.success("firmware flashed OK")   # green marker
    dut.warning("waiting for reboot")    # yellow marker
```

### Robot Framework

```robotframework
Library    backend.log_client.LogClient    127.0.0.1    5001    source=robot

*** Test Cases ***
Heap Check
    Step    running heap stat
    Success    command scheduled
```

### All methods

| Method | Color | Description |
|---|---|---|
| `marker(msg, color=None, source=None)` | any | Write a marker line |
| `info(msg)` | white | Informational marker |
| `step(msg)` | cyan | Test step highlight |
| `success(msg)` | green | Pass / OK marker |
| `warning(msg)` | yellow | Warning marker |
| `error(msg)` | red | Error / fail marker |
| `subscribe(callback, daemon=True)` | — | Receive log stream in background thread |

Available colors: `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `white`, `bold`.

### Subscribe — receive logs without polluting stdout

The inject port streams every log entry back to connected clients. Use
`subscribe()` to consume that stream in a background thread. The callback
must **not** print to stdout — use a `queue.Queue` or `threading.Event`
to communicate with your test thread:

```python
import queue

log_q = queue.Queue()

@pytest.fixture(scope="session")
def dut():
    with LogClient("127.0.0.1", 5001, source="pytest") as client:
        client.subscribe(log_q.put)   # starts background thread, no stdout
        yield client

def test_boot(dut):
    dut.step("waiting for boot complete")
    # Wait for "boot complete" to appear in the log stream
    while True:
        entry = log_q.get(timeout=10)
        if "boot complete" in entry["message"]:
            break
```

### Connection options

```python
LogClient(
    host="127.0.0.1",
    port=5001,
    source="pytest",
    auto_reconnect=True,   # reconnect silently if connection drops
    connect_timeout=30,    # retry initial connect for up to 30 s (useful in CI)
)
```

### TX-only API

```python
from backend.tx_client import TxClient

with TxClient("127.0.0.1", 5001, source="pytest", connect_timeout=30) as tx:
    tx.sendline("reboot")
```

---

## TCP inject port protocol

Newline-delimited JSON. Each connected client simultaneously receives the
log stream and can inject commands.

**Inject** (client → server):

```bash
# Log marker
echo '{"type":"log","source":"manual","message":"board power cycled","color":"yellow"}' \
  | nc 127.0.0.1 5001

# Serial TX (uart sources only)
echo '{"type":"tx","source":"manual","data":"reboot\r\n"}' \
  | nc 127.0.0.1 5001
```

`type` defaults to `"log"` if omitted.

**Stream** (server → client): every log entry is sent back as a JSON line:

```json
{"source_id":"READER","source":"SERIAL","message":"boot ok","timestamp":"2026-03-27T11:49:50.100+01:00"}
```

---

## CI usage example

```yaml
# .gitlab-ci.yml / GitHub Actions

- name: Start embed-log
  run: |
    python3 backend/server.py \
      --source DEVICE_A uart:/dev/ttyUSB0 \
      --source DEVICE_B uart:/dev/ttyUSB1 \
      --inject DEVICE_A 5001 \
      --inject DEVICE_B 5002 \
      --tab "Devices" DEVICE_A DEVICE_B \
      --log-dir $CI_PROJECT_DIR/logs/ \
      --ws-port 8080 &
    echo $! > /tmp/embed-log.pid

- name: Run tests
  run: pytest tests/ -v

- name: Stop embed-log
  if: always()
  run: kill $(cat /tmp/embed-log.pid) || true
```

---

## Watching logs in the terminal

```bash
tail -f logs/READER.log
tail -f logs/CONTROLLER.log

# Both at once (requires multitail)
multitail logs/READER.log logs/CONTROLLER.log
```

---

## Testing without serial hardware (UDP simulator)

You can simulate log traffic into one or more `udp:PORT` sources using:

```bash
python3 utils/udp_log_simulator.py --help
```

### Example: two simulated devices

Start the server:

```bash
python3 backend/server.py \
  --source SENSOR_A udp:6000 \
  --source SENSOR_B udp:6001 \
  --tab "Simulated" SENSOR_A SENSOR_B \
  --ws-port 8080
```

In another terminal, start the simulator:

```bash
python3 utils/udp_log_simulator.py \
  --target 127.0.0.1:6000 \
  --target 127.0.0.1:6001 \
  --interval-min 0.05 \
  --interval-max 0.30
```

Each emitted line uses a local system timestamp and a random message selected
from `utils/sim_messages.txt` (severity tags: `<inf> <wrn> <dbg> <err>`).

### Handy CLI patterns

```bash
# Use shared host + multiple ports
python3 utils/udp_log_simulator.py --host 127.0.0.1 --port 6000 --port 6001

# Deterministic run (reproducible random sequence), finite number of lines
python3 utils/udp_log_simulator.py --target 127.0.0.1:6000 --count 200 --seed 42

# Custom message corpus
python3 utils/udp_log_simulator.py --target 127.0.0.1:6000 --messages ./my-messages.txt
```

---

## Inject demo utility (CLI aligned with server)

`utils/inject_log_demo.py` accepts repeated `--inject NAME PORT`, matching the
server CLI shape and source names.

```bash
# Server
python3 backend/server.py \
  --source SENSOR_A udp:6000 \
  --source SENSOR_B udp:6001 \
  --inject SENSOR_A 5001 \
  --inject SENSOR_B 5002 \
  --tab "Simulated Devices" SENSOR_A SENSOR_B \
  --ws-port 8080

# Inject randomized marker traffic to both sources
python3 utils/inject_log_demo.py \
  --inject SENSOR_A 5001 \
  --inject SENSOR_B 5002 \
  --interval 5 \
  --source demo
```

Messages are randomly selected from `utils/inject_messages.txt`.

