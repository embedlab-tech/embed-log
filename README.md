# embed-log

`embed-log` collects UART, UDP, and file-tail logs, stores them as session artifacts, and serves browser and terminal interfaces for live viewing and static HTML exports.

The current workspace contains:

- `embed-log` CLI: run the log server, inspect and export sessions, and launch browser or TUI mode.
- `embed-log-core`: shared config, sources, parsers, runtime, HTTP/WebSocket server, and session export logic.
- `embed-log-tui`: terminal client used by integrated and standalone TUI modes.
- `frontend/`: browser UI assets embedded into release binaries.

## Install

macOS/Linux latest release:

```bash
curl -fsSL https://github.com/embedlab-tech/embed-log/releases/latest/download/install.sh | sh
```

Windows PowerShell latest release:

```powershell
irm https://github.com/embedlab-tech/embed-log/releases/latest/download/install.ps1 | iex
```

Release binaries include embedded frontend assets, so users do **not** need Rust, Cargo, or a separate `frontend/` directory.

See [docs/releasing.md](docs/releasing.md) for release and installer details.

## Fast start

Connect one UART without creating YAML:

```bash
embed-log run /dev/ttyUSB0
```

Multiple UARTs, a watched file, or the terminal UI:

```bash
embed-log run -s /dev/ttyUSB0 -s /dev/ttyUSB1 -f ./device.log --baud 115200
embed-log run /dev/ttyUSB0 --tui
```

This creates an in-memory configuration, opens the web UI (or TUI), and saves a normal session under `./logs/`. Each source gets its own tab. Persist the generated configuration only when you need a custom layout or parser:

```bash
embed-log run /dev/ttyUSB0 --save-config embed-log.yml
```

See the [quick-start guide](docs/quickstart.md) for all fast-run options, session locations, and when to switch to YAML. Run `embed-log doctor` if a serial device cannot be opened.

## Claude Code plugin

This repo bundles a [Claude Code](https://claude.com/claude-code) skill that teaches an AI
agent to inspect `embed-log` sessions via the CLI (`sessions summary`/`search`/...) instead of
grepping raw log files. Install it once, in any Claude Code session:

```
/plugin marketplace add embedlab-tech/embed-log
/plugin install embed-log@embed-log-tools
```

It's then available in every project on your machine, not just this repo. Source: `skills/embed-log/SKILL.md`, `.claude-plugin/`.

## Build from source

```bash
just build
just run no-browser embed-log.yml
```

Then open:

```text
http://127.0.0.1:8080/
```

Copy a relevant file from `config-samples/`, then validate and run it:

```bash
cargo run --package embed-log-cli --bin embed-log -- validate --config embed-log.yml
cargo run --package embed-log-cli --bin embed-log -- run --config embed-log.yml
```

## Control API

Embed-log exposes a single structured JSON WebSocket endpoint for SDK and automation:

```text
ws://127.0.0.1:8080/api/v1/control
```

### Commands

| Command | Purpose |
|---------|---------|
| `hello` | Get sources, labels, types, writability, session id |
| `subscribe` | Subscribe to log entries by source name |
| `unsubscribe` | Remove source subscriptions |
| `log.inject` | Inject a log entry into the source pipeline and UI |
| `tx.write` | Write bytes to a writable source (UART) |
| `marker.create` | Create a marker on a log line |

### `subscribe` / `log.entry`

Subscribe to sources and receive structured events replacing the legacy per-source forward ports:

```json
{
  "type": "log.entry",
  "source_id": "DUT_UART",
  "origin": "SERIAL",
  "message": "boot complete",
  "timestamp_iso": "2026-06-14T12:00:00.123Z",
  "line_idx": 42,
  "color": null,
  "is_tx": false
}
```

Source-name routing replaces the old `InjectClient`/`ForwardClient` per-port model.

## Python SDK

A synchronous Python SDK is available at `sdk/python/`:

```python
from embed_log_sdk import EmbedLogClient

with EmbedLogClient.from_config("embed-log.yml", origin="pytest") as client:
    client.inject_log("DUT_UART", "test: assertion passed", color="cyan")
    client.tx_write("DUT_UART", "version\r\n")
    client.subscribe(["DUT_UART"])
    for entry in client.entries(timeout=5.0):
        print(entry.source_id, entry.message)
```

## Watcher

The watcher (`embed_log_sdk.watcher`) observes log entries matching regex patterns, writes JSONL evidence, and optionally creates UI markers:

```bash
python sdk/python/examples/watcher_run.py --config watcher.yml --timeout 30
```

## Companion UART command files

Place a `<config-stem>.commands.yml` alongside your config to provide Tab-cycling command suggestions:

```yaml
sources:
  DUT_UART:
    - "help\r\n"
    - "version\r\n"
    - "status\r\n"
```

The fallback `embed-log.commands.yml` is checked in the config directory and current working directory.

## Marker CLI inspection

List and inspect markers created by the watcher or UI:

```bash
embed-log sessions marker list <session-id>
embed-log sessions marker show <session-id> <marker-index>
embed-log sessions marker list <session-id> --search fatal --json
embed-log sessions marker show <session-id> 1 --json
```

## Terminal UI

Run the server and terminal UI together:

```bash
embed-log run --config embed-log.yml --tui
```

Or connect the standalone TUI to an already-running server:

```bash
embed-log-tui connect ws://127.0.0.1:8080/ws
```

See [docs/tui.md](docs/tui.md) for keybindings and limitations.

## Legacy inject/forward ports removed

The old per-source TCP `inject_port`, `forward_port`, and `forward_ports` config fields have been removed. Use the single control WebSocket endpoint (`/api/v1/control`) instead. All automation (log injection, subscription/forwarding, TX, markers) goes through one connection, routed by configured source name.

## Documentation

- [Getting up to speed](docs/getting-up-to-speed.md)
- [Quick start](docs/quickstart.md)
- [Architecture](docs/architecture.md)
- [Configuration](docs/configuration.md)
- [CLI reference](docs/cli.md)
- [Development](docs/development.md)
- [Terminal UI](docs/tui.md)
- [Releasing](docs/releasing.md)

## Repository layout

```text
crates/embed-log-core/     Shared runtime, config, sources, parsers, HTTP/WS, sessions
crates/embed-log-cli/      CLI binary named `embed-log`
crates/embed-log-tui/      Terminal UI client and integrated TUI support
frontend/                  Live/static viewer UI, embedded into release binaries
sdk/python/                Python SDK, watcher, examples
config-samples/            Example YAML configs (no legacy fields)
scripts/                   Release packaging helpers
docs/                      Current docs
justfile                   Common development/release commands
```
