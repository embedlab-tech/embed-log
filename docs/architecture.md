# Architecture

This document describes the current Rust implementation.

## Global record ordering

All source writers share one commit lock. Within that serialized section Embed-log selects the active session, assigns its next global `sequence`, appends `combined.jsonl`, updates replay, matches temporary watches, and publishes the live record. Combined-file, replay, and live log-record order therefore agree even when sources emit concurrently. Titled rotation takes the same lock while swapping paths/session state and resets global sequence to 1 and source-local line counters to 0.

## High-level shape

```text
                         ┌──────────────────────┐
                         │ embed-log.yml config │
                         └──────────┬───────────┘
                                    │ load + validate
                                    ▼
┌──────────────┐        ┌──────────────────────────────┐
│ embed-log CLI│───────▶│ embed-log-core::runtime      │
└──────────────┘        │ LogServer                    │
                        └──────────────┬───────────────┘
                                       │ starts tasks
                                       ▼
          ┌──────────────┬─────────────┬──────────────┐
          │ UART sources │ UDP sources │ file sources │
          └──────┬───────┴──────┬──────┴──────┬───────┘
                 │ LogEntry      │ LogEntry     │ LogEntry
                 ▼               ▼              ▼
          ┌─────────────────────────────────────────────────────────┐
          │ per-source writer tasks                                 │
          │ - append `[timestamp] message` to session log files      │
          │ - update manifest/session metadata                      │
          │ - broadcast JSON messages to WebSocket clients           │
          │ - keep replay buffer for late clients                    │
          └──────────────┬──────────────────────────────┬───────────┘
                         │                              │
                         ▼                              ▼
          ┌──────────────────────────┐      ┌────────────────────────┐
          │ Axum HTTP/WebSocket API  │      │ logs/<session-id>/     │
          │ /, /ws, /api/*           │      │ manifest/logs/html/... │
          └──────────────┬───────────┘      └────────────────────────┘
                         │
                         ▼
          ┌────────────────────────────────────────┐
          │ frontend viewer                        │
          │ live browser UI and terminal UI        │
          │ static exported HTML uses browser assets│
          └────────────────────────────────────────┘
```

## Crates

### `crates/embed-log-core`

Shared library used by the CLI and TUI.

| Module | Responsibility |
| --- | --- |
| `clock` | Timestamp formatting and relative timestamp origin handling. |
| `config` | YAML models, loading, defaulting, and validation. |
| `frontend_assets` | Embeds `frontend/` at compile time with `rust-embed`; runtime can fall back to embedded assets when no filesystem frontend exists. |
| `models` | Core runtime data types like `LogEntry`, `TimestampMode`, ANSI color mapping. |
| `naming` | Slug helpers for filesystem-safe session/log names. |
| `net` | HTTP/WebSocket server and structured control WebSocket API. |
| `parsers` | Stream parsers: text, SLIP/CoAP, and Zephyr dictionary logging. |
| `runtime` | `LogServer`, the main orchestrator. Resolves sources, starts tasks, writes logs, broadcasts messages, rotates/exports sessions. |
| `session` | Session manifest, markers, and static HTML export. |
| `sources` | Source implementations: UART, UDP, and file tail. |

### `crates/embed-log-cli`

Defines the `embed-log` binary.

Main responsibilities:

- parse CLI arguments with `clap`
- resolve config path from `--config`, then `EMBED_LOG_CONFIG_YML_PATH`, then `embed-log.yml`
- run `LogServer`
- launch default browser unless `--no-open-browser` is used
- provide utilities: `doctor`, `ports`, and `sessions`
- launch the integrated terminal UI via `--tui`

### `crates/embed-log-tui`

Terminal WebSocket client used by integrated `--tui` mode and by the standalone `embed-log-tui` binary.

## Runtime data flow

```text
source task
  │ reads bytes/datagrams/files
  ▼
parser
  │ emits text lines
  ▼
LogEntry { timestamp, source, message, color }
  │ mpsc channel per source
  ▼
writer task
  ├─ appends to logs/<session>/<tab>__<source>__<session>.log
  ├─ records first_log_at in manifest
  ├─ updates runtime stats
  ├─ stores message in replay buffer
  └─ broadcasts JSON over tokio broadcast channel
       │
       ├─ WebSocket `/ws` clients receive live messages
       └─ Control WebSocket clients use `/api/v1/control` for subscribe, inject, TX, and markers
```

## Source types

| Config `type` | Implementation | Notes |
| --- | --- | --- |
| `uart` | `sources::uart::UartSource` | Opens a serial port with `serialport`, reads in blocking tasks, parses lines. |
| `udp` | `sources::udp::UdpSource` | Binds UDP on `0.0.0.0:<port>`; text datagrams are treated as newline-terminated. |
| `file` | `sources::file::FileSource` | Creates file if missing, watches parent directory with `notify`, polls/appends from current end. |

`merges` declares presentation-only virtual sources. The runtime persists and broadcasts only the original physical records, stores each merge definition in the session manifest/config message, and lets browser, TUI, static export, control subscriptions, and recorded-session source filters expand the merge into its members. Virtual records never consume global sequence numbers or create source log files. Legacy materialized `source_kind: "merge"` records remain readable only through explicit compatibility flags. See `docs/configuration.md#merges`.

## Parsers

```text
bytes/datagram ──▶ StreamParser::feed(&[u8]) ──▶ Vec<String>
```

| Parser `type` | Scope | Behavior |
| --- | --- | --- |
| `text` | UART, UDP, file | UTF-8-ish line splitting with buffering. |
| `hex-coap` | UART, UDP, file | Replaces textual hexadecimal CoAP packets with a readable decode. |
| `slip-coap` | UART only | Decodes SLIP-framed UDP datagrams carrying CoAP messages. |
| `zephyr-dict` | Any source | Decodes Zephyr dictionary-logging binary messages against `parser.database`. |

Config validation rejects `slip-coap` on non-UART sources and `zephyr-dict` without `parser.database` set.

## HTTP/WebSocket API

The Axum server serves API routes first, then static frontend assets from `frontend_dir` if present, else embedded assets.

| Route | Method | Purpose |
| --- | --- | --- |
| `/` and static paths | `GET` | Viewer UI. |
| `/ws` | WebSocket | Config message, replay buffer, live logs, frontend commands. |
| `/api/health` | `GET` | Health probe. |
| `/api/session/current` | `GET` | Current session info. |
| `/api/session/export` | `POST` | Atomically generate/update canonical `session.html`; `?download=true` returns those same published bytes as an attachment. |
| `/api/session/rotate` | `POST` | Close current session, start a new one, export old session in background. |
| `/api/sessions` | `GET` | List sessions under logs root. |
| `/api/stats` | `GET` | Runtime counters and WebSocket/replay state. |
| `/sessions/{session_id}/{filename}` | `GET` | Serve session artifacts such as logs, `manifest.json`, `session.html`. |

WebSocket commands currently handled by the server:

| Command | Purpose |
| --- | --- |
| `export_session_html` | Export current session HTML. |
| `save_markers` | Persist UI markers to `markers.json`. |
| `clear_logs` | Broadcast a UI clear event. |
| `send_raw` | Add a yellow `TX::UI` entry to a source queue. |

## Session artifacts

A run creates a session directory under `logs.dir`:

```text
logs/
└── 2026-06-14_09-30-00__optional-job-id/
    ├── manifest.json
    ├── combined.jsonl            # structured append-only stream across all sources
    │
    ├── markers.json              # after markers are saved
    ├── .session-html.lock        # advisory lock shared by daemon/offline exporters
    ├── session.html              # atomically published after an export trigger
    └── <tab>__<source>__<session>.log
```

Session HTML is self-contained: log data, CSS, JS, plugin metadata/scripts, markers, and static profile are embedded into one file. New exports read the complete newline-terminated prefix of canonical `combined.jsonl`, render behind the per-session lock, and rename a flushed temporary file into place. A failed export therefore leaves the previous complete report intact.

## Frontend architecture

The viewer is plain ES modules in `frontend/`. The same UI code supports:

- live browser mode served by Axum
- terminal UI mode
- static exported HTML mode, where module imports/exports are stripped and data is bootstrapped inline

Important files:

| File | Responsibility |
| --- | --- |
| `main.js` | Live-mode entry point and import ordering. |
| `ws.js` | WebSocket connection, config message handling, live events. |
| `state.js` | Shared tab/pane/viewer state and timestamp context. |
| `lines.js` | Render/append/re-render lines, timestamp mode updates, optional custom-plugin analysis. |
| `tabcreate.js`, `tabs.js` | Tab/pane construction and switching. |
| `renderPane.js`, `renderToolbar.js` | Shared shell renderers for live/static UI. |
| `selection.js` | Line selection, markers, copy/export selected text. |
| `export.js` | Canonical daemon export download plus client-side selection-only HTML snapshots. |
| `persist.js` | Browser session persistence. |
| `settings.js`, `themes.js`, `fontsize.js` | User settings, themes, font size. |
| `pluginRuntime.js` | Optional custom plugin registry/loading/settings; no built-in protocol decoders. |
| `tsparse.js` | Timestamp parsing for imports/static logs. |
| `import.js` | Import `.log` files into panes. |

## Optional custom plugin path

Custom config-v1 plugins may still be loaded from explicit paths and included in WebSocket config/session exports. Built-in protocol plugins were removed: textual CoAP belongs on the source as `parser.type: hex-coap`, so CLI, browser, TUI, watches, and persistence all receive the same decoded record.

## Release architecture

The CLI release workflow builds precompiled binaries on native/self-hosted runners and publishes one GitHub Release:

```text
Linux runner   ─▶ embed-log-x86_64-unknown-linux-gnu.tar.gz
Mac runner     ─▶ embed-log-aarch64-apple-darwin.tar.gz
Mac runner     ─▶ embed-log-x86_64-apple-darwin.tar.gz
Windows runner ─▶ embed-log-x86_64-pc-windows-msvc.zip
publish job    ─▶ install.sh, install.ps1, SHA256SUMS, GitHub Release
```

See [releasing.md](releasing.md).
