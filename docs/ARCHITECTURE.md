# ARCHITECTURE

This is the shortest accurate end-to-end description of how `embed-log` works today.

## System overview

`embed-log` is a backend-first system.

The backend owns:
- source ingestion,
- log timestamping and persistence,
- session lifecycle,
- UI layout definition,
- HTTP/WebSocket APIs,
- backend-generated session exports.

The frontend is a thin browser client that renders what the backend describes.

## End-to-end flow

```text
UART / UDP sources
        |
        v
backend/sources/*
        |
        v
backend/core/runtime.py
  - normalize/timestamp
  - write per-session logs
  - broadcast live events
  - manage export/rotation
        |
        +------------------------+
        |                        |
        v                        v
logs/<session_id>/*         backend/net/ws_server.py
  - source logs              - GET /
  - manifest.json            - GET /api/session/current
  - session.html             - GET /api/sessions
                             - GET /sessions/<id>/<file>
                             - WS /ws
                                      |
                                      v
                                frontend/*
                          - tabs/panes from config
                          - live logs over WS
                          - export/import/selection/cache
```

## Main backend components

### 1. Sources

- `backend/sources/uart.py`
- `backend/sources/udp.py`

They read incoming data and feed runtime-managed flow.

### 2. Runtime

- `backend/core/runtime.py`

This is the operational center.

It coordinates:
- `SourceManager` instances,
- session creation and rotation,
- manifest updates,
- backend `session.html` export,
- live WebSocket broadcasting.

### 3. Session artifacts

- `backend/session/manager.py`
- `backend/session/exporter.py`

Each session directory contains:
- raw source log files,
- `manifest.json`,
- `session.html`.

### 4. HTTP + WebSocket server

- `backend/net/ws_server.py`

Serves:
- browser assets,
- session APIs,
- session artifact files,
- `/ws` live stream.

### 5. Frontend

- `frontend/*`

Plain JS modules, no build step.

The frontend:
- builds tabs/panes from backend `config`,
- renders live logs,
- manages filters, selection, export/import, cache, sessions UI.

## Session lifecycle

### Session start

On startup the backend creates a session directory and session metadata.

### Live operation

Incoming lines are:
1. timestamped,
2. written to session logs,
3. sent to WebSocket clients,
4. available for later export.

### Save HTML

The backend can generate `session.html` on demand.

Triggers include:
- explicit UI save,
- last WebSocket client disconnect,
- shutdown/signal,
- session rotation.

### Clean session rotation

When rotating a session, the backend:
1. marks the old session as closing,
2. flushes/writes logs,
3. exports old session HTML,
4. creates a new session directory,
5. rotates source log files,
6. publishes `session_rotated` to clients,
7. resumes live logging into the new session.

## Layout model

Layout is config-driven.

- Backend config defines `tabs`.
- Each tab has 1 or 2 panes.
- Frontend renders that layout as-is.
- Exported HTML reuses the same logical tab/pane model.

Current deterministic demo layout:
- `Simulated Devices` → `SENSOR_A`, `SENSOR_B`
- `Other Sensor` → `SENSOR_C`

## Important invariants

1. WebSocket must send `config` before live log events.
2. Session APIs and session metadata must stay aligned with UI expectations.
3. Exported `session.html` must work as a static replay.
4. Session rotation must not leak stale lines into the new session UI.
5. Frontend layout is derived from backend config, not hardcoded in the browser.

## Most common cross-cutting changes

- Session/export work touches:
  - `backend/core/runtime.py`
  - `backend/session/*`
  - `backend/net/ws_server.py`
  - `frontend/ui.js`
- Layout work touches:
  - config loader,
  - websocket config payload,
  - `frontend/state.js`, `tabcreate.js`, `tabs.js`
- UI export/replay work touches:
  - `frontend/export.js`
  - `backend/session/exporter.py`
  - `utils/merge_logs.py`
