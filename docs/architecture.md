# Architecture

This document describes the current Rust/Tauri implementation.

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
┌──────────────┐        └──────────────┬───────────────┘
│ Tauri shell  │───────▶               │
└──────────────┘                       │ starts tasks
                                       ▼
          ┌──────────────┬─────────────┬──────────────┬──────────────┐
          │ UART sources │ UDP sources │ file sources │ mock network │
          └──────┬───────┴──────┬──────┴──────┬───────┴──────┬───────┘
                 │ LogEntry      │ LogEntry     │ LogEntry      │ LogEntry
                 ▼               ▼              ▼              ▼
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
          │ live browser UI or Tauri webview       │
          │ static exported HTML uses same assets  │
          └────────────────────────────────────────┘
```

## Crates

### `crates/embed-log-core`

Shared library used by both the CLI and Tauri app.

| Module | Responsibility |
| --- | --- |
| `clock` | Timestamp formatting and relative timestamp origin handling. |
| `config` | YAML models, loading, defaulting, and validation. |
| `demo` | Demo config support and generated demo traffic. |
| `frontend_assets` | Embeds `frontend/` at compile time with `rust-embed`; runtime can fall back to embedded assets when no filesystem frontend exists. |
| `models` | Core runtime data types like `LogEntry`, `TimestampMode`, ANSI color mapping. |
| `naming` | Slug helpers for filesystem-safe session/log names. |
| `net` | HTTP/WebSocket server plus TCP inject/forward servers. |
| `onboarding` | First-run quick-config builder, serial-port listing, and the shared onboarding HTTP server used by **both** the CLI and the Tauri app. |
| `parsers` | Stream parsers: text and UDP CBOR datagram parser. |
| `runtime` | `LogServer`, the main orchestrator. Resolves sources, starts tasks, writes logs, broadcasts messages, rotates/exports sessions. |
| `session` | Session manifest, markers, and static HTML export. |
| `sources` | Source implementations: UART, UDP, file tail, and network capture (mock or pcap-backed UDP tap). |

### `crates/embed-log-cli`

Defines the `embed-log` binary.

Main responsibilities:

- parse CLI arguments with `clap`
- resolve config path from `--config`, then `EMBED_LOG_CONFIG_YML_PATH`, then `embed-log.yml`
- run `LogServer`
- run first-run **onboarding** (via the shared core `OnboardingServer`) when no config exists, or on the `onboard` subcommand
- launch default browser unless `--no-open-browser` is used
- provide utilities: `init`, `doctor`, `ports`, `sessions`, `merge`, `parse`, `demo`
- launch the Tauri binary via `--ui` when available

### `crates/embed-log-tauri`

Desktop shell around the same core server.

Main responsibilities:

- resolve config path from `--config`, `EMBED_LOG_CONFIG_YML_PATH`, local `embed-log.yml`, or app config directory
- store the resolved config path in process state before onboarding/server startup
- reuse the shared core `OnboardingServer` + `save_quick_config`; the Tauri save handler also starts the `LogServer`
- resolve relative `logs.dir` against the config file directory
- start `LogServer` in Tauri async runtime
- navigate the webview to the local server URL
- provide onboarding when no config exists
- expose thin Tauri commands (serial ports, server status, quick config) for the webview eval fallback
- export current session on close when the server is running

See [tauri.md](tauri.md) for exact config/log path behavior.

## Runtime data flow

```text
source task
  │ reads bytes/datagrams/files/mock events
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
       ├─ TCP forward clients receive raw lines for one source
       └─ TCP inject clients receive raw lines and can submit JSON/tx commands
```

## Source types

| Config `type` | Implementation | Notes |
| --- | --- | --- |
| `uart` | `sources::uart::UartSource` | Opens a serial port with `serialport`, reads in blocking tasks, parses lines. |
| `udp` | `sources::udp::UdpSource` | Binds UDP on `0.0.0.0:<port>`. Text parser treats each datagram as newline-terminated; CBOR parser decodes one datagram. |
| `file` | `sources::file::FileSource` | Creates file if missing, watches parent directory with `notify`, polls/appends from current end. |
| `network_capture` | `sources::network::NetworkCaptureSource` | Supports `network_backend: mock` plus `network_backend: pcap` for simplified UDP packet capture with kernel BPF filters. |

## Parsers

```text
bytes/datagram ──▶ StreamParser::feed(&[u8]) ──▶ Vec<String>
```

| Parser `type` | Scope | Behavior |
| --- | --- | --- |
| `text` | UART, UDP, file | UTF-8-ish line splitting with buffering. |
| `cbor-datagram` | UDP only | Decodes a CBOR datagram and formats key/value output. |

Config validation rejects `cbor-datagram` on non-UDP sources.

## HTTP/WebSocket API

The Axum server serves API routes first, then static frontend assets from `frontend_dir` if present, else embedded assets.

| Route | Method | Purpose |
| --- | --- | --- |
| `/` and static paths | `GET` | Viewer UI. |
| `/ws` | WebSocket | Config message, replay buffer, live logs, frontend commands. |
| `/api/health` | `GET` | Health probe. |
| `/api/session/current` | `GET` | Current session info. |
| `/api/session/export` | `POST` | Generate/update `session.html`. |
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
| `set_filter` | Validate frontend regex filter. |
| `send_raw` | Add a yellow `TX::UI` entry to a source queue. |

## Session artifacts

A run creates a session directory under `logs.dir`:

```text
logs/
└── 2026-06-14_09-30-00__optional-job-id/
    ├── manifest.json
    ├── combined.jsonl            # structured append-only stream across all sources
    │                              # includes packet fields for network_capture
    ├── markers.json              # after markers are saved
    ├── session.html              # after export/shutdown/no-client export
    └── <tab>__<source>__<session>.log
```

Session HTML is self-contained: log data, CSS, JS, plugin metadata/scripts, markers, and static profile are embedded into one file.

## Frontend architecture

The viewer is plain ES modules in `frontend/`. The same UI code supports:

- live browser mode served by Axum
- Tauri webview mode
- static exported HTML mode, where module imports/exports are stripped and data is bootstrapped inline

Important files:

| File | Responsibility |
| --- | --- |
| `main.js` | Live-mode entry point and import ordering. |
| `ws.js` | WebSocket connection, config message handling, live events. |
| `state.js` | Shared tab/pane/viewer state and timestamp context. |
| `lines.js` | Render/append/re-render lines, timestamp mode updates, plugin analysis. |
| `tabcreate.js`, `tabs.js` | Tab/pane construction and switching. |
| `renderPane.js`, `renderToolbar.js` | Shared shell renderers for live/static UI. |
| `selection.js` | Line selection, markers, copy/export selected text. |
| `export.js` | Client-side HTML snapshot/export support. |
| `persist.js` | Browser session persistence. |
| `settings.js`, `themes.js`, `fontsize.js` | User settings, themes, font size. |
| `pluginRuntime.js` | Plugin registry/loading/settings. |
| `plugin-hex-coap.js` | Built-in CoAP hex plugin. |
| `tsparse.js` | Timestamp parsing for imports/static logs. |
| `import.js` | Import `.log` files into panes. |
| `onboarding.js` | First-run config UI — shared by the browser (CLI) and Tauri desktop apps. |

## Plugin path

```text
embed-log.yml
  frontend_plugins:
    hex-coap:
      builtin: hex-coap
  tabs:
    - panes:
        - source: COAP_RAW
          plugins: [hex-coap]

LogServer::load_plugins()
  ├─ reads frontend/plugin-hex-coap.js or custom plugin path
  ├─ builds plugin metadata/scripts
  └─ includes them in WS config + session export

frontend/pluginRuntime.js
  ├─ evaluates/registers plugins
  └─ lets lines.js annotate/render plugin-derived UI
```

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
