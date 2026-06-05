# embed-log

[embed-lab](https://embedlab.tech/) · live log aggregation for embedded development and CI.

`embed-log` reads logs from UART, UDP, files, and simplified network packet captures, stores each run as a session, and shows the logs live in a browser UI.

## Install / uninstall

### macOS / Linux

Install latest release:

```bash
curl -fsSL https://raw.githubusercontent.com/krezolekcoder/embed-log/main/install.sh | bash
```

Uninstall:

```bash
curl -fsSL https://raw.githubusercontent.com/krezolekcoder/embed-log/main/uninstall.sh | bash
```

### Windows PowerShell 7+

Install latest release:

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass; iex ((New-Object System.Net.WebClient).DownloadString('https://raw.githubusercontent.com/krezolekcoder/embed-log/main/install.ps1'))
```

Uninstall:

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass; iex ((New-Object System.Net.WebClient).DownloadString('https://raw.githubusercontent.com/krezolekcoder/embed-log/main/uninstall.ps1'))
```

## Fastest start: real logs on your desk

Find your serial ports:

```bash
embed-log ports
```

Typical Linux ports look like `/dev/ttyUSB0` or `/dev/ttyACM0`.
Typical macOS ports look like `/dev/cu.usbserial-*` or `/dev/cu.usbmodem*`.
Typical Windows ports look like `COM3`, `COM4`, or another `COM<number>`.

Create a config file:

```bash
embed-log sample-config --sample single-tab-dual-pane.yml --output embed-log.yml
```

Edit the `port:` values in `embed-log.yml` to match your devices.

### Example 1: two UART devices

```yaml
version: 1

server:
  host: 127.0.0.1
  ws_port: 8080
  app_name: embed-log
  open_browser: true
  timestamp_mode: absolute

logs:
  dir: logs/

baudrate: 115200

sources:
  - name: DUT
    type: uart
    port: /dev/ttyUSB0

  - name: AUX
    type: uart
    port: /dev/ttyUSB1

tabs:
  - label: Devices
    panes: [DUT, AUX]
```

### Example 2: one UART plus one UDP source

This is useful when a test runner, for example `PYTEST`, sends logs over UDP.

```yaml
version: 1

server:
  host: 127.0.0.1
  ws_port: 8080
  app_name: embed-log
  open_browser: true
  timestamp_mode: absolute

logs:
  dir: logs/

baudrate: 115200

sources:
  - name: DUT
    type: uart
    port: /dev/ttyUSB0

  - name: PYTEST
    type: udp
    port: 6000

tabs:
  - label: Desk
    panes: [DUT, PYTEST]
```

Check which config is active, then run:

```bash
embed-log doctor --config embed-log.yml
embed-log run --config embed-log.yml
```

`embed-log doctor` prints the effective config source: explicit `--config`, `EMBED_LOG_CONFIG_YML_PATH`, or the default `embed-log.yml` in the current directory. Use it when you are not sure which config `embed-log run` will load.

Or point `embed-log` at your config once and run it without flags:

```bash
export EMBED_LOG_CONFIG_YML_PATH="$PWD/embed-log.yml"
embed-log run
```

Windows PowerShell:

```powershell
$env:EMBED_LOG_CONFIG_YML_PATH = "C:\path\to\embed-log.yml"
embed-log run
```

Open:

```text
http://127.0.0.1:8080/
```

Stop the server with `Ctrl+C`.

## Demo without hardware

If you only want to see the UI, run simulated traffic:

```bash
embed-log demo
```

## Ready-made config examples

Write any example into `embed-log.yml`:

```bash
embed-log sample-config --sample single-tab-dual-pane.yml --output embed-log.yml
```

Useful starter files:

| Example | Use when |
|---|---|
| `single-tab-dual-pane.yml` | You have two UART devices and want them side-by-side |
| `multi-tab-multi-baud.yml` | You have UART devices with different baudrates plus a UDP source |
| `three-tab-uart-file-udp-coap.yml` | You want UART, file tailing, and UDP in one UI |
| `network-capture.yml` | You want simplified network packet captures in the UI |
| `annotated-full-config.yml` | You want every config option documented inline |

The same files are also in `config-samples/` in this repo.

## Common commands

```bash
embed-log demo                         # run simulated traffic
embed-log ports                        # list serial ports
embed-log sample-config --output embed-log.yml
embed-log doctor --config embed-log.yml
embed-log run --config embed-log.yml
export EMBED_LOG_CONFIG_YML_PATH="$PWD/embed-log.yml"
embed-log run                            # uses EMBED_LOG_CONFIG_YML_PATH
embed-log version
```

## Sessions and exported reports

Every run is saved under the configured `logs.dir` as a session. The UI can export a portable `session.html` report.

CLI session commands:

```bash
embed-log sessions list
embed-log sessions info <session-id>
embed-log sessions export <session-id>
```

Merge existing raw log files into a standalone HTML report:

```bash
embed-log merge --tab "My Report" SENSOR_A sensor.log --output report.html
```

## Install from source

Use this only if you want the latest `main` branch or local development.

```bash
git clone https://github.com/krezolekcoder/embed-log.git
cd embed-log
./install.sh
```

Developer setup:

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
pip install -e .
```

## More documentation

- `docs/README.md` — documentation index
- `docs/ARCHITECTURE.md` — end-to-end system flow
- `docs/BACKEND.md` / `docs/FRONTEND.md` — subsystem details
- `docs/TESTING.md` — test strategy and commands

## Testing this repo

Backend tests:

```bash
python3 -m unittest discover -s tests -v
```

UI tests:

```bash
cd tests-ui
npm test
```