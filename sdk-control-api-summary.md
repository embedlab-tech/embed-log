# SDK/control API implementation summary

This work replaced the old per-source SDK port model with a single structured control WebSocket and Python SDK.

## Final state

- Runtime exposes a structured control WebSocket at `/api/v1/control` when `server.control_api` is enabled.
- Clients route all automation by configured source name instead of opening separate inject/forward ports.
- UART TX writes real bytes to writable UART sources and records TX log entries.
- UART command suggestions load from companion YAML files and appear in runtime/session metadata.
- Markers can be created from the control API and inspected from the CLI.
- Python SDK supports inject, TX, source-filtered subscriptions, markers, and config-driven initialization.
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
- Subscriptions emit structured `log.entry` messages with source, origin, timestamp, line index, color, and TX metadata.
- Empty subscriptions mean no log delivery.
- `tx.write` waits for backend write acknowledgement before returning `tx.result`.
- Added control WebSocket tests for command handling, subscription filtering, inject, TX success/failure, and structured entries.

## MVP — Retained temporary watches

The control WebSocket also supports process-local one-shot watches:

- `watch.create` adds a process-local literal or regex matcher with a bounded TTL;
- `watch.get` returns `active`, retained `matched`, or `expired` state;
- `watch.delete` removes the state and deactivates its rule.

Matches are retained in process memory until removed or until shutdown and are never written to session artifacts. CLI users should prefer `embed-log watch add|wait|remove` rather than issuing these protocol messages directly.

## Phase 4 — Marker API

Implemented marker creation through the control API.

- Added `marker.create` command.
- Validates source and line index.
- Resolves marker timestamp from request or replay buffer.
- Persists markers in the frontend-compatible marker format.
- Replaces existing marker for the same pane/line.
- Broadcasts `markers_update` for UI/frontends.
- Added tests for marker validation, persistence, replacement, and timestamp behavior.

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

## Phase 8 — End-to-end tests

Added E2E coverage for the Rust backend plus Python SDK.

- Starts a real `embed-log` server with temporary config/log directory.
- Uses a PTY-backed UART source and a UDP source.
- Verifies:
  - SDK `from_config()` handshake
  - injected logs reach subscription and log files
  - UART `tx_write()` writes exact bytes and records TX log entries
  - source subscription filtering
  - marker creation in `markers.json`
  - `markers_update` broadcast
  - command suggestions in runtime/session metadata
- Tests run without real hardware.

## Phase 9 — Docs and config cleanup

Updated documentation and config shape for the new model.

- Documented the control WebSocket model, source-name routing, SDK usage, temporary watches, and command suggestions.
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
