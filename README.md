# embed-log

Capture UART, UDP, and file-tail logs in a browser or terminal UI. Sessions are saved locally and can be exported as self-contained HTML.

## Install

macOS/Linux:

```bash
curl -fsSL https://github.com/embedlab-tech/embed-log/releases/latest/download/install.sh | sh
```

Windows PowerShell:

```powershell
irm https://github.com/embedlab-tech/embed-log/releases/latest/download/install.ps1 | iex
```

Release binaries include the web UI; Rust and a separate frontend checkout are not required.

Update an installation made by an official installer:

```bash
embed-log update --check
embed-log update
```

The CLI works offline. Background update checks are best-effort and can be disabled with `EMBED_LOG_NO_UPDATE_CHECK=1`.

## Run

Open one serial device:

```bash
embed-log run /dev/ttyUSB0 --baud 115200
```

Run a saved configuration:

```bash
embed-log doctor --config embed-log.yml
embed-log run --config embed-log.yml
```

The web server and browser UI start automatically. Use `--no-open-browser` for headless use or `--tui` for the terminal UI instead. Sessions are written under `./logs/` by default.

## Agent use

The binary contains the version-matched agent skill:

```bash
embed-log skill
```

It directs agents to use the CLI only: discover sources with `doctor`, inspect bounded session evidence with `summary`, `read`, `search`, and `around`, then act through the daemon. Do not open configured UARTs or read session files directly.

For a persistent capture, start a named daemon and read only new records by cursor:

```bash
embed-log run --daemon --instance bench-a --config embed-log.yml
embed-log sessions summary latest --dir ./logs
embed-log sessions read latest --dir ./logs --after "$CURSOR" --limit 100
embed-log tx --instance bench-a --source DUT_UART --line "status"
```

Evidence is concise text (`+time seq=N src=SOURCE#INDEX | message`). Set `CURSOR` to the final returned sequence number and use `sessions around` for context. Run `embed-log schema` when an agent needs the machine-readable CLI contract.

Ask your coding agent to create its project skill from the output of `embed-log skill`; this keeps its instructions matched to the installed CLI version.

## More documentation

- [Quick start](docs/quickstart.md)
- [Configuration](docs/configuration.md)
- [CLI reference](docs/cli.md)
- [Terminal UI](docs/tui.md)
- [Development](docs/development.md)
- [Releasing](docs/releasing.md)
