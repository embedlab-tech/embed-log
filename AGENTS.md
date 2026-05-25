# AGENTS.md

Quick onboarding notes for humans and coding agents working in `embed-log`.

## Project intent

`embed-log` is a configurable log aggregation server for embedded development and CI. It ingests multiple sources, timestamps and stores session artifacts, serves a backend-configured browser UI, and supports inject/TX workflows.

## Read these first

- `README.md` — install/run/config basics
- `docs/BACKEND.md` — backend architecture and contracts
- `docs/FRONTEND.md` — frontend architecture and UI behavior
- `docs/TESTING.md` — backend + Playwright test strategy
- `docs/README.md` — documentation index

## Architecture at a glance

1. Source readers (`uart`, `udp`) feed lines into runtime-managed flow.
2. Runtime writes to per-session logs and live sinks.
3. WebSocket broadcaster sends `config` first, then log/session events.
4. Session artifacts are generated per session:
   - `manifest.json`
   - `session.html`
5. Frontend renders tabs/panes, filtering, selection, export/import, sessions, cache.

## Key code locations

- Backend runtime: `backend/core/runtime.py`
- App composition: `backend/app.py`
- HTTP/WS server: `backend/net/ws_server.py`
- Session management/export: `backend/session/*`
- Frontend state/layout: `frontend/state.js`, `frontend/tabs.js`, `frontend/tabcreate.js`, `frontend/ui.js`
- Frontend transport/rendering: `frontend/ws.js`, `frontend/lines.js`, `frontend/selection.js`, `frontend/export.js`
- Playwright tests: `tests-ui/tests/*`

## Current demo/test layout

Deterministic demo config uses:
- `DevA` tab → `SENSOR_A` (`READER`), `SENSOR_B` (`CONTROLLER`)
- `DevB` tab → `SENSOR_C` (`READER`)

Do not assume all panes are visible at once.

## Contracts to preserve

- WS protocol order: `config` first, then log events.
- Session APIs:
  - `GET /api/session/current`
  - `GET /api/sessions`
  - `GET /sessions/<session_id>/<filename>`
- Inject sockets accept newline-delimited JSON.
- Exported `session.html` must remain usable as static replay.

## Working guidelines

- Keep frontend plain modules; no bundler assumptions.
- Prefer targeted edits over rewrites.
- Reuse existing protocol and state patterns.
- When changing exported/static HTML behavior, validate both live and replay paths.
- Prefer deterministic `tick=...` / `kind=...` assertions in UI tests.
- Use `confirm()` before destructive session operations in tests.
- When managing backend server processes for testing:
  - **Never use `pkill -f "embed-log"`, `pkill -f "run_demo"`, or `pkill -f "deterministic"`**. The `-f` flag matches the full command line and can kill the tmux/terminal session itself.
  - Use `lsof -ti:8080 | xargs kill 2>/dev/null` to kill only what's bound to the server port.
  - Or track the PID from a controlled `./run_demo.sh & echo $!` start and kill by PID.
  - After killing, confirm the port is actually free: `sleep 1; lsof -ti:8080` should return nothing before restarting the server.

## Useful commands

```bash
# backend tests
python3 -m unittest discover -s tests -v

# ui tests
cd tests-ui && npm test
```
