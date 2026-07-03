# CLI reference

The CLI binary is named `embed-log`.

```bash
embed-log --help
```

Global options:

| Option | Meaning |
| --- | --- |
| `-c, --config <PATH>` | Config file. Falls back to `EMBED_LOG_CONFIG_YML_PATH`, then `embed-log.yml`. |
| `--frontend-dir <PATH>` | Filesystem frontend directory for development. Defaults to `frontend`. Release binaries can use embedded assets. |
| `--tui` | Launch the terminal UI instead of the browser UI. |
| `--ui` | Launch the beta Tauri desktop UI instead of the browser UI. |
| `--no-open-browser` | Do not open the default browser. |

## Run server

Default command when no subcommand is given:

```bash
embed-log --config embed-log.yml
```

Explicit form:

```bash
embed-log run --config embed-log.yml
```

Headless/no browser:

```bash
embed-log run --config embed-log.yml --no-open-browser
```

Current behavior:

- if no config exists at the resolved path, automatically runs **onboarding** first (see [Onboarding](#onboarding)), then starts `LogServer` from the generated config
- starts `LogServer`
- serves UI/API on `server.host:server.ws_port`
- opens the browser unless `--no-open-browser` is passed (skipped when onboarding ran, since the onboarding page redirects to the live server)
- writes session artifacts under `logs.dir`
- exports `session.html` on Ctrl-C shutdown

Useful runtime overrides:

```bash
embed-log run --config embed-log.yml --host 0.0.0.0 --ws-port 9090 --log-dir /tmp/embed-log-runs
```

`--host` and `--ws-port` override `server.host` / `server.ws_port` in memory. `--log-dir` overrides `logs.dir` and is resolved relative to the current working directory.

## Onboarding

`embed-log` and the Tauri desktop app share the **same** first-run onboarding page (`frontend/onboarding.js`) and the same onboarding HTTP server (`embed_log_core::onboarding::OnboardingServer`). There is no separate web UI for setup.

Onboarding runs automatically when `embed-log run` (or the default command) finds no config file. You can also trigger it explicitly:

```bash
embed-log onboard
```

or writing to a specific path:

```bash
embed-log onboard --config ~/projects/lab-a/embed-log.yml
```

What happens:

1. a small setup server starts on a random localhost port
2. your browser opens the setup page (unless `--no-open-browser`)
3. you pick sources, tabs, parser, and logs directory
4. on **Start logging**, the config is written to the resolved path, validated, and the CLI transitions to the real `LogServer` on the configured `ws_port`

The setup server exposes the same HTTP endpoints used by the page in both browser and Tauri mode:

| Endpoint | Purpose |
| --- | --- |
| `GET /` | the onboarding page |
| `GET /api/serial_ports` | discovered serial ports |
| `GET /api/server_status` | resolved config path + ws port |
| `POST /api/save_config` | persist the draft config |


## Desktop UI

```bash
embed-log --ui --config embed-log.yml
```

The CLI tries to launch the Tauri app directly or through Cargo during development. `EMBED_LOG_TAURI_BIN` can point at a specific Tauri binary.

## Validate config

```bash
embed-log validate --config embed-log.yml
embed-log validate --config embed-log.yml --json
```

Loads the config, runs validation, and prints the resolved server/log/source/tab summary. For packet-capture configs, follow with `embed-log doctor --config <file>` to check the native pcap dependency.

## Init config

```bash
embed-log init --output embed-log.yml
```

Writes the embedded demo config. Edit it before using with real devices.

## Demo

```bash
embed-log demo
```

or without opening the browser:

```bash
embed-log demo --no-open-browser
```

The demo uses an embedded config unless `--config` is supplied. It prepares demo file sources, starts generated traffic, and runs the normal server.

## Diagnostics

Version:

```bash
embed-log version
embed-log version --json
embed-log version --config embed-log.yml
```

Doctor:

```bash
embed-log doctor
embed-log doctor --json
embed-log doctor --config embed-log.yml
```

`doctor` reports the binary version, host system info, config summary, and packet-capture readiness:
- which OS / architecture the binary is running on
- whether the binary was built with the `pcap-capture` feature
- whether the native packet-capture library is installed (`libpcap` on Unix-like systems, `Npcap`/`WinPcap` on Windows)
- whether the inspected config contains `network_capture` sources using `network_backend: pcap`

Serial ports:

```bash
embed-log ports
embed-log ports --json
```

## Sessions

List sessions:

```bash
embed-log sessions list --dir logs
embed-log sessions list --dir logs --limit 10
embed-log sessions list --dir logs --json
```

Show session manifest/info:

```bash
embed-log sessions info <SESSION_ID> --dir logs
embed-log sessions info <SESSION_ID> --dir logs --json
```

Export a recorded session:

```bash
embed-log sessions export <SESSION_ID> --dir logs --format html --output session.html
embed-log sessions export <SESSION_ID> --dir logs --format raw --output merged.txt
```

Read the session-wide combined JSONL stream:

```bash
embed-log sessions combined <SESSION_ID> --dir logs
embed-log sessions combined <SESSION_ID> --dir logs --lines 50
embed-log sessions tail-combined <SESSION_ID> --dir logs --follow
```

Read event-detection hits from a session:

```bash
embed-log sessions events <SESSION_ID> --dir logs
embed-log sessions events <SESSION_ID> --dir logs --severity fatal
embed-log sessions events <SESSION_ID> --dir logs --source DUT --contains watchdog
embed-log sessions events <SESSION_ID> --dir logs --json
```

Search across session combined streams:

```bash
embed-log sessions search --dir logs --source DUT
embed-log sessions search --dir logs --source DUT --from 2026-07-03T09:00:00 --to 2026-07-03T15:00:00
embed-log sessions search --dir logs --job nightly-42 --kind network_capture --dst-port 5683
embed-log sessions search --dir logs --contains panic --regex 'ERROR|WARN'
embed-log sessions search --dir logs --source DUT --count
```

`search` scans `combined.jsonl` files under the selected log directory and prints matching entries as JSONL. It can filter by session id/prefix, job id, source id, source kind, time window, message substring/regex, and packet fields such as source/destination UDP port or IP address.

Formats:

- `html`: self-contained viewer HTML
- `raw`: merged raw text output

## Merge raw logs into static HTML

```bash
embed-log merge \
  --tab Device DUT logs/dut.log HOST logs/host.log \
  --output merged.html
```

Pane labels can be supplied as `PANE_ID=Friendly Label`:

```bash
embed-log merge \
  --tab Device DUT='DUT Device' logs/dut.log \
  --output merged.html
```

Timestamp options:

```bash
embed-log merge \
  --tab Device DUT logs/dut.log \
  --timestamp-mode relative \
  --first-log-at 2026-06-14T09:00:00+02:00 \
  --output merged.html
```

## Parse exported HTML back to logs

```bash
embed-log parse session.html --output parsed/
```

Extracts embedded `logData` from a session HTML file and writes per-pane raw log files.

## Environment variables

| Variable | Used by | Meaning |
| --- | --- | --- |
| `EMBED_LOG_CONFIG_YML_PATH` | CLI/Tauri | Config path fallback. |
| `EMBED_LOG_TAURI_BIN` | CLI `--ui` | Explicit Tauri app binary path. |
| `EMBED_LOG_DEMO_TRAFFIC` | Tauri/dev | Enables generated demo traffic when starting the Tauri server. |
| `RUST_LOG` | tracing | Log filtering, e.g. `RUST_LOG=debug`. |
