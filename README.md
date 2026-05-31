# embed-log
[embed-lab](https://embedlab.tech/) · Log aggregation for embedded development and CI

`embed-log` is a configurable log aggregation server for embedded development and CI.

It reads logs from UART and UDP sources, stores them in per-session artifacts, and streams them live to a browser UI.
## Get up to speed

Read these in order:
- `AGENTS.md` — fast repo orientation for humans and coding agents
- `docs/ARCHITECTURE.md` — end-to-end system flow
- `docs/BACKEND.md` / `docs/FRONTEND.md` — subsystem details
- `docs/TESTING.md` — test strategy and commands

## Quick install

One command, no clone needed — installs the **latest tagged release**:

```bash
curl -fsSL https://raw.githubusercontent.com/krezolekcoder/embed-log/main/install.sh | bash
```

Windows (PowerShell 7+):

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass; iex ((New-Object System.Net.WebClient).DownloadString('https://raw.githubusercontent.com/krezolekcoder/embed-log/main/install.ps1'))
```

Install a specific version:

```bash
EMBED_LOG_REF_TYPE=release EMBED_LOG_REF=v1.0.2 curl -fsSL https://raw.githubusercontent.com/krezolekcoder/embed-log/main/install.sh | bash
```

Install the **latest** `main` branch — clone and run the installer locally:

```bash
git clone https://github.com/krezolekcoder/embed-log.git
cd embed-log
./install.sh
```

After install, `embed-log` is available globally (no venv activation needed):

```bash
embed-log create-config
embed-log run --config embed-log.yml
```

Uninstall:

```bash
curl -fsSL https://raw.githubusercontent.com/krezolekcoder/embed-log/main/uninstall.sh | bash
```

### Fonts

The UI defaults to **JetBrains Mono** (a Nerd Font). If it's not installed,
the browser falls back through:
`ui-monospace` → `SFMono-Regular` (macOS) → `Menlo` → `Monaco`
→ `Consolas` (Windows) → `'Courier New'` → `monospace`

Windows (PowerShell 7+):

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass; iex ((New-Object System.Net.WebClient).DownloadString('https://raw.githubusercontent.com/krezolekcoder/embed-log/main/uninstall.ps1'))
```

### Developer setup

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
pip install -e .
```

## Run with a config file

Validate first:

```bash
embed-log validate --config embed-log.yml
```

Run:
```bash
embed-log run --config embed-log.yml
# or override timestamp mode from the CLI
embed-log run --config embed-log.yml --timestamp-mode relative
```

UI default:

```text
http://127.0.0.1:8080/
```

## Configuration

See the `config-samples/` directory for ready-to-use examples:

| File | What it shows |
|---|---|
| `single-tab-dual-pane.yml` | Two UART sources side-by-side |
| `multi-tab-multi-baud.yml` | Two tabs with different UART baudrates + UDP |
| `udp-cbor-datagram.yml` | UDP sources with CBOR datagram parser |
| `annotated-full-config.yml` | All options documented inline |

### Timestamp modes

- `absolute` — wall-clock timestamps like `05-29 12:42:47.123`
- `relative` — elapsed time from the first log line like `T+00:00:01.234`

In the UI you can switch between absolute and relative time when the session carries the required origin metadata. Exported HTML snapshots embed that metadata too, so the same toggle works offline.

### Source types

| Type | Syntax | Parser options |
|---|---|---|
| `uart` | Serial port path | `text` (default), `cbor-datagram` |
| `udp` | UDP port number | `text` (default), `cbor-datagram` |

Add a `parser:` block to use structured decoding:

```yaml
sources:
  - name: TELEMETRY
    type: udp
    port: 6001
    parser:
      type: cbor-datagram
```

## CLI reference

| Command | Description |
|---|---|
| `run` | Start the log server from a config file |
| `demo` | Start a local demo with simulated traffic (no hardware needed) |
| `merge` | Merge raw log files into a standalone static HTML report |
| `parse` | Extract raw log files from an exported session HTML |
| `tail-file` | Tail a file and forward lines to a UDP source |
| `version` | Show version and environment information |
| `ports` | List detected serial ports |
| `update` | Update embed-log to a new version |
| `sessions` | List, inspect, and export session artifacts |

### `run`

```bash
embed-log run --config embed-log.yml
```

See `config-samples/` for example config files.

### `demo`

Start a local demo with simulated traffic — useful for testing the UI without real hardware. Uses the bundled `embed-log.demo.yml` config.

```bash
embed-log demo
embed-log demo --profile test
embed-log demo --profile random --fast --no-browser
```

Profiles: `random` (interactive, default), `test` / `deterministic` (for UI tests).

### `merge`

Take recorded log files and produce a portable static HTML — useful in CI for archiving test runs:

```bash
embed-log merge --tab "My Report" SENSOR_A sensor.log --output report.html
```

Each `--tab` takes a label, a pane name, and a log file path. Repeat `--tab` for multiple tabs.

### `parse`

Extract raw log files from a previously exported session HTML:

```bash
embed-log parse session.html
embed-log parse session.html --output my-session
```

### `tail-file`

Forward log lines from an existing file into a running embed-log UDP source. Useful for integrating file-based loggers:

```bash
embed-log tail-file app.log 127.0.0.1:6000
embed-log tail-file app.log 127.0.0.1:6000 --from-start
```

### `version`

```bash
embed-log version
embed-log version --json
```

### `ports`

```bash
embed-log ports
embed-log ports --json
```

### `update`

```bash
embed-log update                # update to latest release
embed-log update --tag v1.0.1   # specific tag
embed-log update --branch main  # specific branch
```

### `sessions`

```bash
embed-log sessions list
embed-log sessions list --search build-123
embed-log sessions list --with-markers --app demo
embed-log sessions info <session-id>
embed-log sessions logs <session-id> --grep "timeout"
embed-log sessions logs <session-id> --pane SENSOR_A --grep "error" --tail 20
embed-log sessions export <session-id>
embed-log sessions marker list <session-id> --search boot
```

Full documentation with all flags and examples is at `backend/skills/sessions.md` (also available via `read skill://sessions`).


## Testing

Backend tests:

```bash
python3 -m unittest discover -s tests -v
```

UI tests:

```bash
cd tests-ui
npm test
```

## More docs

See `docs/README.md` for the curated documentation index.
