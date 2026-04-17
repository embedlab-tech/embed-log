# Directory Guide

Short, practical map of this repository for humans and coding agents.

## Root directories

- `backend/`
  - Core log server and Python client APIs.
  - Key files:
    - `server.py` — main runtime (sources, inject ports, WS UI broadcast).
    - `log_client.py` — marker injection + stream subscription client.
    - `tx_client.py` — TX-only client.

- `frontend/`
  - Browser UI (vanilla HTML/CSS/JS, no build step).
  - Handles tabs/panes, live websocket updates, filtering, selection, export, import, pane swapping, splitter drag, and refresh persistence cache.

- `utils/`
  - Helper scripts for demos and offline workflows.
  - Includes UDP simulator, inject demo sender, and log merge utility.

- `logs/`
  - Runtime output logs (`<SOURCE>.log`).
  - Generated/updated when server runs.

- `.venv/`
  - Local virtual environment (developer-local).

- `.git/`
  - Git metadata.

- `~/`
  - Local scratch/session directory present in this workspace (not core app logic).

## Important root files

- `README.md` — main project documentation and backend overview.
- `AGENTS.md` — quick instructions for future contributors/agents.
- `INSTALL.md` — setup and run prerequisites.
- `FRONTEND.md` — frontend internals.
- `MERGE.md` — merged-log report behavior.
- `SAMPLE_COMMANDS.md` — copy/paste examples.
- `run_demo.sh` — one-command local demo launcher.
- `requirements.txt` / `pyproject.toml` — Python dependencies and metadata.

## Fast orientation by task

- Need to change ingestion or protocol? → `backend/server.py`
- Need to change browser behavior/layout? → `frontend/`
- Need demo traffic? → `utils/udp_log_simulator.py`, `utils/inject_log_demo.py`
- Need docs first? → `README.md`, then `FRONTEND.md` or `INSTALL.md`
