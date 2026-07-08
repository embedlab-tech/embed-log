# SDK/control API implementation summary

This work replaced the old per-source SDK port model with a single structured control WebSocket and a Python SDK/watch workflow around it.

## Final state

- Runtime exposes a structured control WebSocket at `/api/v1/control` when `server.control_api` is enabled.
- Clients route all automation by configured source name instead of opening separate inject/forward ports.
- UART TX writes real bytes to writable UART sources and records TX log entries.
- UART command suggestions load from companion YAML files and appear in runtime/session metadata.
- Markers can be created from the control API, inspected from the CLI, and used by watcher automation.
- Python SDK supports inject, TX, subscription, markers, config-driven initialization, and watcher workflows.
- End-to-end tests exercise the Rust backend plus Python SDK without real hardware.
- Docs/configs now describe the new model and mark legacy inject/forward ports as deprecated.

## Phase 1 — Real UART TX backend

Implemented real UART transmit support in the Rust backend.

- Added per-source TX command channels for writable sources.
- `send_raw`/TX requests now write bytes to the UART serial port instead of being UI-only.
- Successful TX writes emit yellow TX log entries with origin metadata.
- TX command acknowledgements report actual write success/failure.
- Added UART tests using PTY-backed serial ports for exact-byte TX verification.

## Phase 2 — UART command suggestions

Implemented configuration-driven UART command suggestions.

- Added companion command file loading:
  - `<config-stem>.commands.yml`
  - `embed-log.commands.yml` next to the config
  - `embed-log.commands.yml` in the current working directory
- Commands are filtered to known writable sources.
- Runtime/session metadata preserves command suggestions across session rotation.
- Added tests for command loading and metadata behavior.

## Phase 3 — Structured control WebSocket

Added the new single automation endpoint.

- Added `/api/v1/control` WebSocket.
- Implemented commands:
  - `hello`
  - `subscribe`
  - `unsubscribe`
  - `log.inject`
  - `tx.write`
- Subscriptions emit structured `log.entry` events with source, origin, timestamp, line index, color, and TX metadata.
- Empty subscriptions mean no log delivery.
- `tx.write` waits for backend write acknowledgement before returning `tx.result`.
- Added control WebSocket tests for command handling, subscription filtering, inject, TX success/failure, and structured entries.

## Phase 4 — Marker API

Implemented marker creation through the control API.

- Added `marker.create` command.
- Validates source and line index.
- Resolves marker timestamp from request or replay buffer.
- Persists markers in the frontend-compatible marker format.
- Replaces existing marker for the same pane/line.
- Broadcasts `markers_update` for UI/frontends.
- Added tests for marker validation, persistence, replacement, and timestamp behavior.

## Phase 5 — Marker CLI inspection

Added marker inspection commands to the CLI.

- Added:
  - `embed-log sessions marker list <session-id>`
  - `embed-log sessions marker show <session-id> <marker-index>`
  - `--json`
  - `--search`
  - `--pane`
- Added `sessions list --with-markers`.
- Supports marker wrapper files and plain marker arrays.
- Handles line ranges and missing marker fields correctly.
- Added CLI tests and smoke verification for watcher-created marker inspection.

## Phase 6 — Python SDK

Created the Python SDK under `sdk/python`.

- Added `EmbedLogClient` with synchronous WebSocket support.
- Supports:
  - `from_config()`
  - `inject_log()`
  - `tx_write()`
  - `subscribe()` / `unsubscribe()`
  - `entries()`
  - `create_marker()`
- Matches command responses by request id.
- Buffers interleaved `log.entry` messages so command waits do not lose logs.
- Adds command timeouts and typed exceptions.
- Parses embed-log YAML for early source validation and command metadata.
- Added unit tests for config parsing, client protocol behavior, interleaving, errors, and timeouts.

## Phase 7 — Python watcher

Implemented Python watcher automation on top of the SDK.

- Added `embed_log_sdk.watcher`.
- Watch rules match regex patterns against subscribed log entries.
- Watcher subscribes to the union of configured source rules.
- Writes JSONL evidence with source, line index, timestamp, origin, message, and regex groups.
- Optional `marker: true` creates UI markers through `marker.create`.
- Added watcher examples:
  - `examples/watcher.yml`
  - `examples/watcher_run.py`
- Added watcher tests for evidence output, source subscriptions, marker creation, and deterministic timeout behavior.

## Phase 8 — End-to-end tests

Added E2E coverage for the Rust backend plus Python SDK.

- Starts a real `embed-log` server with temporary config/log directory.
- Uses a PTY-backed UART source and a UDP source.
- Verifies:
  - SDK `from_config()` handshake
  - injected logs reach subscription and log files
  - UART `tx_write()` writes exact bytes and records TX log entries
  - source subscription filtering
  - watcher JSONL evidence output
  - watcher marker creation in `markers.json`
  - `markers_update` broadcast
  - command suggestions in runtime/session metadata
  - CLI `sessions marker list/show` can verify watcher-created markers
- Tests run without real hardware.

## Phase 9 — Docs and config cleanup

Updated documentation and config shape for the new model.

- Documented the control WebSocket model, source-name routing, SDK usage, watcher usage, command suggestions, and marker CLI inspection.
- Added `server.control_api` config with default `true`.
- Wired `server.control_api` into server route registration.
- Removed active legacy inject/forward fields from generated/demo configs.
- Marked legacy per-source `inject_port`, `forward_port`, and `forward_ports` as deprecated migration-only fields.
- Added tests that sample configs parse and do not contain active legacy directives.
- Added config test for `control_api: false`.

## Legacy compatibility status

Legacy per-source inject/forward fields still parse and produce deprecation warnings. New configs and docs should use the control API instead. Existing compatibility code remains in the runtime for now, but the preferred automation path is the single control WebSocket.

## Verification performed during review

Across the phases, targeted verification included:

- `just fmt-check`
- `cargo check --workspace`
- `cargo check -p embed-log-core --target x86_64-pc-windows-msvc`
- `cargo clippy -p embed-log-core --all-targets -- -D warnings`
- `cargo clippy -p embed-log-cli --all-targets -- -D warnings`
- `cargo test -p embed-log-core`
- `cargo test -p embed-log-core net::control_ws -- --nocapture`
- `cargo test -p embed-log-core sources::uart -- --nocapture`
- `cargo test -p embed-log-core config::loader -- --nocapture`
- `cargo test -p embed-log-cli`
- Python SDK unit tests with `pytest -q`
- Python E2E tests with `pytest tests/test_e2e.py -q`
- Python `compileall` over SDK, tests, and examples
- `git diff --check`
