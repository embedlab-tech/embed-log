# Cross-platform backend benchmark plan

Goal: create a separate, repeatable benchmark that simulates multiple UART-like ports without physical hardware, stresses the backend, and reports whether frames were lost, duplicated, delayed, or only accepted at a lower-than-requested rate.

This benchmark should be separate from unit tests and runnable on macOS, Linux, and Windows.

## Chosen approach

Use **pyserial URL ports** with `socket://127.0.0.1:<port>` as the cross-platform simulated UART transport.

Why this path:

- works on macOS/Linux/Windows without external virtual-COM drivers
- keeps the backend path close to real UART usage because it still goes through `UartSource`
- avoids platform-specific pseudo-terminal setup
- lets the benchmark control producer rate, frame format, disconnects, and slow/backpressured links

Important prerequisite: update `UartSource` to use `serial.serial_for_url(...)` instead of `serial.Serial(...)`. `serial_for_url` still supports normal ports like `/dev/ttyUSB0`, `/dev/tty.usbserial-*`, and `COM3`, while also supporting URLs like `socket://...`.

Optional later: add a POSIX-only `pty` transport for closer local TTY behavior on macOS/Linux.

## Benchmark architecture

```text
benchmarks/serial_stress.py

  ┌────────────────────────────┐
  │ virtual UART TCP servers   │  one per source
  │ send numbered log frames   │
  └─────────────┬──────────────┘
                │ socket://127.0.0.1:<port>
                ▼
  ┌────────────────────────────┐
  │ embed-log backend process  │
  │ normal CLI/config path     │
  └─────────────┬──────────────┘
                │ session log files
                ▼
  ┌────────────────────────────┐
  │ verifier/parser            │
  │ missing/duplicate/order    │
  └────────────────────────────┘
```

The benchmark should:

1. create a temporary config file
2. start virtual UART servers
3. start the backend as a subprocess
4. generate deterministic numbered frames for each source
5. stop the backend cleanly
6. parse raw session log files
7. produce a JSON report and a short terminal summary

## Frame format

Each generated line should include enough data to verify correctness:

```text
BENCH src=SRC0 seq=000000001 t_ns=184467440000000000 payload=xxxxxxxx
```

Verifier should check per source:

- first sequence number
- last sequence number
- generated frames
- actually sent frames
- logged frames
- missing sequence numbers
- duplicate sequence numbers
- out-of-order sequence numbers
- corrupt/unparseable lines

## Benchmark modes

Initial modes:

1. `disk-only`
   - `ws_port: 0`
   - no browser/client
   - isolates UART read + queue + disk path

2. `ws-server-no-client`
   - WebSocket server enabled
   - no connected client
   - measures overhead of UI server existing

3. `ws-fast-client`
   - one benchmark WebSocket client connected and draining messages as fast as possible
   - measures backend WebSocket fanout cost

4. `ws-slow-client`
   - one WebSocket client connected but intentionally reading slowly
   - confirms slow UI does not break disk capture once sink isolation is implemented

Later modes:

5. `forward-fast-client`
6. `forward-slow-client`
7. `inject-client-active`
8. mixed UART + UDP stress

## CLI proposal

```bash
python benchmarks/serial_stress.py \
  --sources 4 \
  --duration 60 \
  --line-rate 1000 \
  --payload-bytes 80 \
  --mode disk-only \
  --logs-root .benchmark-runs
```

Useful options:

```text
--sources N                 number of simulated UART sources
--duration SEC              test duration
--line-rate N               target lines/sec per source
--payload-bytes N           payload size per line
--mode MODE                 disk-only/ws-server-no-client/ws-fast-client/ws-slow-client
--baud N                    config baudrate value, mostly informational for socket://
--logs-root DIR             where benchmark session logs go
--report FILE               write JSON report path
--keep-temp                 do not delete generated config/temp files
--startup-timeout SEC       backend startup timeout
--shutdown-timeout SEC      backend shutdown timeout
```

## Output report

Write JSON like:

```json
{
  "ok": true,
  "mode": "disk-only",
  "sources": 4,
  "duration_sec": 60,
  "line_rate_per_source": 1000,
  "payload_bytes": 80,
  "totals": {
    "generated": 240000,
    "sent": 240000,
    "logged": 240000,
    "missing": 0,
    "duplicates": 0,
    "corrupt": 0
  },
  "per_source": {
    "SRC0": {
      "generated": 60000,
      "sent": 60000,
      "logged": 60000,
      "missing": 0,
      "duplicates": 0,
      "out_of_order": 0,
      "corrupt": 0
    }
  },
  "backend": {
    "returncode": 0,
    "session_dir": "...",
    "stdout_tail": "...",
    "stderr_tail": "..."
  }
}
```

Terminal summary should be concise:

```text
PASS disk-only sources=4 duration=60s rate=1000 lps/source
sent=240000 logged=240000 missing=0 duplicates=0 corrupt=0
session=.benchmark-runs/2026-...
report=.benchmark-runs/report.json
```

## Progress checklist

### Phase 1 — Enable simulated UART URLs

- [ ] Change `backend/sources/uart.py` to use `serial.serial_for_url(...)`.
- [ ] Add/adjust a unit test proving `uart:socket://127.0.0.1:12345@921600` parses correctly.
- [ ] Confirm normal serial paths still work syntactically: `/dev/ttyUSB0`, `/dev/tty.usbserial-*`, `COM3`.

### Phase 2 — Build benchmark skeleton

- [ ] Create `benchmarks/serial_stress.py`.
- [ ] Add argument parsing.
- [ ] Add temp directory/config generation.
- [ ] Add subprocess launch for backend CLI:

```bash
python -m backend.cli run --config <temp-config.yml>
```

- [ ] Add clean shutdown via SIGINT/SIGTERM and timeout fallback.

### Phase 3 — Add virtual UART producers

- [ ] Implement one TCP server per source.
- [ ] Wait until backend connects to all servers before starting measurement.
- [ ] Generate deterministic numbered newline-delimited frames.
- [ ] Track generated count, accepted/sent count, and producer backpressure/send delays.
- [ ] Add rate control using monotonic time.

### Phase 4 — Add log verifier

- [ ] Locate generated session directory and source log files.
- [ ] Parse benchmark frame lines with regex.
- [ ] Count missing, duplicate, out-of-order, and corrupt frames.
- [ ] Return non-zero exit code on failed thresholds.

### Phase 5 — First smoke benchmark

- [ ] Run on macOS with one source:

```bash
python benchmarks/serial_stress.py --sources 1 --duration 10 --line-rate 100 --mode disk-only
```

- [ ] Confirm report is generated.
- [ ] Confirm missing/duplicate/corrupt counts are zero.

### Phase 6 — Four-source baseline

- [ ] Run current backend baseline:

```bash
python benchmarks/serial_stress.py --sources 4 --duration 60 --line-rate 1000 --payload-bytes 80 --mode disk-only
```

- [ ] Save report under `.benchmark-runs/baselines/`.
- [ ] Repeat with `ws-server-no-client`.
- [ ] Repeat with `ws-fast-client`.

### Phase 7 — WebSocket client modes

- [ ] Add simple benchmark WebSocket client.
- [ ] Implement fast-drain client.
- [ ] Implement slow client.
- [ ] Confirm disk logs remain complete even if UI is slow after sink isolation improvements.

### Phase 8 — Profiles and documentation

- [ ] Add named profiles:
  - `smoke`: short, low-rate, local sanity check
  - `baseline`: 4 sources, medium rate, 60 seconds
  - `stress`: higher rate, longer duration
- [ ] Document benchmark usage in `README.md` or `BENCHMARK.md`.
- [ ] Store example reports for before/after optimization comparison.

## Acceptance criteria

Initial acceptance:

- benchmark runs on macOS without physical serial hardware
- final transport choice can also run on Linux and Windows
- 4 simulated sources can be tested from one command
- report clearly separates:
  - frames generated by producer
  - frames actually sent/accepted by simulated transport
  - frames written to backend logs
- benchmark exits non-zero when missing/duplicate/corrupt frames exceed threshold

Performance acceptance after backend optimization:

- `disk-only` mode should show zero missing frames for the chosen baseline profile
- `ws-fast-client` should show zero missing disk frames
- `ws-slow-client` may drop UI messages only if configured to do so, but disk logs must remain complete
