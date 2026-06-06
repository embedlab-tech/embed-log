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

## Update an existing install

Use the installer scripts for first install. After `embed-log` is installed, update through the CLI:

```bash
embed-log update
embed-log update --sha <sha>
```

`embed-log update --sha <sha>` refuses commits older than the latest release unless you pass `--allow-rollback`.


## Quick start

**Step 1** — find serial ports:

```bash
embed-log ports
```

**Step 2** — generate a config (double UART + UDP by default):

```bash
embed-log init
```

This writes `embed-log.yml`. Edit the `port:` values to match your devices.

To start from a specific sample:

```bash
embed-log init --list                                    # see all samples
embed-log init --sample single_uart_single_tab           # pick one
```

**Step 3** — validate the config:

```bash
embed-log doctor --config embed-log.yml
```

**Step 4** — start the UI:

```bash
embed-log run --config embed-log.yml
```

Open `http://127.0.0.1:8080/`. Stop with `Ctrl+C`.

Or set the config once for this shell:

```bash
export EMBED_LOG_CONFIG_YML_PATH="$PWD/embed-log.yml"
embed-log run
```

Windows PowerShell:

```powershell
$env:EMBED_LOG_CONFIG_YML_PATH = "C:\path\to\embed-log.yml"
embed-log run
```


### UART TX autocomplete (optional)

embed-log can show TX command suggestions in the browser UI. Focus a UART input field and press `Tab` to cycle matching commands.

**Generate alongside a new config:**

```bash
embed-log init --add-uart-shell
```

This writes `embed-log.commands.yml` next to `embed-log.yml` with starter commands for every UART source.

**Generate for an existing config:**

```bash
embed-log init --config embed-log.yml --add-uart-shell
```

This generates only the commands file; the config is not modified.

`embed-log run` loads `<config-stem>.commands.yml` automatically when it is next to the config file. Edit the commands to match the shell your firmware actually supports.


### Demo without hardware

```bash
embed-log demo
```


## Config reference

### Example: two UART devices

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

### Example: one UART plus one UDP source

Useful when a test runner, for example `PYTEST`, sends logs over UDP.

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


## Ready-made config samples

List all samples:

```bash
embed-log init --list
```

Generate from a sample:

```bash
embed-log init --sample double_uart_udp_two_tabs --output embed-log.yml
```

| Sample | Use when |
|---|---|
| `single_uart_single_tab` | One UART source in one tab |
| `double_uart_single_tab` | Two UART panes side-by-side in one tab |
| `double_uart_udp_two_tabs` | Two UART panes plus one UDP/pytest tab |
| `double_uart_network_two_tabs` | Two UART panes plus a packet-capture network tab |
| `double_uart_udp_coap_two_tabs` | Two UART panes plus UDP panes using the CoAP plugin |
| `single_file_single_tab` | One file-tail source in one tab |
| `double_uart_file_two_tabs` | Two UART panes plus a file-tail log tab |
| `double_uart_minimal_single_tab` | Minimal two-UART single-tab layout |
| `double_uart_udp_multi_baud_two_tabs` | Two UARTs with different baudrates plus a UDP tab |
| `double_uart_file_udp_coap_three_tabs` | Two UARTs, file tailing, UDP, and CoAP across three tabs |
| `single_network_single_tab` | Simplified packet capture in one tab |
| `three_udp_cbor_two_tabs` | Two CBOR UDP sources plus one text UDP monitor |
| `reference_full_annotated` | Every config option documented inline |

The same files are in `config-samples/` in this repo.


## Agents / quick repo orientation

```bash
embed-log doctor
embed-log onboard --samples
embed-log init --list
```

`embed-log onboard --json` prints stable machine-readable orientation: version, install source, active config, samples, commands, docs, and next steps.


## Common commands

```bash
embed-log init                            # generate default config
embed-log init --list                     # list available samples
embed-log init --add-uart-shell           # config + TX command suggestions
embed-log init --config x.yml --add-uart-shell  # TX suggestions for existing config
embed-log doctor --config embed-log.yml   # validate config
embed-log run --config embed-log.yml      # start the UI
embed-log demo                            # simulated traffic, no hardware
embed-log ports                           # list serial ports
embed-log onboard                         # practical CLI orientation
embed-log sessions list                   # list saved sessions
embed-log update                          # install the latest release
embed-log update --sha <sha>              # install a specific commit
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