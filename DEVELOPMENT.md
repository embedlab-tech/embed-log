# Development guide

This document covers how to work with the `embed-log` source code.

---

## One-time setup

```bash
git clone git@github.com:krezolekcoder/embed-log.git
cd embed-log
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
pip install -e .
```

Notes:
- `pip install -e .` makes the `embed-log` CLI available inside the venv
- it also means any edits to `.py` files are reflected immediately — no re-install needed

---

## Running the CLI during development

There are two equivalent ways to run the tool from source:

### Method A — via the module (recommended for quick tests)

```bash
python3 -m backend.server create-config
python3 -m backend.server validate --config embed-log.yml
python3 -m backend.server run --config embed-log.yml
```

Advantages:
- always runs from source
- works even when a global pipx install of `embed-log` is on PATH
- no venv activation required (as long as deps are installed)

### Method B — via the `embed-log` command (venv must be active)

```bash
source .venv/bin/activate
embed-log create-config
embed-log validate --config embed-log.yml
embed-log run --config embed-log.yml
```

Advantages:
- same command as end users type
- useful to verify the installed entry point behaves correctly

Note: if you also have `embed-log` installed via pipx system-wide, the venv's copy takes precedence when the venv is active, because `.venv/bin` comes first on `PATH`.

---

## Running the demo

The bundled demo starts a server with three simulated sources and injects test traffic.

```bash
./run_demo.sh --no-browser
# or with the fast deterministic profile:
./run_demo.sh --profile test --fast --no-browser
# or faster random traffic:
./run_demo.sh --profile random --fast --no-browser
```

Default UI: `http://127.0.0.1:8080/`

See `./run_demo.sh --help` for all options.

---

## Running tests

Backend:

```bash
python3 -m unittest discover -s tests -v
```

Frontend (requires demo server running separately):

```bash
cd tests-ui
npm test
```

---

## Config wizard

The `create-config` command walks through:
- tab labels
- per-pane source type (UART / UDP)
- serial port detection + baudrate for UART
- UDP port selection

It generates a ready-to-use YAML config. This is the recommended onboarding path.

See `CLI_SIMPLIFY_PLAN.md` for planned CLI improvements.

---

## Project layout

Key directories:

| Path | Purpose |
|------|---------|
| `backend/cli.py` | CLI entry point, argument parsing, config wizard |
| `backend/core/runtime.py` | Server runtime, source management, session lifecycle |
| `backend/config/loader.py` | YAML config loading and validation |
| `backend/sources/` | Source reader implementations (UART, UDP) |
| `backend/net/` | Network servers (WebSocket, inject, forward) |
| `backend/session/` | Session metadata and HTML export |
| `frontend/` | Plain-JS browser UI (no build tool) |
| `tests/` | Backend unit tests |
| `tests-ui/` | Playwright UI tests |
| `docs/` | Documentation index and subsystem details |

---

## Making changes and testing them

1. Edit the `.py` or `.js` file
2. Run the relevant test file:
   ```bash
   python3 -m unittest tests.test_queue_stats -v
   ```
3. Or run the demo manually and verify behavior:
   ```bash
   python3 -m backend.server run --config embed-log.demo.yml -v --no-open-browser
   ```
4. For frontend changes, reload the browser UI (no build step needed)

---

## Debugging

- Backend uses Python's `logging` module. Increase verbosity with `-v` (events) or `--verbose-full` (every log line to stdout).
- Frontend errors appear in the browser devtools console.
- If a serial port is unavailable, the UART source retries every 3 seconds and logs a warning.
- WebSocket connection status is shown in the UI top bar.
