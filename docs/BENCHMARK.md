# Stress benchmark for embed-log

This directory contains `serial_stress.py`, a cross-platform stress benchmark
for the embed-log backend. It simulates multiple UART sources over TCP
(via pyserial `socket://` URLs), runs the real backend against them, and
reports whether frames were lost, duplicated, or corrupted.

No physical serial hardware is required. The benchmark works on macOS, Linux,
and Windows.

## Quick start

```bash
# From the embed-log repo root, with the virtualenv active:

# Smoke test — 1 source, 10 seconds, 100 lines/sec
python benchmarks/serial_stress.py \
  --sources 1 \
  --duration 10 \
  --line-rate 100 \
  --mode disk-only

# Four-source baseline (takes ~1 minute)
python benchmarks/serial_stress.py \
  --sources 4 \
  --duration 60 \
  --line-rate 1000 \
  --mode disk-only
```

A typical `PASS` output looks like:

```text
PASS disk-only sources=1 duration=10s rate=100 lps/source
generated=1001 sent=1001 logged=1001 missing=0 duplicates=0 corrupt=0
report=.benchmark-runs/report.json
```

## How it works

```text
benchmarks/serial_stress.py

  ┌────────────────────────────┐
  │ virtual UART TCP servers   │  one per source, dynamically allocated ports
  │ send numbered BENCH frames │
  └─────────────┬──────────────┘
                │ socket://127.0.0.1:<port>
                ▼
  ┌────────────────────────────┐
  │ embed-log backend process  │  real CLI/config path
  │  -m backend.cli run …      │
  └─────────────┬──────────────┘
                │ session log files
                ▼
  ┌────────────────────────────┐
  │ verifier/parser            │
  │ missing/duplicate/order    │
  └────────────────────────────┘
```

1. The benchmark starts one TCP server per source on ephemeral ports.
2. A temporary YAML config is generated pointing each source at
   `socket://127.0.0.1:<port>`.
3. The embed-log backend is launched as a subprocess (`embed-log run` or `python -m backend.server run`
4. Once all backend connections are detected, each producer sends
   deterministic numbered frames at the configured rate.
5. After the test duration, producers are stopped and a drain wait gives
   the backend time to flush queued frames.
6. The backend is shut down gracefully (SIGINT).
7. Session log files are parsed and verified against the frames that were
   actually sent over TCP.
8. A JSON report and terminal summary are produced.

## Frame format

Each generated line looks like:

```text
BENCH src=SRC0 seq=000000001 t_ns=1779472263487022000 payload=xxxxxxxx
```

- `src` — source name (`SRC0`, `SRC1`, …)
- `seq` — zero-padded 9‑digit sequence number (starts at 1)
- `t_ns` — monotonic timestamp in nanoseconds
- `payload` — filler of the configured `--payload-bytes` size

## CLI reference

```
usage: serial_stress.py [-h] [--sources SOURCES] [--duration DURATION]
                        [--line-rate LINE_RATE]
                        [--payload-bytes PAYLOAD_BYTES]
                        [--mode {disk-only,ws-server-no-client}]
                        [--baud BAUD] [--logs-root LOGS_ROOT]
                        [--report REPORT] [--keep-temp]
                        [--startup-timeout STARTUP_TIMEOUT]
                        [--shutdown-timeout SHUTDOWN_TIMEOUT]
                        [--drain-wait DRAIN_WAIT]
```

| Argument | Default | Description |
|---|---|---|
| `--sources` | `4` | Number of simulated UART sources |
| `--duration` | `60` | Test duration in seconds |
| `--line-rate` | `1000` | Target lines/sec per source |
| `--payload-bytes` | `80` | Size of the payload field per line |
| `--mode` | `disk-only` | Benchmark mode (see below) |
| `--baud` | `921600` | Informational baudrate (ignored by socket://) |
| `--logs-root` | `.benchmark-runs` | Root directory for session logs and reports |
| `--report` | `<logs-root>/report.json` | Path for the JSON report file |
| `--keep-temp` | — | Do not delete temporary config files after the run |
| `--startup-timeout` | `15.0` | Seconds to wait for the backend to become ready |
| `--shutdown-timeout` | `10.0` | Seconds to wait for the backend to exit after SIGINT |
| `--drain-wait` | `1.0` | Seconds between stopping producers and sending SIGINT |

## Benchmark modes

### `disk-only` (default)

- WebSocket server disabled (`ws_port: 0`)
- No UI / browser
- Isolates the UART read + queue + disk path
- **Use for**: measuring raw ingest throughput, finding pyserial `readline()`
  bottlenecks.

### `ws-server-no-client`

- WebSocket server enabled on an ephemeral port
- No connected client
- Measures the overhead of running the UI server even when no one is connected
- **Use for**: quantifying the cost of the WS server loop when no clients drain.

### `ws-fast-client` *(future)*

- WebSocket server + a fast-draining benchmark client
- **Use for**: measuring end-to-end throughput with WS fanout.

## Output report

A JSON report is written to the path given by `--report` (default:
`<logs-root>/report.json`).

Example:

```json
{
  "ok": true,
  "mode": "disk-only",
  "sources": 4,
  "duration_sec": 60,
  "line_rate_per_source": 1000,
  "payload_bytes": 80,
  "backend": {
    "returncode": 0,
    "session_dir": ".benchmark-runs/2026-05-22_19-59-08"
  },
  "totals": {
    "generated": 109222,
    "sent": 109219,
    "logged": 75083,
    "missing": 34139,
    "duplicates": 0,
    "corrupt": 0
  },
  "per_source": {
    "SRC0": {
      "generated": 29060,
      "sent": 29059,
      "logged": 18819,
      "missing": 10241,
      "duplicates": 0,
      "out_of_order": 0,
      "corrupt": 0,
      "send_blocked_sec": 37.8261
    }
  }
}
```

### Key fields

| Field | Meaning |
|---|---|
| `totals.generated` | Frames the producer attempted to generate |
| `totals.sent` | Frames the producer successfully wrote to the TCP socket |
| `totals.logged` | Unique frames found in the backend session log files |
| `totals.missing` | Frames sent over TCP but not present in the logs |
| `totals.duplicates` | Sequence numbers appearing more than once in the logs |
| `totals.corrupt` | Lines containing `BENCH` that don't match the expected format |
| `per_source.*.send_blocked_sec` | Cumulative seconds the producer spent blocked on `sendall()` (backpressure indicator) |

### PASS / FAIL criteria

The benchmark exits with code **0 (PASS)** only when **all** of these hold:

- Backend process return code is `0`
- `missing == 0`
- `duplicates == 0`
- `corrupt == 0`

If any frames are missing, duplicated, or corrupted, the result is
**FAIL** and the exit code is **1**.

## Profiles

Quick-reference invocations for common scenarios:

### smoke

```bash
python benchmarks/serial_stress.py \
  --sources 1 --duration 10 --line-rate 100 --payload-bytes 8 \
  --mode disk-only \
  --logs-root .benchmark-runs/smoke
```

### baseline

```bash
python benchmarks/serial_stress.py \
  --sources 4 --duration 60 --line-rate 1000 --payload-bytes 80 \
  --mode disk-only \
  --report .benchmark-runs/baselines/disk-only.json
```

### ws-overhead

```bash
python benchmarks/serial_stress.py \
  --sources 4 --duration 60 --line-rate 1000 --payload-bytes 80 \
  --mode ws-server-no-client \
  --report .benchmark-runs/baselines/ws-server-no-client.json
```

## Interpreting results

- **`generated ≈ sent`** — the producer kept up with the target rate;
  `send_blocked_sec` is near zero.
- **`generated > sent`** — the producer hit backpressure (TCP send buffer
  full). `send_blocked_sec` shows how long it was blocked.
- **`sent ≈ logged`** — the backend captured everything it received over
  the wire.
- **`sent > logged`** (missing > 0) — frames were received by pyserial but
  not written to disk. Possible causes:
  - pyserial `readline()` reads one byte at a time and can't keep up
    with the producer rate.
  - The writer thread queue had a backlog that wasn't drained before
    shutdown (mitigated by `--drain-wait`).

## Known bottlenecks

The current backend uses pyserial's `readline()` which internally calls
`read(1)` (one byte at a time). For a ~165‑byte line this requires ~165
`select()` + `recv()` syscalls per line. At high line rates this becomes
the primary bottleneck:

- Single-source throughput ceiling ≈ 400–500 lines/sec
- Four-source aggregate ≈ 1800 lines/sec
- Target 4000 lines/sec (4 × 1000) produces significant backpressure

This is a measurement, not a bug. The benchmark is designed to track
improvements as the backend is optimised (e.g. buffered reads, zero-copy
paths).

## Requirements

- Python 3.10+
- pyserial (`pip install pyserial`)
- PyYAML (`pip install pyyaml`)
- embed-log backend (local checkout, virtualenv active)
