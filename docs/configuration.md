# Configuration

`embed-log` uses YAML configuration version `2`.

Config path resolution:

1. CLI `--config` / `-c`
2. `EMBED_LOG_CONFIG_YML_PATH`
3. `embed-log.yml` in the current directory

Version 1 files remain readable during migration, but generated configurations and maintained examples use version 2.

## Minimal example

```yaml
version: 2
server:
  listen: 127.0.0.1:18080
logs:
  dir: logs/
sources:
  DUT:
    label: Device
    type: udp
    port: 6000
ui:
  tabs:
    - title: Device
      sources: [DUT]
```

Run it and send a test line:

```bash
embed-log run --config embed-log.yml
echo 'boot complete' | nc -u -w0 127.0.0.1 6000
```

The server port hosts browser HTTP, browser WebSocket, the status API, and the control WebSocket. UDP source ports are independent and always explicit.

## Top-level keys

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `version` | integer | required | Canonical format is `2`. |
| `server` | object | see below | HTTP/WebSocket settings. |
| `logs` | object | `{ dir: "logs/" }` | Session root; relative to the config file. |
| `sources` | mapping | `{}` | Source ID to source definition. |
| `ui` | object | generated | Optional tabs; one tab per source when omitted. |
| `merges` | array | `[]` | Optional presentation-only virtual merged streams; never persisted as duplicate records. |

Unknown version 2 fields are rejected rather than silently ignored. Runtime choices such as browser, TUI, and daemon mode belong to CLI flags.

## Server

```yaml
server:
  listen: 127.0.0.1:18080
  app_name: embed-log
  timestamp_mode: absolute
  verbosity: events
  queue_size: 20000
  control_api: true
```

`listen` defaults to `127.0.0.1:18080`. Override it for one run with `--host` and `--port`.

The control endpoint is:

```text
ws://127.0.0.1:18080/api/v1/control
```

It supports source discovery, subscriptions, structured injection, UART TX, and marker creation.

## Logs

```yaml
logs:
  dir: logs/
```

Relative paths resolve against the configuration directory. Every session contains per-source logs and a cross-source `combined.jsonl` stream.

## Sources

Source IDs are mapping keys. `label` and `parser` are optional.

### UDP text

```yaml
sources:
  HOST:
    type: udp
    port: 16000
```

UDP binds on `0.0.0.0:<port>`. There is no default UDP source port.

### UART

```yaml
sources:
  DUT:
    label: Main device
    type: uart
    path: /dev/ttyUSB0
    baud: 115200
```

UART baud is source-local; there is no global baud setting. It defaults to `115200` when omitted.

### UART SLIP/CoAP

```yaml
sources:
  LINK:
    type: uart
    path: /dev/ttyUSB1
    baud: 921600
    parser:
      type: slip-coap
```

`slip-coap` is valid only for UART sources.

### Textual hexadecimal CoAP

Use `hex-coap` when a UART, UDP, or file source emits newline-delimited logs containing a CoAP packet as compact or separated hexadecimal text:

```yaml
sources:
  RADIO:
    type: uart
    path: /dev/ttyUSB1
    parser:
      type: hex-coap
```

For a line such as `radio rx: 40 01 12 34 b3 66 6f 6f 03 62 61 72`, the parser retains `radio rx: ` and replaces the hexadecimal packet from the first valid CoAP header with a readable decode containing type, method/status, message ID, token, options, and payload length. Lines without a valid CoAP packet pass through unchanged.

### Zephyr dictionary logging

```yaml
sources:
  DUT:
    type: uart
    path: /dev/ttyUSB0
    baud: 115200
    parser:
      type: zephyr-dict
      database: build/zephyr/log_dictionary.json
```

The database path resolves relative to the configuration file and must match the firmware build that produced the logs.

### File tail

```yaml
sources:
  TEST:
    type: file
    path: ./pytest.log
```

The source creates the file if missing, starts at its current end, and emits appended lines.

Supported backend parser types are `text`, `hex-coap`, `slip-coap`, and `zephyr-dict`.

## UI layout

Without `ui`, Embed-log creates one tab per source. Put two source IDs in one tab to show their panes side by side in the browser:

```yaml
sources:
  DUT_UART:
    label: DUT UART
    type: uart
    path: /dev/ttyUSB0
    baud: 115200
  HOST_UART:
    label: Host UART
    type: uart
    path: /dev/ttyUSB1
    baud: 115200
ui:
  tabs:
    - title: Device
      sources: [DUT_UART, HOST_UART]
```

Version 2 intentionally has no frontend plugin or per-pane plugin fields. The former config-v1 built-in `hex-coap` frontend plugin was removed; configurations using it fail with guidance to attach backend `parser.type: hex-coap` to the source. Explicit custom config-v1 plugins remain compatibility-only.

## Merged streams

Merges retain the list form because they describe virtual sources:

```yaml
merges:
  - name: LINK
    label: Link conversation
    of: [LINK_TX, LINK_RX]

ui:
  tabs:
    - title: Link
      sources: [LINK]
```

A merge must reference at least two distinct existing sources and must not collide with a source ID. Merges are presentation-only virtual sources: Embed-log persists and sequences each member record exactly once, stores the merge definition in `manifest.json`, and constructs the merged pane dynamically in the browser, TUI, and HTML export. No merge `.log` file or duplicate `combined.jsonl` record is created.

## Version 1 migration

| Version 1 | Version 2 |
| --- | --- |
| `server.host` + `server.ws_port` | `server.listen: HOST:PORT` |
| global `baudrate` | per-UART `baud` |
| `sources: [{ name, ... }]` | `sources: { NAME: {...} }` |
| UART/file `port` | `path` |
| source `baudrate` | `baud` |
| `tabs[].label` | `ui.tabs[].title` |
| `tabs[].panes` | `ui.tabs[].sources` |
| frontend/pane plugins | omitted; protocol decoding moves to the backend |

Use `embed-log doctor --config embed-log.yml --json` to validate and inspect the resolved configuration, physical sources, endpoint, logs directory, and UART accessibility.
