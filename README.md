# embed-log

`embed-log` is a configurable log aggregation server for embedded development and CI.

It reads logs from UART and UDP sources, stores them in per-session artifacts, and streams them live to a browser UI.
## Get up to speed

Read these in order:
- `AGENTS.md` — fast repo orientation for humans and coding agents
- `DEVELOPMENT.md` — working from the source tree
- `docs/ARCHITECTURE.md` — end-to-end system flow
- `docs/BACKEND.md` / `docs/FRONTEND.md` — subsystem details
- `docs/TESTING.md` — test strategy and commands

## Quick install

One command, no clone needed:

```bash
curl -fsSL https://raw.githubusercontent.com/krezolekcoder/embed-log/main/install.sh | bash
```

Windows (PowerShell):

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass; iex ((New-Object System.Net.WebClient).DownloadString('https://raw.githubusercontent.com/krezolekcoder/embed-log/main/install.ps1'))
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

Windows (PowerShell):
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

See `DEVELOPMENT.md` for the full development workflow — running from source, testing, debugging.

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

## Sample config

The UI layout supports 1 or 2 panes per tab. A three-source setup therefore uses two tabs below.

```yaml
version: 1

server:
  host: 127.0.0.1
  ws_port: 8080
  app_name: embed-log
  open_browser: false
  verbose: false
  timestamp_mode: absolute

logs:
  dir: logs/

baudrate: 115200

sources:
  - name: FTDI_A
    label: DUT
    type: uart
    port: /dev/ttyFTDI_A
    inject_port: 5001

  - name: FTDI_B
    label: AUX
    type: uart
    port: /dev/ttyFTDI_B
    inject_port: 5002

  - name: PYTEST
    label: PYTEST
    type: udp
    port: 6000
    inject_port: 5003

tabs:
  - label: Devices
    panes: [FTDI_A, FTDI_B]

  - label: Pytest
    panes: [PYTEST]
```

`timestamp_mode` values:
- `absolute` — wall-clock timestamps like `05-29 12:42:47.123`
- `relative` — elapsed time from the first log line like `T+00:00:01.234`

In the UI settings panel you can switch between absolute and relative time when the session carries the required origin metadata. Exported full HTML snapshots and exported HTML snippets embed that metadata too, so the same toggle works offline.

## Useful commands

```bash
# create starter config
embed-log create-config --output embed-log.yml

# validate config
embed-log validate --config embed-log.yml

# run app
embed-log run --config embed-log.yml

# run bundled demo
./run_demo.sh --no-browser

# deterministic fast demo for local UI testing
./run_demo.sh --profile test --fast --no-browser

# faster random demo traffic for manual testing
./run_demo.sh --profile random --fast --no-browser
```

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
