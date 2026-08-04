# Embed-log MVP overhaul

Implementation handoff for reducing Embed-log to an agent-friendly, persistent multi-source logger while retaining the browser, TUI, UART TX, required protocol parsers, and efficient retrospective analysis.

## Goal

Embed-log should support:

- humans watching logs in a browser or TUI;
- agents running repeated hardware experiments;
- UART RX and TX through the process that owns the serial ports;
- multiple correlated log sources;
- token-efficient real-time watches;
- bounded post-factum analysis.

The backend captures and persists everything. CLI queries return only bounded evidence unless full output is explicitly requested.

## 1. Runtime modes

```bash
# Human: foreground server, opens browser
embed-log run --config embed-log.yml

# Human/CI: foreground server, no browser
embed-log run --config embed-log.yml --no-browser

# Human: foreground server with TUI
embed-log run --config embed-log.yml --tui

# Agent: background daemon
embed-log run --config embed-log.yml --daemon --instance bench-a --port 18080
```

### `--daemon` semantics

- Start in the background.
- Never open a browser or onboarding.
- Wait for the HTTP server and sources to become ready before returning.
- Register the PID, instance name, endpoint, config path, logs directory, and diagnostic-log path.
- Keep UARTs open across experiment sessions.
- Skip automatic HTML export by default.
- Shut down gracefully through `embed-log stop`.

`--daemon` replaces the previously discussed `--headless` behavior. `--no-browser` means foreground without a browser.

## 2. Default server port

Change the HTTP/WebSocket/control API default from `8080` to `18080`:

```yaml
server:
  listen: 127.0.0.1:18080
```

CLI override:

```bash
embed-log run --port 18080
```

This is not a UDP source port. The same server port hosts browser HTTP, the browser WebSocket, the control WebSocket, and the status API. UDP log sources always require explicit ports and have no shared default.

Daemon startup requires an explicit `--port` and never scans for another port. Repeating the same instance/endpoint/config request reuses the verified existing daemon; conflicts fail visibly.

## 3. Named daemon instances

```bash
embed-log run --daemon --instance bench-a --config bench-a.yml --port 18080
embed-log run --daemon --instance bench-b --config bench-b.yml --port 18081
```

Target a daemon with:

```bash
embed-log status --instance bench-a
```

Read-only status resolution order:

1. `--instance <name>`;
2. `EMBED_LOG_INSTANCE`;
3. automatically select the only running daemon;
4. if multiple daemons are running, fail and list their names.

Mutating commands require `--instance`, `EMBED_LOG_INSTANCE`, or an explicit URL where supported; they never infer the only daemon.

Also support an explicit endpoint for remote or unregistered servers:

```bash
embed-log status --url http://127.0.0.1:18080
```

On Linux, instance records can live under `$XDG_RUNTIME_DIR/embed-log/`, with a suitable user-state fallback. Detect and clean stale PID records safely.

## 4. Session rotation and titles

Keep the daemon alive while creating a new logical session for every experiment:

```bash
embed-log sessions new \
  --instance bench-a \
  --title edhoc-reconnect-attempt-3 \
  --json
```

This must:

- rotate without releasing UARTs;
- accept the title through the existing rotation mechanism;
- preserve the original title in `manifest.json`;
- slugify it for the directory name;
- return the exact new session ID;
- notify browser and TUI clients with `session_rotated`;
- clear and switch the live view.

Example directory:

```text
2026-08-03_14-22-10_edhoc-reconnect-attempt-3/
```

The frontend already reconnects to a restarted backend on the same host and port and handles `session_rotated`. Prefer rotation over restarting the process.

## 5. Minimal CLI

Keep this primary surface:

```text
embed-log run
embed-log status
embed-log stop
embed-log doctor
embed-log ports
embed-log validate
embed-log version

embed-log sessions new
embed-log sessions list
embed-log sessions summary
embed-log sessions search
embed-log sessions read
embed-log sessions around
embed-log sessions open
embed-log sessions export

embed-log tx
embed-log watch add
embed-log watch wait
embed-log watch remove
```

Fold useful information from `sessions info` into `sessions summary`.

## 6. UART experiments

UART TX must remain because Embed-log owns the serial port.

Simple TX:

```bash
embed-log tx \
  --instance bench-a \
  --source DUT_UART \
  --line status \
  --json
```

`--line` should append the configured line ending, avoiding shell `\r\n` quoting problems. Support raw, file, and stdin input where required.

### Atomic TX and expectation

The primary agent command is:

```bash
embed-log tx \
  --instance bench-a \
  --source DUT_UART \
  --line reset \
  --expect "boot complete" \
  --timeout 30s \
  --context 20 \
  --json
```

Internally:

1. Arm the expectation before TX.
2. Send through the already-open UART.
3. Wait for a matching RX record.
4. Return the event and bounded context.
5. Record TX origin in the session.

Substring matching should be the default. Use `--expect-regex` for advanced matching.

## 7. Watches

Watches handle experiments triggered outside UART TX:

```bash
embed-log watch add \
  --instance bench-a \
  --source DUT_UART \
  --contains "session established" \
  --once \
  --ttl 30s \
  --json

embed-log watch wait \
  --instance bench-a \
  <WATCH_ID> \
  --timeout 30s \
  --json

embed-log watch remove --instance bench-a <WATCH_ID>
```

Requirements:

- Implement watches using the existing runtime event-rule pipeline.
- Retain match state so a match occurring before `watch wait` is not lost.
- Default to one match and a short lifetime.
- Do not stream ordinary logs to the waiting CLI.
- Return session, event, source, line, sequence, timestamp, message, and captures.
- Keep watches temporary; do not persist them into project configuration by default.

`TTL` is the amount of time a watch remains active before automatic expiration.

## 8. Efficient session analysis

Recommended agent sequence:

```text
sessions summary
→ search count
→ bounded read or context
```

Examples:

```bash
embed-log sessions summary --instance bench-a <SESSION_ID> --json

embed-log sessions search \
  --instance bench-a \
  <SESSION_ID> \
  --contains timeout \
  --count

embed-log sessions read \
  --instance bench-a \
  <SESSION_ID> \
  --source DUT_UART \
  --last 100 \
  --json

embed-log sessions around \
  --instance bench-a \
  <SESSION_ID> \
  --event <EVENT_ID> \
  --before 10 \
  --after 20 \
  --json
```

Never return unlimited logs by default.

## 9. Global sequence and cursors

Add a session-wide monotonic `sequence` to every combined and streamed record while retaining source-local `line_idx`:

```json
{
  "sequence": 719,
  "session_id": "...",
  "source_id": "DUT_UART",
  "line_idx": 428,
  "timestamp_iso": "...",
  "message": "boot complete"
}
```

Use it for:

- `sessions read --after <cursor>`;
- exact cross-source ordering;
- event context;
- reconnect and replay;
- stream-gap recovery.

Eventually support subscribe-with-replay so persisted replay transitions atomically into live delivery.

## 10. Structured output contract

Agent commands should consistently support JSON:

```json
{
  "ok": true,
  "session_id": "...",
  "result": {},
  "truncated": false,
  "next_cursor": null
}
```

Rules:

- stdout contains only requested JSON or JSONL;
- diagnostics go to stderr;
- outputs are bounded;
- report stream gaps, truncation, and invalid stored records;
- errors contain a stable code, message, valid choices, and actionable hint;
- concise agent JSONL uses full source IDs, not shortcodes requiring stderr legends.

Example error:

```json
{
  "ok": false,
  "code": "MULTIPLE_INSTANCES",
  "instances": ["bench-a", "bench-b"],
  "hint": "Repeat with --instance bench-a."
}
```

## 11. Browser, TUI, and HTML policy

### Keep

- Browser UI.
- Integrated TUI mode.
- Standalone TUI client if it remains inexpensive.

### Delete

- Tauri crate and application.
- `embed-log --ui`.
- Tauri configuration, documentation, and release paths.

### HTML export

Foreground browser, TUI, and `--no-browser` modes:

- export the completed session on rotation;
- export the current session on clean shutdown or SIGTERM.

Daemon mode:

- skip HTML by default;
- permit an explicit override.

Keep:

```bash
embed-log sessions open <SESSION_ID>
embed-log sessions export <SESSION_ID>
```

`sessions open` should regenerate missing or stale HTML. Browser disconnects do not trigger automatic HTML export.

## 12. Sources and backend parsers

### Keep sources

```text
UART
UDP text
file tail
```

### Keep parsers

```text
text
slip-coap
hex-coap
zephyr-dict
```

Current verification performed during planning:

- SLIP/CoAP unit tests: 6 passed.
- Zephyr dictionary tests: 24 passed.
- The optional real Zephyr fixture test skipped without environment variables and must become mandatory.

### CoAP backend migration

The current frontend `hex-coap` plugin handles arbitrary textual hex lines and is richer than the backend `slip-coap` parser. Before deleting frontend plugins:

1. Port textual hex scanning to Rust as `hex-coap`.
2. Share one Rust CoAP decoder between `slip-coap` and `hex-coap`.
3. Preserve original messages.
4. Persist structured CoAP metadata.
5. Render that metadata directly in the browser and TUI.
6. Add request, response, option, block, malformed-frame, and frontend-parity tests.

Example metadata:

```json
{
  "coap": {
    "type": "CON",
    "code": "GET",
    "message_id": 4660,
    "uri": "/status",
    "token": "ab",
    "payload_len": 0
  }
}
```

### Remove

```text
CBOR parser
network_capture/pcap
generic frontend plugin framework
```

## 13. YAML configuration overhaul

Introduce a minimal version 2 format:

```yaml
version: 2

server:
  listen: 127.0.0.1:18080

logs:
  dir: ./logs

sources:
  DUT:
    type: uart
    path: /dev/ttyUSB0
    baud: 115200
    parser:
      type: zephyr-dict
      database: build/log_dictionary.json

  LINK:
    type: uart
    path: /dev/ttyUSB1
    baud: 921600
    parser:
      type: slip-coap

  HOST:
    type: udp
    port: 16000
    parser:
      type: hex-coap

  TEST:
    type: file
    path: ./pytest.log
```

Optional UI layout:

```yaml
ui:
  tabs:
    - title: Device
      sources: [DUT, TEST]

    - title: Protocol
      sources: [LINK, HOST]
```

Without `ui`, generate one tab per source.

Remove from YAML:

- frontend and pane plugins;
- pcap/network-capture fields;
- CBOR settings;
- onboarding settings;
- `open_browser`;
- Tauri settings;
- global baud rate;
- demo fields.

Runtime behavior belongs to CLI flags:

```text
--daemon
--instance
--tui
--no-browser
--session-title
--export-html
```

## 14. CLI-only capture configuration

All essential source configuration should work without YAML:

```bash
embed-log run \
  --uart DUT=/dev/ttyUSB0 \
  --uart LINK=/dev/ttyUSB1 \
  --udp HOST=16000 \
  --file TEST=./pytest.log \
  --baud DUT=115200 \
  --baud LINK=921600 \
  --parser DUT=zephyr-dict \
  --parser-db DUT=build/log_dictionary.json \
  --parser LINK=slip-coap
```

Save the generated configuration:

```bash
embed-log run \
  --uart DUT=/dev/ttyUSB0 \
  --save-config embed-log.yml
```

Use one auto-generated UI tab per source for CLI-only runs. Complex layouts may remain YAML-only.

## 15. Remove obsolete functionality

Delete from the production CLI:

```text
onboard
demo
init
update
merge
parse

sessions import
sessions bundle
sessions prune
sessions marker
```

Also remove:

- automatic onboarding;
- Tauri;
- generic frontend plugins;
- pcap/native packet-capture dependencies;
- CBOR;
- demo-specific source generation.

Fix or replace the checked-in root config, which currently contains the removed `server.open_browser` field and fails validation.

## 16. Keep and improve `doctor`

```bash
embed-log doctor --config embed-log.yml --json
embed-log doctor --instance bench-a --json
```

Check:

- config validity;
- UART existence and permissions;
- log-directory writability;
- parser/source compatibility;
- Zephyr dictionary paths;
- HTTP port conflicts;
- daemon registry and stale PIDs;
- backend readiness;
- source runtime health;
- conflicting instances.

Source status should distinguish:

```text
starting
running
idle
failed
reconnecting
stopped
```

Do not report configured sources as unconditionally available.

## 17. Replace `demo` in tests

Synthetic traffic belongs under test infrastructure, not the production CLI:

```text
tests/fixtures/
tests/support/
scripts/start-test-rig
```

Use:

- a Linux pseudo-terminal UART fixture;
- an external UDP traffic generator;
- fixed SLIP/CoAP captures;
- a real Zephyr dictionary/capture fixture;
- temporary configuration and log directories.

## 18. Linux local acceptance

Run static and unit checks:

```bash
cargo fmt --all -- --check

cargo clippy --locked \
  --package embed-log-core \
  --package embed-log-cli \
  --all-targets -- -D warnings

cargo test --locked \
  --package embed-log-core \
  --package embed-log-cli
```

Add `scripts/test-mvp-linux.sh` covering:

1. Start a pseudo-terminal device simulator.
2. Start daemon `test-a`.
3. Verify status and source health.
4. Create a titled session.
5. Send a UART command with `--expect`.
6. Verify TX and RX persistence and bounded context.
7. Start daemon `test-b` on another port.
8. Verify ambiguous instance selection fails with a useful hint.
9. Stop both daemons and verify UART release and instance cleanup.
10. Run foreground browser and TUI modes.
11. Rotate a session without restarting.
12. Verify the browser follows rotation.
13. Exit cleanly and verify self-contained HTML export.
14. Verify removed commands and config fields are rejected clearly.
15. Test the packaged release executable, not only `cargo run`.

## 19. Pi/model benchmark after CLI stabilization

Create a deterministic firmware-like fixture where an agent:

- implements a small UART command;
- creates a titled Embed-log session;
- sends valid and invalid commands;
- verifies expected logs with `tx --expect`;
- retrieves bounded evidence.

Run Pi in JSON mode and measure:

- task success;
- invalid CLI calls;
- tool calls;
- total tokens;
- captured records versus records returned to the model;
- unnecessary raw-session reads.

Keep the LLM benchmark nightly or manual. Deterministic Rust, CLI, PTY, parser, and browser tests remain required PR checks.

## Suggested implementation order

1. Remove obsolete/Tauri/demo/pcap/CBOR surfaces and fix checked-in configs.
2. Introduce config v2 and the `18080` server default.
3. Add daemon lifecycle, instance registry, readiness, status, and stop.
4. Add titled session rotation and browser/TUI continuity.
5. Add CLI UART TX and atomic `--expect`.
6. Add durable temporary watches.
7. Add global sequence, bounded read, and event context.
8. Normalize JSON output and errors.
9. Move frontend CoAP parsing into the backend and remove generic plugins.
10. Add the Linux MVP integration harness and model benchmark.
