# Getting up to speed with embed-log

Embed-log collects embedded-device logs, presents them live in a browser or terminal UI, and keeps each run as a portable session. Start with a UART in seconds; use YAML when the capture becomes part of a repeatable workflow.

## 1. Install and verify

Install a released binary:

```bash
curl -fsSL https://github.com/krezolekcoder/embed-log/releases/latest/download/install.sh | sh
embed-log version
embed-log doctor
```

`doctor` shows system/config/packet-capture readiness. Check a serial device without opening or reconfiguring it:

```bash
embed-log doctor --serial /dev/ttyUSB0
embed-log ports
```

## 2. Capture one device immediately

No YAML is required for the common case:

```bash
embed-log run /dev/ttyUSB0
```

This opens the browser UI and saves a normal session under `./logs/`. Change baud rate or session location when needed:

```bash
embed-log run /dev/ttyUSB0 --baud 9600 --log-dir ./captures
```

Use the terminal UI for SSH or terminal-first work:

```bash
embed-log run /dev/ttyUSB0 --tui
```

Quick-run source layout is intentionally simple: each source gets its own tab.

```bash
embed-log run /dev/ttyUSB0 /dev/ttyUSB1
embed-log run -s /dev/ttyUSB0 -s /dev/ttyUSB1 -f ./host.log --tui
```

- positional paths and `-s` / `--serial` add UART sources;
- `-f` / `--file` follows appended text files;
- `--baud` applies to every quick-run UART;
- `--no-open-browser` is useful for headless browser-server runs.

Persist a generated starting point once the capture needs customization:

```bash
embed-log run /dev/ttyUSB0 /dev/ttyUSB1 --save-config embed-log.yml
```

## 3. Move to a repeatable YAML capture

Use a config for custom tabs, two-pane layouts, different source baud rates, parsers, events, merges, plugins, UDP, and packet capture.

```bash
embed-log onboard                 # browser setup flow
embed-log init --output embed-log.yml
embed-log validate --config embed-log.yml
embed-log run --config embed-log.yml
```

A minimal UART configuration:

```yaml
logs:
  dir: logs/

sources:
  - name: DUT
    label: Device
    type: uart
    port: /dev/ttyUSB0
    baudrate: 115200

tabs:
  - label: Device
    panes: [DUT]
```

Read [Configuration](configuration.md) for the complete schema and [Architecture](architecture.md) for runtime behavior.

## 4. Work with the live UI

Both UIs use the same server/session pipeline.

### Browser UI

The browser is the full viewer: tabs/panes, filters, UART TX, markers, events, static HTML export, and browser plugins.

### Terminal UI

```bash
embed-log run --config embed-log.yml --tui
embed-log run /dev/ttyUSB0 --tui
```

Useful keys:

- `:` or `i`: send UART text from a writable pane;
- `/`: regex filter the active pane; empty filter clears it;
- `m`: toggle a marker; `[` and `]`: navigate markers;
- `e`: events tab; `x`: export session HTML;
- `?`: built-in keybinding help.

The TUI does not execute browser JavaScript plugins. See [Terminal UI](tui.md).

## 5. Understand sessions

Every run creates a session directory containing structured data and human-shareable artifacts. Typical files include:

```text
logs/<session-id>/
├── manifest.json
├── combined.jsonl
├── events.jsonl
├── markers.json
├── session.html
└── per-source log files
```

Use `latest` wherever a session ID is accepted:

```bash
embed-log sessions list
embed-log sessions info latest
embed-log sessions summary latest
embed-log sessions search --contains panic
embed-log sessions open latest
```

`open` generates `session.html` if needed, then opens it in the default browser.

### Export and share

```bash
embed-log sessions export latest --format html --output report.html
embed-log sessions export latest --format raw --output merged.log
embed-log sessions bundle latest --output support.tar.gz
```

A support bundle includes the complete session directory plus `embed-log-version.json` diagnostics, making it suitable for bug reports and offline handoff.

### Import external logs into a session

Merge a non-embed-log file into the session timeline when each non-empty line begins with RFC3339 time:

```text
2026-07-11T11:21:47.123Z pytest started
[2026-07-11T11:21:48+00:00] assertion passed
```

```bash
embed-log sessions import latest ./pytest.log --source PYTEST --dry-run
embed-log sessions import latest ./pytest.log --source PYTEST
```

The import adds a source/tab, stores the original file, and merges records into `combined.jsonl` in timestamp order. Use `--dry-run` before modifying a valuable session.

### Retain disk space

```bash
embed-log sessions prune --dir logs --keep 20 --dry-run
embed-log sessions prune --dir logs --keep 20
```

Always run the dry-run first. It lists sessions affected and reports the bytes that would be reclaimed.

## 6. Automate embed-log

The control WebSocket provides source-aware log subscription, injection, UART TX, and markers:

```text
ws://127.0.0.1:8080/api/v1/control
```

The Python SDK can inject test output, issue UART commands, subscribe to entries, and run regex watchers. Example:

```python
from embed_log_sdk import EmbedLogClient

with EmbedLogClient.from_config("embed-log.yml", origin="pytest") as client:
    client.tx_write("DUT", "version\r\n")
    client.inject_log("DUT", "test: passed", color="cyan")
```

See the [README](../README.md#python-sdk), `sdk/python/`, and [CLI reference](cli.md) for commands suited to CI and agents.

## 7. Use advanced sources and parsers deliberately

- **UART:** default choice for device console and shell traffic.
- **File:** follow logs produced by host tools/tests.
- **UDP:** receive application datagrams.
- **Network capture:** optional pcap-backed UDP capture; requires a pcap-enabled build and platform permissions.
- **Parsers:** text, CBOR datagram, SLIP/CoAP, and Zephyr dictionary logging.

Validate configs before lab runs:

```bash
embed-log validate --config embed-log.yml
embed-log doctor --config embed-log.yml
```

For real pcap capture, follow the dependency/permission instructions in [Configuration](configuration.md).

## 8. Update safely

```bash
embed-log update --check
embed-log update --yes
```

The updater selects the platform archive, verifies it against release `SHA256SUMS`, stages it, and retains a rollback backup during replacement. Use package-manager updates for package-managed installations. See [CLI reference](cli.md).

## 9. Suggested team workflow

1. Start with quick-run while bringing up hardware.
2. Save YAML once source names/layout/parser choices stabilize.
3. Keep logs in a project-relative directory or CI artifact path.
4. Mark failures/events while live.
5. Import pytest/host/external RFC3339 logs into the same session.
6. Share `session.html` for lightweight review or a support bundle for diagnosis.
7. Prune old sessions after preserving releases/incidents.

## Reference map

- [Quick start](quickstart.md) — shortest no-YAML path.
- [Configuration](configuration.md) — complete YAML model.
- [CLI reference](cli.md) — every command and output format.
- [Terminal UI](tui.md) — TUI workflow and keybindings.
- [Architecture](architecture.md) — core/server/session design.
- [Releasing](releasing.md) — release artifacts and installer process.
- [Non-session roadmap](non-session-roadmap.md) — deferred distribution, UI, and parser work.
