# Quick start

Embed-log can collect UART and appended-file logs without creating YAML. It starts the normal server, UI, and session recorder; only the source configuration is temporary.

## One UART in the web UI

```bash
embed-log run /dev/ttyUSB0
```

The browser UI opens automatically. Add `--no-open-browser` when running headless.

## Terminal UI

```bash
embed-log run /dev/ttyUSB0 --tui
```

## Multiple sources

```bash
embed-log run /dev/ttyUSB0 /dev/ttyUSB1 --baud 115200
embed-log run -s /dev/ttyUSB0 -s /dev/ttyUSB1 -f ./device.log --tui
```

Positional paths and `-s` / `--serial` add UART sources. `-f` / `--file` watches appended files. `--baud` applies to every UART source in this quick run.

Each quick-run source gets its own tab. Save a generated configuration and customize tabs/panes when you need a side-by-side layout:

```bash
embed-log run /dev/ttyUSB0 /dev/ttyUSB1 --save-config embed-log.yml
```

## Sessions

Quick runs write the same session artifacts as YAML-based runs. By default they are saved under `./logs/`; choose another location with `--log-dir`:

```bash
embed-log run /dev/ttyUSB0 --log-dir ./captures
```

Each session includes source logs, `combined.jsonl`, a manifest, markers/events, and a self-contained `session.html` export when the run exits.

## When to use YAML

Use `embed-log onboard` or a saved YAML configuration for per-source parsers, different baud rates, event rules, plugins, merges, and custom layouts.
