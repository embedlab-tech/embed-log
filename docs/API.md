# API

This document describes the current browser-facing API and WebSocket contract implemented in code.

## HTTP endpoints

### `GET /`
Serves the browser UI entry page.

### `GET /<static-file>`
Serves frontend static assets from the frontend directory.

### `GET /api/session/current`
Returns current session metadata.

Current shape includes:

```json
{
  "id": "2026-05-23_18-23-15",
  "job_id": null,
  "app_name": "embed-log",
  "system_timezone": "CEST",
  "dir": "logs/2026-05-23_18-23-15",
  "manifest": "/sessions/2026-05-23_18-23-15/manifest.json",
  "html": "/sessions/2026-05-23_18-23-15/session.html",
  "html_ready": true,
  "html_status": "ready",
  "html_updated_at": "2026-05-23T18:23:16+02:00",
  "html_error": null,
  "api": "/api/session/current",
  "tabs": [{ "label": "Devices", "panes": ["FTDI_A", "FTDI_B"] }],
  "sources": [{ "name": "FTDI_A", "log": "/sessions/.../FTDI_A.log" }]
}
```

### `POST /api/session/export`
Triggers backend `session.html` generation.

Responses:
- `200` with `{ "ok": true, "session": ... }` on success
- `409` with `{ "ok": false, "session": ... }` if export is refused/fails in current flow
- `503` if export is unavailable

### `POST /api/session/rotate`
Triggers clean session rotation.

Success response:

```json
{
  "ok": true,
  "old_session": { "id": "..." },
  "session": { "id": "new-session-id" }
```

### `POST /api/session/snippet`
Saves a selection snippet from the UI.

Success response:

```json
{
  "ok": true,
  "filename": "embed-log-snippet-2026-05-31T13-00-00.html"
}
```

### `GET /api/sessions`
Returns saved sessions and the current session id.

Shape:

```json
{
  "current": "2026-05-23_18-23-15",
  "sessions": [
    {
      "id": "2026-05-23_18-23-15",
      "started_at": "2026-05-23T18:23:15+02:00",
      "html_ready": true,
      "html_status": "ready",
      "html_updated_at": "2026-05-23T18:23:16+02:00",
      "html_error": null,
      "html": "/sessions/2026-05-23_18-23-15/session.html",
      "manifest": "/sessions/2026-05-23_18-23-15/manifest.json",
      "tabs": [{ "label": "Devices", "panes": ["FTDI_A", "FTDI_B"] }]
    }
  ]
}
```

### `GET /sessions/<session_id>/<filename>`
Serves a file from a saved session directory.

Used for:
- `manifest.json`
- `session.html`
- per-source `.log` files

## WebSocket endpoint

### `GET /ws`
Browser connects here for live UI updates.

## WebSocket server → client messages

### 1. `config`
Always sent first.

```json
{
  "type": "config",
  "tabs": [{ "label": "Devices", "panes": ["FTDI_A", "FTDI_B"] }],
  "pane_labels": { "FTDI_A": "DUT", "FTDI_B": "AUX" },
  "session": { "id": "...", "html_status": "pending" },
  "app_name": "embed-log",
  "theme_defaults": {}
}
```

Semantics:
- `tabs` is authoritative for UI layout.
- `pane_labels` is optional display metadata; pane ids remain the stable technical keys used by commands and DOM ids.
- `session` seeds session/export UI state.
- frontend must not assume panes before this message arrives.

### 2. `rx` / `tx`
Live log event.

```json
{
  "type": "rx",
  "data": "TEST src=FTDI_A tick=001 ...",
  "timestamp": "05-23 18:23:16.123",
  "timestamp_iso": "2026-05-23T18:23:16.123+02:00",
  "source_id": "FTDI_A"
}
```

Fields:
- `type`: `rx` or `tx`
- `data`: rendered log payload (may already include ANSI color codes)
- `timestamp`: short display timestamp
- `timestamp_iso`: ISO timestamp with milliseconds
- `source_id`: pane/source identifier

### 3. `session_html_status`
Published when backend session HTML state changes.

```json
{
  "type": "session_html_status",
  "session_id": "2026-05-23_18-23-15",
  "html_ready": true,
  "html_status": "ready",
  "html_updated_at": "2026-05-23T18:23:16+02:00",
  "html_error": null,
  "last_export_reason": "manual_ui"
}
```

Common statuses:
- `pending`
- `updating`
- `ready`
- `error`

### 4. `session_rotated`
Published after clean-session rotation.

```json
{
  "type": "session_rotated",
  "old_session": { "id": "old-id" },
  "session": { "id": "new-id", "html_status": "pending" }
}
```

Frontend uses this to clear stale state and bind to the new session.

### 5. `markers_update`
Published when markers are saved via the UI.

```json
{
  "type": "markers_update",
  "markers": [
    {
      "pane": "FTDI_A",
      "lines": [1, 5],
      "description": "Boot complete",
      "timestamp": "...",
      "created_at": "..."
    }
  ]
}
```

Frontend uses this to update the marker list without re-fetching.

## WebSocket client → server commands

### `send_raw`
Send raw TX data to a source.

```json
{ "cmd": "send_raw", "id": "FTDI_A", "data": "reboot\n" }
```

### `export_session_html`
Trigger backend session HTML export.

```json
{ "cmd": "export_session_html" }
```

### `clear_logs`
Clear pane or all panes through marker flow.

Per-pane:

```json
{ "cmd": "clear_logs", "scope": "pane", "id": "FTDI_A" }
```

All panes:

```json
{ "cmd": "clear_logs", "scope": "all" }
```

### `save_markers`
Save markers from the UI.

```json
{
  "cmd": "save_markers",
  "markers": [
    {
      "pane": "FTDI_A",
      "lines": [1, 5],
      "description": "Boot complete",
      "timestamp": "2026-05-23T18:23:16.123+02:00",
      "created_at": "2026-05-23T18:23:16.456+02:00"
    }
  ]
}
```

## Compatibility notes

- `config` first is a hard requirement.
- `tabs` and `session` shapes are effectively frontend contracts.
- Export/session flows are used by both live UI and Playwright tests.
- Changes here should be coordinated with `docs/BACKEND.md`, `docs/FRONTEND.md`, and E2E coverage.
