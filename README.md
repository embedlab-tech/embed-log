# embed-log

`embed-log` is a configurable log aggregation server for embedded development and CI.

It reads logs from UART and UDP sources, stores them in per-session artifacts, and streams them live to a browser UI.

## Get up to speed

Read these in order:
- `AGENTS.md` — fast repo orientation for humans and coding agents
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
embed-log init
embed-log run --config embed-log.yml
```

### Developer setup

From a cloned repository:

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
```

## Run with a config file

Validate first:

```bash
embed-log validate --config embed-log.yml
```

Run:

```bash
embed-log run --config embed-log.yml
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

logs:
  dir: logs/

baudrate: 115200

sources:
  - name: FTDI_A
    type: uart
    port: /dev/ttyFTDI_A
    inject_port: 5001

  - name: FTDI_B
    type: uart
    port: /dev/ttyFTDI_B
    inject_port: 5002

  - name: PYTEST
    type: udp
    port: 6000
    inject_port: 5003

tabs:
  - label: Devices
    panes: [FTDI_A, FTDI_B]

  - label: Pytest
    panes: [PYTEST]
```

## Useful commands

```bash
# create starter config
embed-log init --output embed-log.yml

# validate config
embed-log validate --config embed-log.yml

# run app
embed-log run --config embed-log.yml

# run bundled demo
./run_demo.sh --no-browser
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
