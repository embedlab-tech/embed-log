# Implemented Features

Living document of all features in `embed-log`. Agents: update this file when adding or changing observable behavior.

---

## How to Use This File

- **Before starting work:** Read relevant sections to understand what exists.
- **After implementing a feature:** Add or update the entry. Include the test that covers it.
- **Format:** `| Feature | Scope | Test(s) | Notes |`

---

## Core Server

| Feature | Scope | Test(s) | Notes |
|---------|-------|---------|-------|
| UART source reading | backend/sources | `test_runtime_export_and_uart` | Serial port → log lines, retry on error |
| UDP source reading | backend/sources | `test_parsed_source`, `test_app_parse_source` | Datagram → log lines |
| CBOR datagram parsing | backend/parsers | `test_cbor_datagram_parser`, `test_cbor_integration` | CBOR map → key=value text |
| Text line parsing | backend/parsers | (via integration tests) | Newline-delimited UTF-8 |
| Source inject (bidirectional TCP) | backend/net | `test_runtime_export_and_uart` | JSON lines in, log stream out |
| Source forward (read-only TCP) | backend/net | (manual) | Mirror RX lines to TCP clients |
| WebSocket UI server | backend/net | Playwright tests | Config-first protocol, replay buffer |
| Session logging (per-source .log files) | backend/core | `test_session_components` | Timestamped lines written to disk |
| ANSI color passthrough | backend/core | (manual) | Color codes in log lines forwarded to UI |
| Queue backpressure (TrackedQueue) | backend/core | `test_queue_stats` | Bounded queue with saturation tracking |

## Session Management

| Feature | Scope | Test(s) | Notes |
|---------|-------|---------|-------|
| Session creation (timestamped directory) | backend/session | `test_session_components` | `YYYY-MM-DD_HH-MM-SS[_JOBID][_N]` naming |
| Manifest writing | backend/session | `test_session_components` | `manifest.json` per session |
| Session HTML export | backend/session | `test_session_components`, `test_cli_sessions_export` | Via `merge_logs.py` subprocess |
| Session rotation | backend/core | Playwright `session-workflows.spec.js` | New session, export old, continue logging |
| Snippet saving | backend/session | `test_session_components` | Selection → `snippets/*.log` with manifest entry |
| Marker persistence | backend/session | `test_cli_markers` | `markers.json` per session |
| First-log-at tracking | backend/core | `test_runtime_timestamp_mode` | ISO timestamp of first log line |
| Relative timestamp mode | backend/core | `test_runtime_timestamp_mode` | `T+HH:MM:SS.mmm` from first log |

## CLI Commands

| Feature | Scope | Test(s) | Notes |
|---------|-------|---------|-------|
| `create-config` wizard | backend/cli | `test_cli_create_config` | Interactive YAML config creation |
| `validate` | backend/cli | `test_config_loader` | Config file validation |
| `run` | backend/cli | `test_cli_run_timestamp_mode`, `test_startup_port_conflicts` | Start server from config or flags |
| `sessions list` | backend/cli | `test_sessions` | Tabular or JSON session listing |
| `sessions info` | backend/cli | `test_sessions` | Session details with source stats |
| `sessions export` | backend/cli | `test_cli_sessions_export` | HTML or raw format, time filtering |
| `sessions export --missing` | backend/cli | `test_cli_sessions_export` | Batch export sessions without HTML |
| `sessions open` | backend/cli | (manual) | Open session HTML in browser |
| `sessions delete` | backend/cli | `test_sessions` | By ID, age (`--older-than`), or `--all` |
| `sessions marker list/show` | backend/cli | `test_cli_markers` | View session markers |
| `sessions snippet list/show/delete` | backend/cli | `test_cli_snippet` | Manage selection snippets |
| `version` / `doctor` | backend/cli | `test_cli_version` | Environment and config diagnostics |
| `ports` | backend/cli | (manual) | List detected serial ports |
| `update` | backend/cli | `test_cli_update` | Self-update from git/release |
| `merge` | backend/cli | `test_merge_logs` | Merge raw logs into static HTML |
| `parse` | backend/cli | (manual) | Parse exported HTML back to raw logs |
| `tail-file` | backend/cli | `test_tail_file_integration`, `test_file_tail_udp` | Tail file → UDP forwarding |

## Config

| Feature | Scope | Test(s) | Notes |
|---------|-------|---------|-------|
| YAML config loading | backend/config | `test_config_loader` | Sources, tabs, server settings |
| Config validation | backend/config | `test_config_loader` | Type checks, required fields, duplicates |
| CLI flag overrides | backend/cli | `test_cli_run_timestamp_mode` | CLI flags override config values |
| Parser config (text, cbor-datagram) | backend/config | `test_config_loader` | Per-source parser type selection |

## Networking

| Feature | Scope | Test(s) | Notes |
|---------|-------|---------|-------|
| WS config-first protocol | backend/net | Playwright `layout-sync.spec.js` | Config message before log events |
| WS replay buffer | backend/net | Playwright `demo-smoke.spec.js` | Late-joining clients get history |
| WS send_raw command | backend/net | Playwright `demo-smoke.spec.js` | UI → source TX |
| WS clear_logs command | backend/net | Playwright `demo-smoke.spec.js` | UI clear markers |
| WS export_session_html command | backend/net | Playwright `export-replay.spec.js` | Trigger export from UI |
| WS session rotation | backend/net | Playwright `session-workflows.spec.js` | Trigger rotation from UI |
| WS save_markers command | backend/net | Playwright `demo-smoke.spec.js` | Persist markers from UI |
| WS snippet save | backend/net | Playwright `scope-selection.spec.js` | Save selection as snippet |
| HTTP `/api/session/current` | backend/net | Playwright tests | Current session metadata |
| HTTP `/api/sessions` | backend/net | Playwright tests | All sessions listing |
| HTTP `/api/stats` | backend/net | Playwright tests | Queue stats per source |
| HTTP `/api/health` | backend/net | Playwright tests | Health check |
| HTTP static file serving | backend/net | Playwright tests | UI HTML, JS, CSS |
| Path traversal protection | backend/net | (manual) | `..` and `/` blocked in session_id/filename |

---

## Data Models (Post-Refactor)

_Agents: fill this section as Phase 1 tasks are completed._

| Model | Location | Fields | Replaces |
|-------|----------|--------|----------|
| — | — | — | — |

---

## Changelog

| Date | Change | Author | Tests |
|------|--------|--------|-------|
| 2026-05-30 | Phase 2 complete: `backend/cli.py` (2591 lines) → `backend/cli/` package (10 modules, 2715 lines total) | refactor | 222 tests pass |

_Update this section when adding features or making behavioral changes._
