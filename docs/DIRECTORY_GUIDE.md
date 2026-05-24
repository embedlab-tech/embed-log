# Directory Guide

Short, practical map of this repository for humans and coding agents.

`embed-log` is a backend-first, configurable log aggregation system (UART/UDP + inject/TX TCP + session artifacts) with a dynamic browser UI driven by backend tabs/panes config.

## Root directories

- `backend/`
  - Core log server and Python client APIs.
  - Key files:
    - `server.py` — compatibility entrypoint (`backend.server:main`) + runtime re-exports.
    - `cli.py` — command parsing (`init`, `validate`, `run`).
    - `app.py` — app composition/startup (sources, tabs, session dir/log wiring).
    - `core/runtime.py` — runtime engine (`LogServer`, `SourceManager`, line formatting/router loop).
    - `net/ws_server.py` — WebSocket/HTTP UI server implementation.
    - `net/inject_server.py` / `net/forward_server.py` — TCP inject/forward socket servers.
    - `sources/base.py`, `sources/uart.py`, `sources/udp.py` — source adapter interfaces + implementations.
    - `session/manager.py`, `session/exporter.py` — session metadata/manifest + HTML export orchestration.
    - `config/loader.py` — YAML config parsing/validation.
    - `log_client.py` — marker injection + stream subscription client.
    - `tx_client.py` — TX-only client.

- `frontend/`
  - Browser UI (vanilla HTML/CSS/JS, no build step).
  - Handles tabs/panes, live websocket updates, filtering, selection, export, import, pane swapping, splitter drag, and refresh persistence cache.

- `utils/`
  - Helper scripts for demos and offline workflows.
  - Includes UDP simulator, inject demo sender, and log merge utility.

- `logs/`
  - Runtime output generated per session (e.g. `logs/<session_id>/`).
  - Session directory contains raw logs plus `manifest.json` and `session.html`.

- `tests/`
  - Unit tests for config parsing, source parsing, and session components.

- `.venv/`
  - Local virtual environment (developer-local).

- `.git/`
  - Git metadata.

- `~/`
  - Local scratch/session directory present in this workspace (not core app logic).

## Important root files

> Note: this file mirrors the high-level tree in `README.md`.

- `README.md` — main project documentation and backend overview.
- `AGENTS.md` — quick instructions for future contributors/agents.
- `INSTALL.md` — setup and run prerequisites.
- `FRONTEND.md` — frontend internals.
- `MERGE_LOGS.md` — merged-log report behavior
- `SAMPLE_COMMANDS.md` — copy/paste examples.
- `run_demo.sh` — one-command local demo launcher.
- `embed-log.demo.yml` — demo runtime configuration (YAML, version 1).
- `examples/embed-log.yml` — example user/CI YAML config.
- `requirements.txt` / `pyproject.toml` — Python dependencies and packaging metadata.

## Fast orientation by task

- Need to change ingestion or protocol? → `backend/core/runtime.py`
- Need to change browser behavior/layout? → `frontend/`
- Need demo traffic? → `utils/udp_log_simulator.py`, `utils/inject_log_demo.py`
- Need docs first? → `README.md`, then `FRONTEND.md` or `INSTALL.md`
