# Configuration

`embed-log` reads YAML config version `1`.

Config path resolution:

1. CLI `--config` / `-c`
2. `EMBED_LOG_CONFIG_YML_PATH`
3. `embed-log.yml` in the current directory

The Tauri app also falls back to its platform app config directory when no local config exists.

## Minimal UDP example

```yaml
version: 1

server:
  host: 127.0.0.1
  ws_port: 8080
  app_name: embed-log
  timestamp_mode: absolute

logs:
  dir: logs/

sources:
  - name: DUT
    label: Device
    type: udp
    port: 6000

tabs:
  - label: Device
    panes: [DUT]
```

Run:

```bash
embed-log run --config embed-log.yml
```

Send a test line:

```bash
echo 'boot complete' | nc -u -w0 127.0.0.1 6000
```

## Control API

Embed-log exposes a single structured JSON WebSocket endpoint for all automation:

```
ws://127.0.0.1:8080/api/v1/control
```

Use this instead of the legacy per-source inject/forward ports. The control API
provides:

- `hello` — discover sources, labels, types, and writability
- `subscribe` / `unsubscribe` — receive structured `log.entry` events by source
- `log.inject` — inject log entries (color, origin, source)
- `tx.write` — write bytes to writable (UART) sources
- `marker.create` — create UI markers with description and line index

See the [Python SDK](../sdk/python/README.md) or the [README](../README.md) for protocol details.

## Top-level keys

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `version` | integer | `1` | Only version `1` is supported. |
| `server` | object | see below | HTTP/WebSocket/UI settings. |
| `logs` | object | `{ dir: "logs/" }` | Session root directory. Relative paths resolve against the config file directory. |
| `baudrate` | integer | `115200` | Default UART baudrate. Source-level `baudrate` overrides it. |
| `sources` | array | `[]` | Input definitions. |
| `tabs` | array | `[]` | Viewer tab definitions. Practical configs should define tabs for each visible source. |
| `frontend_plugins` | map | `{}` | Plugin definitions loaded by the runtime and passed to the frontend. |

## `server`

```yaml
server:
  host: 127.0.0.1
  ws_port: 8080
  app_name: embed-log
  timestamp_mode: absolute
  job_id: optional-job-name
  default_light_theme: whitesand
  default_dark_theme: one-dark
  queue_size: 20000
  control_api: true
```

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `host` | string | `127.0.0.1` | Bind host for the HTTP/WebSocket server. |
| `ws_port` | integer | `8080` | HTTP/WebSocket port. |
| `ws_ui` | string? | `null` | Parsed for compatibility; current Rust runtime does not use it. |
| `app_name` | string | `embed-log` | Shown in UI/session metadata. |
| `open_browser` | bool | `false` | Parsed for compatibility. Current CLI opens by default unless `--no-open-browser` is used. |
| `verbosity` | string? | `null` | If set, must be `quiet`, `events`, or `full`. |
| `verbose` | bool | `false` | Parsed for compatibility. |
| `job_id` | string? | `null` | Added to session directory name after timestamp. |
| `default_light_theme` | string? | `null` | Frontend theme default. |
| `default_dark_theme` | string? | `null` | Frontend theme default. |
| `timestamp_mode` | `absolute` / `relative` | `absolute` | Initial timestamp mode for storage/export metadata. Live messages include both absolute and relative data. |
| `queue_size` | integer | `20000` | Per-source MPSC channel size. |
| `control_api` | bool | `true` | Enable the `/api/v1/control` WebSocket endpoint. |

## `logs`

```yaml
logs:
  dir: logs/
```

Relative `logs.dir` values are resolved relative to the config file directory, not necessarily the process current directory. This is especially important for Tauri onboarding: the default onboarding config uses `logs/`, so sessions are stored next to the generated app config, under `<tauri app_config_dir>/logs/`. See [tauri.md](tauri.md).

Each session directory also includes a `combined.jsonl` file: one structured JSON object per log line across all configured sources. This is useful for agents and automation that want a single append-only stream instead of reading multiple per-source `.log` files.

For `network_capture` sources, combined entries also include structured packet metadata fields such as `src_ip`, `dst_ip`, `src_port`, `dst_port`, `payload_len`, `payload_preview_len`, `payload_hex_preview`, and `payload_truncated`.

## `sources`

Every source requires:

```yaml
- name: SOURCE_ID
  type: udp # uart | udp | file | network_capture
```

Common optional keys:

| Key | Notes |
| --- | --- |
| `label` | Friendly UI label. Defaults to `name`. |
| `parser.type` | `text` or `cbor-datagram`. |

### UDP source

```yaml
sources:
  - name: DUT
    label: Device UDP
    type: udp
    port: 6000
    parser:
      type: text
```

`port` must be an integer. UDP binds on `0.0.0.0:<port>`.

### UDP CBOR datagram source

```yaml
sources:
  - name: SENSORS
    type: udp
    port: 6002
    parser:
      type: cbor-datagram
```

`cbor-datagram` is valid only for UDP sources.

### UART source

```yaml
baudrate: 115200

sources:
  - name: UART_DUT
    label: UART main
    type: uart
    port: /dev/ttyUSB0
    baudrate: 921600 # optional source override
```

`port` must be a string. The runtime opens the port through the Rust `serialport` crate.

### File source

```yaml
sources:
  - name: FILE_WATCH
    label: Watched file
    type: file
    port: ./device.log
```

`port` is a file path string. The source creates the file if missing, starts reading from the current end, and emits appended lines.

### Network capture source

Deterministic mock source for demos/tests:

```yaml
sources:
  - name: NET_CAPTURE
    type: network_capture
    interface: mock0
    network_backend: mock
    mock_interval: 1.0
    bpf_filter: udp or coap
```

Minimal real UDP capture with libpcap/Npcap:

```yaml
sources:
  - name: COAP_NET
    type: network_capture
    interface: en0
    network_backend: pcap
    udp:
      ports: [8333, 5683, 5684]
      host: 192.168.1.10
      src_ips: [192.168.1.20]
      dst_ips: [224.0.1.187]
    snaplen: 256
    promisc: false
    payload:
      include_preview: true
      max_preview_bytes: 192
```

Notes:

- `network_backend: mock` emits deterministic synthetic events.
- `network_backend: pcap` is a simplified UDP packet tap, not a full packet sniffer.
- `udp.ports` narrows capture to matching source or destination UDP ports.
- `udp.host`, `udp.src_ips`, and `udp.dst_ips` add IP constraints on top of the UDP-port filter.
- `bpf_filter` may still be set; when present it is combined with the structured UDP filter using `and (...)`.
- `snaplen` limits captured bytes per packet. Small values like `256` or `512` are recommended for CoAP/UDP use-cases.
- `payload.max_preview_bytes` limits the emitted hex payload preview in the log line.
- Real packet capture requires building with the Cargo feature `pcap-capture` and having `libpcap`/`Npcap` available at build/runtime.

## `tabs`

Tabs define which panes the UI renders. Each tab has one or two panes.

Simple form:

```yaml
tabs:
  - label: Device
    panes: [DUT, HOST]
```

Detailed form with plugins:

```yaml
tabs:
  - label: CoAP
    panes:
      - source: COAP_RAW
        plugins: [hex-coap]
```

Detailed plugin options form:

```yaml
tabs:
  - label: CoAP
    panes:
      - source: COAP_RAW
        plugins:
          - name: hex-coap
            options:
              showPayload: true
```

Validation rules:

- tab label must be non-empty
- each tab must have 1 or 2 panes
- every pane must reference an existing source name

## Frontend plugins

Built-in plugin:

```yaml
frontend_plugins:
  hex-coap:
    builtin: hex-coap

tabs:
  - label: CoAP
    panes:
      - source: COAP_RAW
        plugins: [hex-coap]
```

This loads `frontend/plugin-hex-coap.js`.

Custom plugin file:

```yaml
frontend_plugins:
  my-plugin:
    path: ./plugins/my-plugin.js
```

Relative custom plugin paths resolve against the config file directory.

## Full example (new model)

```yaml
version: 1

server:
  host: 127.0.0.1
  ws_port: 8080
  app_name: embed-log demo
  timestamp_mode: absolute
  control_api: true

logs:
  dir: logs/

baudrate: 115200

frontend_plugins:
  hex-coap:
    builtin: hex-coap

sources:
  - name: DUT
    label: Device
    type: udp
    port: 6000

  - name: HOST
    label: Host Controller
    type: udp
    port: 6001

  - name: UART_DUT
    label: UART Main
    type: uart
    port: /dev/ttyUSB0
    baudrate: 921600

  - name: COAP_RAW
    label: CoAP Raw Hex
    type: udp
    port: 6005

  - name: SENSORS
    label: Sensor CBOR
    type: udp
    port: 6002
    parser:
      type: cbor-datagram

  - name: FILE_WATCH
    label: Watched File
    type: file
    port: ./device.log

  - name: NET_CAPTURE
    label: Network Mock
    type: network_capture
    network_backend: mock
    interface: mock0
    mock_interval: 1.0
    bpf_filter: udp or coap

tabs:
  - label: Device
    panes: [DUT, HOST]

  - label: UART
    panes: [UART_DUT]

  - label: CoAP
    panes:
      - source: COAP_RAW
        plugins: [hex-coap]

  - label: Sensors
    panes: [SENSORS]

  - label: File/Net
    panes: [FILE_WATCH, NET_CAPTURE]
```

## Legacy inject/forward ports (deprecated)

The old per-source `inject_port`, `forward_port`, and `forward_ports` fields are
**deprecated**.  They still parse and produce a startup warning, but new
configs should use the single control WebSocket endpoint instead.

| Legacy field | Replacement |
|---|---|
| `inject_port: <tcp-port>` | Use the control API's `log.inject` and `tx.write` commands |
| `forward_port: <tcp-port>` | Use the control API's `subscribe` and `entries()` |
| `forward_ports: [<ports>]` | Same as above; one subscription replaces any number of forward ports |

Migration is straightforward: instead of starting per-source TCP servers, open
one WebSocket to `ws://host:port/api/v1/control` and send JSON commands.
