# BACKEND

## What the backend does

The backend is a configurable log aggregation runtime for embedded and CI workflows.

Main responsibilities:
- read from UART and UDP sources,
- timestamp and serialize events,
- write per-session raw logs,
- broadcast live events to the browser UI over WebSocket,
- expose session APIs and session artifacts,
- support inject/TX and optional raw forwarding.

## Main features

- multi-source ingestion (`uart`, `udp`)
- deterministic session directories under `logs/<session_id>/`
- session artifacts:
  - `manifest.json`
  - `session.html`
  - per-source `.log` files
- live WebSocket UI transport
- UI-driven session export and clean-session rotation
- inject-port JSON input and optional forward-port raw output
- config-driven tab/pane layout shared with frontend

## Key files

- `backend/app.py` — composes sources, runtime, tabs, logs
- `backend/cli.py` — `init`, `validate`, `run`, other CLI entry flow
- `backend/core/runtime.py` — runtime coordination, sessions, export, rotation
- `backend/net/ws_server.py` — HTTP + WS endpoints and UI session APIs
- `backend/session/manager.py` — session metadata and manifest state
- `backend/session/exporter.py` — backend `session.html` export orchestration
- `backend/config/loader.py` — YAML parsing/validation
- `backend/sources/uart.py`, `backend/sources/udp.py` — source adapters

## Runtime flow

1. Sources read lines.
2. Runtime normalizes/timestamps them.
3. `SourceManager` writes them to session logs and live sinks.
4. WebSocket broadcaster emits:
   - `config` first,
   - then log events,
   - plus session/export status updates.
5. Session APIs expose current session and saved sessions.
6. Session HTML is generated on demand and on lifecycle events.

## Contracts the frontend depends on

- `config` must be sent before log events.
- `config.tabs` is authoritative for visible tabs/panes.
- `config.session` contains current session metadata.
- `GET /api/session/current`
- `GET /api/sessions`
- `GET /sessions/<session_id>/<filename>`

Breaking any of the above requires coordinated frontend and test changes.

## Things to preserve during backend changes

- session rotation must not leave stale in-memory/UI state,
- `session.html` export must remain reopenable as static replay,
- inject sockets accept newline-delimited JSON,
- log ordering and timestamp semantics must stay stable enough for deterministic UI tests,
- config-driven layout must remain compatible with both live UI and exported HTML.

## Common change zones

- session/export flow → `backend/core/runtime.py`, `backend/session/*`, `backend/net/ws_server.py`
- new source behavior → `backend/sources/*`
- config schema/layout → `backend/config/loader.py`, `backend/cli.py`, frontend tab consumers
- API/session metadata → backend + frontend together
