# AGENTS.md

Quick onboarding notes for future coding agents and humans working in `embed-log`.

## Project intent

`embed-log` aggregates embedded-device logs from multiple sources, stores them to per-source log files, streams them live to a web UI, and accepts test-driven marker/TX injections.

## Architecture at a glance

1. **Source readers** (`uart`, `udp`) feed lines into per-source queues.
2. **SourceManager writer thread** serializes output order and writes to:
   - stdout
   - `logs/<SOURCE>.log`
   - WebSocket payloads (if UI enabled)
   - inject-port stream clients
3. **WebSocketBroadcaster** serves UI and pushes `rx`/`tx` events.
4. **Frontend** renders tabs/panes, filters, settings, export/import, selection, splitters, and cached refresh restore.

## Run locally (recommended)

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
./run_demo.sh
```

UI: `http://127.0.0.1:8080/`

## Key code locations

- Backend core: `backend/server.py`
- Client APIs: `backend/log_client.py`, `backend/tx_client.py`
- Frontend entry: `frontend/main.js`
- Frontend state/layout: `frontend/state.js`, `frontend/tabs.js`, `frontend/tabcreate.js`, `frontend/ui.js`
- Live transport: `frontend/ws.js`
- Rendering: `frontend/lines.js`
- Frontend cache persistence: `frontend/persist.js`

## Current UX features (important context)

- Reliable macOS-friendly splitter dragging.
- Pane swap UI via hover popup on tab labels.
- Visual swap pulse animation.
- Local refresh cache of logs/layout (with toolbar "Clear cache" action).

## Change guidelines

- Keep frontend as plain modules (no build tool assumptions).
- Prefer minimal, targeted edits over rewrites.
- Preserve protocol compatibility:
  - WS sends `config` first, then log events.
  - Inject sockets accept newline-delimited JSON.
- Any new backend capability should be easy to drive from both:
  - direct Python clients (`log_client`, `tx_client`)
  - browser WebSocket path.

## Suggested next backend priorities

1. Bounded queues + drop/backpressure counters.
2. `/health` + `/stats` endpoint(s) for CI observability.
3. Optional server-side retention/replay window.
4. Optional auth for non-local deployments.

## Documentation map

- `README.md` — main docs
- `DIRECTORY_GUIDE.md` — directory explanations
- `FRONTEND.md` — frontend details
- `INSTALL.md` — setup
- `MERGE.md` — offline merge utility
