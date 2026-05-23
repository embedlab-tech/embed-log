# Benchmark implementation review

Review date: 2026-05-22

Reviewed scope:

- `backend/sources/uart.py`
- `tests/test_app_parse_source.py`
- `benchmarks/serial_stress.py`
- local macOS smoke benchmark run

## Summary

The implementation is moving in the right direction: the benchmark is a separate tool, starts the real backend through the CLI, uses `socket://` as a cross-platform UART simulation transport, and generates deterministic numbered frames. This is a solid foundation for backend performance work.

There are a few important issues to fix before using this benchmark as a reliable source of truth for optimization/regression decisions.

## What looks good

- `UartSource` was changed from `serial.Serial(...)` to `serial.serial_for_url(...)`, enabling `socket://` while preserving normal UART ports.
- A parser test was added for `uart:socket://127.0.0.1:12345@921600`.
- The benchmark runs the backend as a subprocess through the normal CLI/config path, so it tests the real app instead of mocks.
- The benchmark has clear components for:
  - config generation,
  - backend process management,
  - virtual UART producers,
  - log verification,
  - JSON reporting.
- Local smoke test passed:

```bash
.venv/bin/python benchmarks/serial_stress.py \
  --sources 1 \
  --duration 2 \
  --line-rate 10 \
  --payload-bytes 8 \
  --mode disk-only \
  --logs-root .benchmark-runs/review-smoke \
  --report .benchmark-runs/review-smoke/report.json
```

Result:

```text
PASS disk-only sources=1 duration=2s rate=10 lps/source
generated=21 sent=21 logged=21 missing=0 duplicates=0 corrupt=0
```

## Main issues to fix

### 1. `PASS` ignores missing/duplicate/corrupt frames

Currently `report["ok"]` is based only on the backend process return code:

```python
"ok": returncode == 0
```

This means the benchmark can print `PASS` even if frames are missing, duplicated, or corrupted.

Recommendation:

- `ok` should include at least:
  - `returncode == 0`
  - `missing == 0`
  - `duplicates == 0`
  - `corrupt == 0`
- optionally add CLI thresholds later, e.g. `--allow-missing`, `--allow-duplicates`, `--allow-corrupt`.

### 2. `ws-fast-client` is not actually implemented

The parser allows this mode:

```text
ws-fast-client
```

But in `run_benchmark()`, the WebSocket port is assigned only for `ws-server-no-client`. For `ws-fast-client`, `ws_port` remains `0`, so the benchmark effectively behaves like `disk-only`.

Confirmed locally:

```text
mode=ws-fast-client
ws_port=0
```

Recommendation:

- either temporarily remove `ws-fast-client` from `choices`,
- or implement it fully:
  - assign a WebSocket port,
  - start a WebSocket client,
  - drain messages quickly until the end of the test,
  - report received WS message/event counts.

### 3. Cross-platform readiness is not Windows-ready yet

`BackendProcess._drain_stderr_into()` implements non-blocking pipe reads only on non-Windows platforms. On Windows it currently does `pass`, so the benchmark will likely fail to detect `log server running` and time out.

There is also a risk that `_read_stderr()` blocks if called while the process is still running.

Recommendation:

- replace manual non-blocking pipe reads with background reader threads:
  - one thread for stdout,
  - one thread for stderr,
  - store output in `deque(maxlen=...)` buffers,
  - detect readiness from those buffers.
- This approach should work on macOS, Linux, and Windows.

### 4. Fixed ports `20000 + i` can conflict

`resolve_ports()` always returns ports starting from `20000`.

Risks:

- the port may already be used by another process,
- parallel benchmark runs will collide,
- fast restarts may hit temporary port availability issues.

Recommendation:

- allocate free ports dynamically by binding to port `0`,
- optionally keep `--base-port` only as a diagnostic/debug option.

### 5. Verification uses `generated` as the expected count

The verifier expects sequence numbers `1..generated`. If the producer generated a frame but failed to send it because of timeout/backpressure, the benchmark will count it as a backend missing frame.

Recommendation:

- clearly separate:
  - `producer_generated`,
  - `producer_sent`,
  - `backend_logged`.
- for backend correctness, compare `logged` mainly against frames actually `sent`.
- if `generated != sent`, report it separately as a producer/transport/backpressure issue.

### 6. No guaranteed drain before backend shutdown

The benchmark stops producers and immediately sends SIGINT to the backend. The current backend writer has `writer_thread.join(timeout=2.0)`, so under heavy backlog the process may exit before everything is written.

Recommendation:

- add `--drain-wait SEC`, default e.g. `1-3` seconds,
- later, after `/stats` exists, wait until backend queues reach zero,
- report drain time in the JSON output.

### 7. `.benchmark-runs/` is not ignored by git

`.benchmark-runs/` is currently untracked but not listed in `.gitignore`.

Recommendation:

```gitignore
.benchmark-runs/
```

## Smaller notes

- `VirtualUartProducer` mentions `BM frames`, while the actual prefix is `BENCH`. Standardize wording.
- `ProducerStats.connect_time` is set after `start_producing()`, so the name is misleading. It is closer to `start_time` or `produce_start_time`.
- `payload_bytes` means the size of the payload field, not the full frame. This is fine, but the CLI help should say `payload field size`.
- `find_latest_session_dir()` may pick an older session if backend startup fails or multiple tests run close together. Prefer a unique `logs_root` per run or a benchmark-specific `job_id`.
- `stdout_tail` / `stderr_tail` may miss output that was already consumed by `_drain_stderr_into()`. A central output buffer would be more reliable.

## Recommended fix order

### Step 1 — Fix PASS/FAIL criteria

- [ ] `ok = backend rc == 0 and missing == 0 and duplicates == 0 and corrupt == 0`
- [ ] optional CLI thresholds later
- [ ] benchmark exit code should depend on actual frame verification

### Step 2 — Fix benchmark modes

- [ ] Temporarily remove `ws-fast-client` from parser or fully implement it
- [ ] Assign WS ports consistently for `ws-server-no-client` and `ws-fast-client`
- [ ] Add received WS message/event counters when a WS client exists

### Step 3 — Make subprocess output reading portable

- [ ] background thread for stdout
- [ ] background thread for stderr
- [ ] `deque(maxlen=...)` output buffers
- [ ] readiness detection from buffered output
- [ ] no blocking `.read()` while the process is alive

### Step 4 — Use dynamic ports

- [ ] allocate ports via socket bind to `0`
- [ ] remove the fixed `20000+` assumption
- [ ] optionally add `--base-port` for debugging

### Step 5 — Clarify `sent` vs `generated` metrics

- [ ] verify backend logs against frames actually sent
- [ ] report producer-side issues separately when `generated != sent`
- [ ] expose producer backpressure as a separate status/warning

### Step 6 — Add drain before shutdown

- [ ] add `--drain-wait`, default e.g. `1.0`
- [ ] later replace this with `/stats`-based queue draining once backend stats exist

### Step 7 — Repository cleanup

- [ ] add `.benchmark-runs/` to `.gitignore`
- [ ] do not commit `benchmarks/__pycache__/`
- [ ] optionally add `BENCHMARK.md` with short benchmark usage docs

## Status against the original plan

- Phase 1: partially done and looks good.
- Phase 2: benchmark skeleton done.
- Phase 3: virtual UART producers done, but ports and metrics need refinement.
- Phase 4: verifier done, but PASS/FAIL logic needs fixing.
- Phase 5: macOS smoke test works.
- Phase 6+: WebSocket modes are not ready yet.

## Final recommendation

Do not use the current benchmark as a hard regression gate yet, because it can report `PASS` despite missing frames and `ws-fast-client` does not actually test WebSockets. After fixing items 1-4, it should become a very useful baseline tool for backend performance optimization.
