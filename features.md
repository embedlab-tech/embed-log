# Implemented Features

Living document of the `embed-log` codebase. Agents: update this file when adding or changing observable behavior.

---

## How to Use This File

- **Before starting work:** Read relevant sections to understand what exists.
- **After implementing a feature:** Add or update the entry. Include the test that covers it.
- **Format:** `| Feature | Scope | Test(s) | Notes |`

---

## Architecture

```
backend/
  app.py                   — source construction, run_app() entry point
  server.py                — thin __main__ entrypoint
  cli/                     — CLI package
    __init__.py            — re-exports main()
    dispatch.py            — main() dispatcher, match/case routing
    parser.py              — argparse construction for all subcommands
    util.py                — pure helpers: timestamps, durations, file stats, session ID
    wizard.py              — create-config interactive wizard
    diagnostics.py         — version / doctor / ports
    update.py              — self-update logic
    run.py                 — run / validate / merge
    sessions/
      __init__.py          — _run_sessions() dispatcher + re-exports
      list.py              — sessions list
      info.py              — sessions info
      logs.py              — sessions logs
      export.py            — sessions export (HTML + raw format)
      delete.py            — sessions delete
      open.py              — sessions open
      marker.py            — sessions marker list/show
      snippet.py           — sessions snippet list/show/delete
  config/
    loader.py              — YAML config parsing + validation
    models.py              — AppConfig, SourceConfig, TabConfig, ServerConfig, LogsConfig, ParserConfig
  core/
    runtime.py             — SourceManager, LogServer (thread-safe _session_info)
    queue.py               — TrackedQueue (bounded queue with saturation tracking)
    clock.py               — SessionClock (absolute/relative timestamp modes)
    models.py              — LogEntry(@dataclass(slots=True)), QueueStats
    naming.py              — slugify()
    ansi.py                — ANSI color codes dict
  net/
    ws_server.py           — WebSocketBroadcaster (aiohttp in background thread)
    inject_server.py       — InjectServer (bidirectional TCP)
    forward_server.py      — ForwardServer (read-only TCP mirror)
  sources/
    base.py                — LogSource ABC
    raw_base.py            — RawLogSource ABC
    parsed.py              — ParsedSource (raw + parser)
    uart.py, udp.py        — convenience wrappers
    raw_uart.py, raw_udp.py — raw source implementations
  parsers/
    base.py                — StreamParser ABC
    text.py                — TextParser (newline-delimited)
    cbor_datagram.py       — CborDatagramParser
    factory.py             — create_parser()
  session/
    manager.py             — SessionManager (manifest, markers, snippets)
    exporter.py            — SessionExporter (subprocess to merge_logs.py)
    models.py              — SessionStats, SnippetEntry
```

### Import chains

- `cli/dispatch.py` → imports handlers from `cli/run.py`, `cli/wizard.py`, `cli/diagnostics.py`, `cli/update.py`, `cli/sessions/`
- `cli/sessions/__init__.py` → imports from `cli/util.py`
- `cli/diagnostics.py` → imports `_detected_serial_ports` from `cli/wizard.py`
- `cli/run.py` → imports from `backend/app.py` and `backend/config/`
- `backend/app.py` → imports from `backend/sources/`, `backend/parsers/`, `backend/core/`
- `backend/core/runtime.py` → imports from `backend/net/`, `backend/session/`, `backend/sources/`

### Threading model

- **Main thread:** signal handling, session rotation/export orchestration
- **Per-source writer thread:** dequeues LogEntry, writes to .log file, broadcasts to WS and stream clients
- **Per-source reader thread:** reads from serial/UDP, enqueues LogEntry
- **WS broadcaster thread:** runs aiohttp event loop in its own thread
- **Inject/forward server threads:** one per inject/forward port, accept + handle TCP clients
- `_session_info` is protected by `threading.Lock` in LogServer

---

## Core Server

| Feature | Scope | Test(s) | Notes |
|---------|-------|---------|-------|
| UART source reading | backend/sources | `test_runtime_export_and_uart` | Serial port → log lines, retry on error with 3s backoff |
| UDP source reading | backend/sources | `test_parsed_source`, `test_app_parse_source` | Datagram → log lines |
| CBOR datagram parsing | backend/parsers | `test_cbor_datagram_parser`, `test_cbor_integration` | CBOR map → key=value text, trailing bytes rejected |
| Text line parsing | backend/parsers | (via integration tests) | Newline-delimited UTF-8, buffer across chunks |
| Source inject (bidirectional TCP) | backend/net | `test_runtime_export_and_uart` | JSON lines in, log stream out. Uses select() for timeout |
| Source forward (read-only TCP) | backend/net | (manual) | Mirror RX lines to TCP clients, ignore inbound bytes |
| WebSocket UI server | backend/net | Playwright tests | Config-first protocol, replay buffer (5000 entries), batch drain |
| Session logging (per-source .log files) | backend/core | `test_session_components` | Timestamped lines written to disk, flush every 100 lines |
| ANSI color passthrough | backend/core | (manual) | Color codes in log lines forwarded to UI, stripped in file output |
| Queue backpressure (TrackedQueue) | backend/core/queue.py | `test_queue_stats` | Bounded queue with saturation tracking, blocks on put() when full |
| Session clock (relative mode) | backend/core/clock.py | `test_runtime_timestamp_mode` | T+HH:MM:SS.mmm from first log line, origin set on first observe |
| Injectable clock | backend/core/runtime.py | `test_source_manager_clock` | SourceManager accepts `clock` callable for deterministic testing |

---

## Session Management

| Feature | Scope | Test(s) | Notes |
|---------|-------|---------|-------|
| Session creation (timestamped directory) | backend/session | `test_session_components` | `YYYY-MM-DD_HH-MM-SS[_JOBID][_N]` naming, collision-safe |
| Manifest writing | backend/session | `test_session_components` | `manifest.json` per session, updated on export/rotate/first-log |
| Session HTML export | backend/session | `test_session_components`, `test_cli_sessions_export` | Via `merge_logs.py` subprocess, html_status tracking |
| Session rotation | backend/core | Playwright `session-workflows.spec.js` | New session + export old + continue logging, lock-guarded |
| Snippet saving | backend/session | `test_session_components` | Selection → `snippets/*.log` with manifest entry, MAX_SNIPPETS=50 |
| Marker persistence | backend/session | `test_cli_markers` | `markers.json` per session, broadcast to all WS clients on save |
| First-log-at tracking | backend/core | `test_runtime_timestamp_mode` | ISO timestamp of first log line, written to manifest |
| Relative timestamp mode | backend/core | `test_runtime_timestamp_mode` | `T+HH:MM:SS.mmm` from first log, SessionClock origin-based |
| Short alias resolution | backend/cli/util | `test_cli_util` | 4-char SHA256 prefix for session IDs, used in CLI |

---

## CLI Commands

| Feature | Scope | Test(s) | Notes |
|---------|-------|---------|-------|
| `create-config` wizard | backend/cli/wizard.py | `test_cli_create_config` | Interactive YAML config creation, serial port detection |
| `validate` | backend/cli/run.py | `test_config_loader` | Config file validation, --json output |
| `run` | backend/cli/run.py | `test_cli_run_timestamp_mode`, `test_startup_port_conflicts` | CLI flags override config values, 12+ precedence pairs |
| `sessions list` | backend/cli/sessions/list.py | `test_sessions` | Tabular or JSON, --sort, --limit |
| `sessions info` | backend/cli/sessions/info.py | `test_sessions` | Session details with per-source line counts |
| `sessions export` | backend/cli/sessions/export.py | `test_cli_sessions_export` | HTML or raw format, --after/--before/--first/--last time filters |
| `sessions export --missing` | backend/cli/sessions/export.py | `test_cli_sessions_export` | Batch export sessions without HTML |
| `sessions open` | backend/cli/sessions/open.py | (manual) | Open session HTML in browser, #marker-N fragment |
| `sessions delete` | backend/cli/sessions/delete.py | `test_sessions` | By ID, --older-than duration, or --all, with confirmation |
| `sessions marker list/show` | backend/cli/sessions/marker.py | `test_cli_markers` | View session markers with line ranges |
| `sessions snippet list/show/delete` | backend/cli/sessions/snippet.py | `test_cli_snippet` | Manage selection snippets, --index or --all |
| `version` / `doctor` | backend/cli/diagnostics.py | `test_cli_version` | Environment and config diagnostics, --json |
| `ports` | backend/cli/diagnostics.py | (manual) | List detected serial ports, deduplicates tty/cu on macOS |
| `update` | backend/cli/update.py | `test_cli_update` | Self-update from git/release, local or remote installer |
| `merge` | backend/cli/run.py | `test_merge_logs` | Merge raw logs into static HTML via subprocess |
| `parse` | backend/parse.py | (manual) | Parse exported HTML back to raw logs |
| `tail-file` | backend/file_tail_udp.py | `test_tail_file_integration`, `test_file_tail_udp` | Tail file → UDP forwarding, poll-based |

---

## Config

| Feature | Scope | Test(s) | Notes |
|---------|-------|---------|-------|
| YAML config loading | backend/config | `test_config_loader` | Returns AppConfig dataclass with typed fields |
| Config validation | backend/config | `test_config_loader` | ConfigError for all invalid inputs, field-level error messages |
| CLI flag overrides | backend/cli/run.py | `test_cli_run_timestamp_mode` | CLI flags override config values, config overrides defaults |
| Parser config (text, cbor-datagram) | backend/config | `test_config_loader` | Per-source parser type, cbor-datagram only valid for UDP |
| Source label mapping | backend/config | `test_config_loader` | Labels default to source name if not specified |

---

## Networking

| Feature | Scope | Test(s) | Notes |
|---------|-------|---------|-------|
| WS config-first protocol | backend/net | Playwright `layout-sync.spec.js` | Config message sent before adding client to broadcast set |
| WS replay buffer | backend/net | Playwright `demo-smoke.spec.js` | deque(maxlen=5000), replayed to late-joining clients |
| WS broadcast coalescing | backend/net | (manual) | Cross-thread messages batched into single drain task, batch_size=1000 |
| WS send_raw command | backend/net | Playwright `demo-smoke.spec.js` | UI → source TX, serial.SerialException handled |
| WS clear_logs command | backend/net | Playwright `demo-smoke.spec.js` | Scope: all or pane, inserts SYSTEM markers |
| WS export_session_html command | backend/net | Playwright `export-replay.spec.js` | Runs export in asyncio.to_thread |
| WS session rotation | backend/net | Playwright `session-workflows.spec.js` | Runs rotation in asyncio.to_thread |
| WS save_markers command | backend/net | Playwright `demo-smoke.spec.js` | Persists markers, broadcasts update to all clients |
| WS snippet save | backend/net | Playwright `scope-selection.spec.js` | POST body validation, snippet limit check |
| WS no-clients callback | backend/net | (manual) | 1s delayed callback when all WS clients disconnect, triggers export |
| HTTP `/api/session/current` | backend/net | Playwright tests | Returns session_info dict |
| HTTP `/api/sessions` | backend/net | Playwright tests | Reads manifest.json from each session dir |
| HTTP `/api/stats` | backend/net | Playwright tests | Per-source queue stats + totals |
| HTTP `/api/health` | backend/net | Playwright tests | Simple `{"status": "ok"}` |
| HTTP static file serving | backend/net | Playwright tests | UI HTML + JS/CSS from same directory |
| Path traversal protection | backend/net | (manual) | `..` and `/` blocked in session_id/filename |

---

## Data Models

| Model | Location | Key Fields |
|-------|----------|------------|
| `AppConfig` | backend/config/models.py | sources, tabs, server, logs, injects, forwards, baudrate |
| `SourceConfig` | backend/config/models.py | name, type, port, parser, baudrate, label, inject_port, forward_ports |
| `TabConfig` | backend/config/models.py | label, panes |
| `ServerConfig` | backend/config/models.py | host, ws_port, ws_ui, app_name, verbosity, timestamp_mode, ... |
| `LogsConfig` | backend/config/models.py | dir |
| `ParserConfig` | backend/config/models.py | type |
| `LogEntry` | backend/core/models.py | timestamp, source, message, color, no_ws (`@dataclass(slots=True)`) |
| `QueueStats` | backend/core/models.py | maxsize, depth, utilization_pct, enqueued, dequeued, peak_depth, near_full_events |
| `SessionStats` | backend/session/models.py | alias, lines, size_kb, time_start, time_end, duration_secs, markers |
| `SnippetEntry` | backend/session/models.py | file, label, scope, panes, line_count, saved_at |

---

## Known Issues

| Issue | Scope | Notes |
|-------|-------|-------|
| Duplicated session ID generation | backend/app.py + backend/core/runtime.py | Same collision-avoidance algorithm in two places |
| `_run_sessions_export` is ~300 lines | backend/cli/sessions/export.py | Mixes --missing batch mode, raw format, HTML format |
| Playwright scope-selection tests flaky | tests-ui | Timing-sensitive, retry on first attempt, timeout at 300s |

---

## Test Coverage Gaps

| Area | Gap | Priority |
|------|-----|----------|
| `sessions list` | No dedicated test | Medium |
| `sessions info` | No dedicated test | Medium |
| `sessions logs` | No dedicated test | Low |
| `sessions delete` | No dedicated test | Medium |
| `sessions open` | No dedicated test (involves webbrowser.open) | Low |
| `_run_run` end-to-end | Only timestamp override tested | Medium |
| WS broadcast coalescing | No unit test | Medium |
| WS command handling | No unit test (E2E only) | Medium |
