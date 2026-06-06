# Demos

## Quick start

```bash
# Full demo — 7 tabs with UART, UDP, CoAP, CBOR, and network capture
embed-log demo --profile deterministic --fast

# Focused network capture demo (one tab, mock traffic)
embed-log run --config config-samples/single_network_single_tab.yml
```

Both open the UI at `http://127.0.0.1:8080/` by default.

---

## `embed-log demo` command

The built-in `demo` command starts a server with a pre-configured 7‑tab layout (DevA, DevB, PYTEST, cbor-tab, CoAP, UART, Network) and launches simulated traffic in the background. No configuration file needed.

```
embed-log demo [--profile PROFILE] [--fast] [--continuous] [--cycles N]
```

### Profiles

| Profile | Content | Best for |
|---------|---------|----------|
| `curated` | Hand-written REST API / CoAP story with markers | Demos, screenshots |
| `deterministic` | Predictable test messages (repeatable) | UI tests, verification |
| `random` | Random log lines at variable intervals | Stress testing |
| `test` | Alias for `deterministic` | — |

### Common flags

| Flag | Effect |
|------|--------|
| `--fast` | Reduces tick interval from 300 ms → 50 ms; tightens random profile delays |
| `--continuous` | Restarts traffic generators when they exit (keeps the demo alive indefinitely) |
| `--cycles 0` | Same as `--continuous` |
| `--cycles 50` | Run exactly 50 traffic ticks then stop |
| `--no-browser` | Don't open the browser |
| `--log-dir DIR` | Override log output directory |
| `--tick-ms N` | Set tick interval in milliseconds (overrides --fast) |
| `--verbose` | Enable server event logging to stdout |

### Examples

```bash
# Standard demo (curated storyline)
embed-log demo

# Fast deterministic demo for testing (runs until Ctrl-C)
embed-log demo --profile deterministic --fast --continuous

# Random traffic, headless, 100 cycles
embed-log demo --profile random --no-browser --cycles 100

# Custom tick rate
embed-log demo --profile deterministic --tick-ms 100 --continuous
```

---

## Config sample demos (`embed-log run --config`)

Each config file in `config-samples/` is a self-contained demo. Run like:

```bash
embed-log run --config config-samples/<file>.yml
```

### `single_network_single_tab.yml` — network packet capture

Mock packets (no Scapy/root needed) in a single pane:

```
proto:UDP  len:78  src:192.168.1.100:5683  dst:192.168.1.1:5683  payload:CoAP
proto:ICMP len:94  src:192.168.1.100       dst:192.168.1.1       payload:Echo request
```

- **Filter bar** shows `Filter (BPF)…` placeholder (not regex)
- Type BPF expressions like `udp`, `port 5683`, `host 192.168.1.1`
- Full JSON saved to the session log file

To switch to real capture (requires Scapy + root):

```yaml
# In config-samples/single_network_single_tab.yml, change:
network_backend: scapy
interface: lo0                # your real interface
bpf_filter: "udp port 5683"
```

Then:

```bash
# Linux: set capabilities once (no need for sudo after)
sudo setcap cap_net_raw=+ep $(which python3)
# macOS / Windows: run as root
sudo python3 -m backend.server run --config config-samples/single_network_single_tab.yml
```

### `double_uart_file_udp_coap_three_tabs.yml` — UART + file tail + UDP with CoAP plugin

Three tabs showing different source types:

| Tab | Sources | Notes |
|-----|---------|-------|
| Serial | DUT (`/dev/ttyUSB0`), DEBUG (`/dev/ttyACM0`) | Virtual or real UART |
| Logs | APP_LOG (`/var/log/myapp.log`) | Tail-follows a file |
| Net | TELEMETRY (UDP :6000), COAP_DEVICE (UDP :6001) | CoAP pane has hex-coap plugin |

### `double_uart_minimal_single_tab.yml` — simplest layout

One tab with two UART devices side-by-side.

### `double_uart_udp_multi_baud_two_tabs.yml` — UART at different baud rates

Demonstrates per-source baudrate configuration.

### `three_udp_cbor_two_tabs.yml` — structured CBOR diagnostics

UDP source with a CBOR datagram parser that decodes embedded structured data.

### `reference_full_annotated.yml` — reference config

Every supported option with inline comments explaining each field.

---

## Demo architecture (how it works)

```
embed-log demo
  ├── Starts embed-log run --config <temp config>    (WebSocket UI on :8080)
  ├── Starts deterministic_demo_traffic.py            (UDP/UART injectors)
  └── MockNetworkCaptureSource generates fake packets  (no Scapy needed)
```

The mock network source cycles through 8 predefined packet flows (CoAP request/response, mDNS, DHCP, ICMP ping) with deterministic lengths and payloads. It runs entirely in-process — no sockets, no privileges needed.

---

## Troubleshooting

**Port 8080 already in use**

```bash
lsof -ti:8080 | xargs kill -9
```

If the server was started with `sudo`, kill with `sudo`:

```bash
sudo lsof -ti:8080 | xargs sudo kill -9
```

The server now sets `SO_REUSEADDR` so restarts should not fail on TIME_WAIT sockets.

**No packets appearing in the Network tab**

- Check `embed-log run --config config-samples/single_network_single_tab.yml` (standalone, mock backend)
- In the full demo, click the **Network** tab at the end of the tab bar
- Verify the server started: `curl http://127.0.0.1:8080/api/health` should return `{"status":"ok"}`

**Scapy / real capture errors**

- macOS: packet capture requires `sudo`
- Linux: either `sudo` or `sudo setcap cap_net_raw=+ep $(which python3)`
- Windows: install [Npcap](https://npcap.com/), run as Administrator
- Scapy not installed: `pip install scapy` (or `pip install "embed-log[network-capture]"`)

**Filter changes don't take effect immediately**

BPF filters are applied on the next sniff iteration (within ~1 second). If the filter is invalid, the input shows a red border with the error in a tooltip.
