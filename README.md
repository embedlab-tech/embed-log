# embed-log

`embed-log` collects UART, UDP, file-tail, and mock network-capture logs, stores them as session artifacts, and serves a browser/Tauri UI for live viewing and static HTML exports.

The current workspace contains:

- `embed-log` CLI: run the log server, inspect sessions, export/merge logs.
- `embed-log-tauri` desktop app: wraps the same server in a Tauri shell with onboarding helpers.
- `embed-log-core`: shared config, sources, parsers, runtime, HTTP/WebSocket server, and session export logic.
- `frontend/`: browser UI assets embedded into release binaries.

## Install

macOS/Linux latest release:

```bash
curl -fsSL https://github.com/krezolekcoder/embed-log/releases/latest/download/install.sh | sh
```

Windows PowerShell latest release:

```powershell
irm https://github.com/krezolekcoder/embed-log/releases/latest/download/install.ps1 | iex
```

Release binaries include embedded frontend assets, so users do **not** need Rust, Cargo, or a separate `frontend/` directory.

See [docs/releasing.md](docs/releasing.md) for release and installer details.

## Quick start from source

```bash
cargo build --workspace
just demo-headless
```

Then open:

```text
http://127.0.0.1:8080/
```

Generate a starter config:

```bash
cargo run --package embed-log-cli --bin embed-log -- init --output embed-log.yml
```

Run with a config:

```bash
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

## Migration from legacy inject/forward ports

The old per-source `inject_port`, `forward_port`, and `forward_ports` fields are deprecated. Use the single control WebSocket endpoint (`/api/v1/control`) instead. All automation (log injection, forwarding, TX) goes through one connection, routed by configured source name. Legacy fields still parse but produce a deprecation warning.

## Documentation

- [Architecture](docs/architecture.md)
- [Configuration](docs/configuration.md)
- [CLI reference](docs/cli.md)
- [Development](docs/development.md)
- [Tauri desktop app](docs/tauri.md)
- [Releasing](docs/releasing.md)

## Repository layout

```text
crates/embed-log-core/     Shared runtime, config, sources, parsers, HTTP/WS, sessions
crates/embed-log-cli/      CLI binary named `embed-log`
crates/embed-log-tauri/    Tauri desktop binary
frontend/                  Live/static viewer UI, embedded into release binaries
sdk/python/                Python SDK, watcher, examples
config-samples/            Example YAML configs (no legacy fields)
scripts/                   Release packaging helpers
docs/                      Current docs
justfile                   Common development/release commands
```
