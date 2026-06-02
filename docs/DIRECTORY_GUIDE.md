# Directory Guide

Short, practical map of this repository for humans and coding agents.

`embed-log` is a backend-first, configurable log aggregation system (UART/UDP + inject/TX TCP + session artifacts) with a dynamic browser UI driven by backend tabs/panes config.

## Root directories

- `backend/`
  - Core log server, CLI, and Python APIs.
  - Key files:
    - `server.py` — compatibility entrypoint (`backend.server:main`).
    - `app.py` — app composition/startup (sources, tabs, session dir/log wiring).
    - `cli/` — CLI package (`__init__.py` re-exports `main()`):
      - `dispatch.py` — `main()` dispatcher, match/case routing.
      - `parser.py` — argparse construction for all subcommands.
      - `util.py` — pure helpers: timestamps, durations, file stats, session IDs.
      - `wizard.py` — `create-config` interactive wizard.
      - `run.py` — `run` / `validate` / `merge`.
      - `diagnostics.py` — `version` / `doctor` / `ports`.
      - `update.py` — self-update logic.
      - `demo.py` — demo utility commands.
      - `sessions/` — subpackage for all `sessions` subcommands.
    - `config/loader.py` — YAML config parsing/validation.
    - `config/models.py` — typed config dataclasses (`AppConfig`, `SourceConfig`, etc.).
    - `core/runtime.py` — runtime engine (`LogServer`, `SourceManager`).
    - `core/models.py` — `LogEntry`, `QueueStats` dataclasses.
    - `core/queue.py` — `TrackedQueue` (bounded queue with saturation tracking).
    - `core/clock.py` — `SessionClock` (absolute/relative timestamp modes).
    - `core/naming.py` — `slugify()`.
    - `core/ansi.py` — ANSI color codes dict.
    - `net/ws_server.py` — WebSocket/HTTP UI server implementation.
    - `net/inject_server.py` / `net/forward_server.py` — TCP inject/forward socket servers.
    - `sources/base.py` — `LogSource` ABC.
    - `sources/raw_base.py` — `RawLogSource` ABC for byte-stream sources.
    - `sources/parsed.py` — `ParsedSource` adapter (wraps raw + parser into `LogSource`).
    - `sources/uart.py`, `sources/udp.py` — convenience wrappers.
    - `sources/raw_uart.py`, `sources/raw_udp.py` — raw source implementations.
    - `parsers/` — stream parser abstraction:
      - `base.py` — `StreamParser` ABC.
      - `text.py` — `TextParser` (newline-delimited UTF-8).
      - `cbor_datagram.py` — `CborDatagramParser`.
      - `factory.py` — `create_parser()`.
    - `session/manager.py` — session metadata/manifest.
    - `session/exporter.py` — HTML export orchestration.
    - `sinks/` — output sink abstractions (extensible).
    - `log_client.py` — marker injection + stream subscription client.
    - `tx_client.py` — TX-only client.
    - `parse.py` — parse exported HTML back to raw logs.

- `frontend/`
  - Browser UI (vanilla HTML/CSS/JS, no build step).
  - Handles tabs/panes, live websocket updates, filtering, selection, export, import, pane swapping, splitter drag, and refresh persistence cache.
  - Key modules:
    - `state.js` / `tabs.js` / `tabcreate.js` / `ui.js` — layout and state.
    - `ws.js` — WebSocket transport.
    - `lines.js` — log line rendering and sync logic.
    - `selection.js` — range selection and overlay.
    - `export.js` / `import.js` — export/import flows.
    - `renderPane.js` / `renderToolbar.js` — modular rendering.
    - `settings.js` / `themes.js` / `fontsize.js` — user preferences.
    - `persist.js` — local storage cache.
    - `tsparse.js` — timestamp parsing.
    - `profile.js` — demo profile config.
    - `viewer.css` — styles and themes.

- `utils/`
  - Helper scripts for demos and offline workflows.
  - Includes UDP simulator, inject demo sender, deterministic traffic generator, curated demo log generator, and log merge utility.

- `benchmarks/`
  - `serial_stress.py` — cross-platform stress benchmark for backend throughput.

- `logs/`
  - Runtime output generated per session (e.g. `logs/<session_id>/`).
  - Session directory contains raw logs plus `manifest.json` and `session.html`.

- `tests/`
  - Unit tests for config parsing, source parsing, parsers, session components, CLI, and runtime.

- `tests-ui/`
  - Playwright UI/E2E tests for the browser frontend.

- `scripts/`
  - `test-backend.sh` / `test-ui.sh` — convenience test runners.

## Important root files

- `README.md` — main project documentation and backend overview.
- `AGENTS.md` — quick instructions for future contributors/agents.
- `docs/` — curated documentation set (start at `docs/README.md`).
- `run_demo.sh` — one-command local demo launcher.
- `embed-log.demo.yml` — demo runtime configuration.
- `examples/embed-log.yml` — example user/CI YAML config.
- `pyproject.toml` / `requirements.txt` — Python dependencies and packaging metadata.

## Fast orientation by task

- Need to change ingestion or protocol? → `backend/core/runtime.py`
- Need to change browser behavior/layout? → `frontend/`
- Need to change parser logic? → `backend/parsers/`
- Need demo traffic? → `utils/deterministic_demo_traffic.py`, `utils/curated_demo_logs.py`
- Need docs first? → `README.md`, then `docs/README.md`
