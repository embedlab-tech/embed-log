# Configuration

## Inspecting the running configuration

In the live browser, open **Settings → Configuration**. In the TUI, press **I** (uppercase). Both show the same read-only runtime summary: server host/port and timestamp mode, effective logs directory, source type/port or path, effective UART baudrate, parser and dictionary path. CLI overrides are reflected; this is not the original YAML. Logs, file-source and dictionary paths are made absolute (existing paths are canonicalized); UART device names such as `COM4` and numeric UDP ports are unchanged.

The summary deliberately excludes plugin options, script contents and environment variables. It is visible to connected viewers, so source paths are not private. Older servers and offline HTML without runtime metadata show the view as unavailable. Configuration cannot be edited here.

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
| `level_profiles` | mapping | `{}` | Named log level detection profiles referenced by `sources.<ID>.levels`. See [Log levels](#log-levels). |

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

`zephyr-dict` decodes standard Zephyr v3 **binary** dictionary records. For UART,
select `CONFIG_LOG_BACKEND_UART_OUTPUT_DICTIONARY_BIN=y` in the firmware. No
additional framing, escaping, or checksums are introduced by the parser.

For ASCII HEX dictionary output (`CONFIG_LOG_BACKEND_UART_OUTPUT_DICTIONARY_HEX=y`),
use `type: zephyr-dict-hex` instead. This retains the existing HEX decoder.
**Migration:** configurations that used `zephyr-dict` for HEX in earlier versions
must change that name to `zephyr-dict-hex`; the formats are not auto-detected.

The database path resolves relative to the configuration file and must match the firmware build that produced the logs.

UART TX is independent of the receive parser. A dictionary configuration can
still be used to send shell commands to plaintext firmware. If the database
cannot be loaded, both dictionary parsers report the error once and preserve
console text instead of discarding replies; dictionary records are **not decoded**
in this state. Soft UART idle callbacks also flush non-newline-terminated shell
prompts in both modes, without discarding partial dictionary frames.

The binary parser also preserves console text, ANSI sequences, and UTF-8 between
records. Firmware must serialize complete records with console output; the parser
cannot repair byte-level interleaving. Once a record is recognized, its declared
length determines its end, including arbitrary binary hexdump data.

Binary acquisition is heuristic: attaching midway through a record or byte loss
can discard initial data, and recovery is not guaranteed for arbitrary damaged
streams or binary console replies. Drop records alone do not establish initial
synchronization. During acquisition, an incomplete candidate may be skipped for
an independently validated complete record from the configured dictionary. Once
synchronized, the parser never scans inside a record's hexdump.

On UART, soft idle callbacks start after **one second without received bytes**
and repeat every 250 ms. They preserve plausible incomplete BIN records. After
**five seconds of silence**, the parser reports truncation, releases buffered
console text, and reacquires on subsequent records. A legitimate record paused
beyond that deadline is also treated as truncated. A single 100 ms serial read
timeout does not reset the parser, and newly received bytes restart the idle
deadline. File/UDP parsers have no UART idle timer. Buffers are bounded and
detectable decode failures are reported.
This does not automatically select a different firmware's dictionary. Stop or
restart a session with the matching database when flashing a different image;
do not use stale decoded boot output as evidence about the newly flashed image.

#### Empty strings and raw section bytes

The decoder handles dynamic printf width/precision (`%*s`, `%.*s`, `%*.*s`),
including signed arguments and following conversions. Literal string arguments
are not rewritten as format strings; packed strings are decoded as UTF-8.

A string pointer to a mapped string's terminating NUL resolves to `""`. The
linker can also pool `""` into a NUL inside a **binary** `.rodata` constant.
Stock Zephyr dictionaries may omit that address and contain no raw sections;
the decoder cannot infer its byte value from `string_mappings` alone. Unmapped
pointers remain visible as `<string@0x…>` rather than being silently hidden.

To resolve those addresses, enrich the dictionary once using its **matching ELF**:

```sh
python -m pip install pyelftools
python scripts/enrich_zephyr_dictionary.py \
  --database build/zephyr/log_dictionary.json \
  --elf build/zephyr/zephyr.elf \
  --output build/zephyr/log_dictionary.enriched.json
```

Use the enriched JSON as `parser.database` (and package that JSON alongside the
firmware). The tool verifies the target and existing string bytes before adding read-only
string sections. No firmware or wire-format change is required, and decoding
the enriched JSON needs neither Python nor the ELF at runtime.

### File tail

```yaml
sources:
  TEST:
    type: file
    path: ./pytest.log
```

The source creates the file if missing, starts at its current end, and emits appended lines.

Supported backend parser types are `text`, `hex-coap`, `slip-coap`, `zephyr-dict` (binary), and `zephyr-dict-hex`.

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

## Log levels

Each pane has a **Levels** button next to Highlight. It lists the levels its sources can produce, with a line count for each, plus **No level** for lines that matched no level. Unchecked levels are hidden from the pane, from Highlight counts and from navigation; the regex filter still applies on top. The choice is remembered per pane in the browser. TX lines are never hidden by the level filter. A merged pane offers the union of its members' levels.

The server classifies every received line once, with the profile of its source, and stores the result as `level` in `combined.jsonl`. The UI receives only level ids, labels, colors and default visibility; regexes stay on the server.

### Built-in Zephyr profile

Sources without a `levels` key use the built-in `zephyr` profile:

| Id | Label | Matches (case-insensitive) | Visible by default |
| --- | --- | --- | --- |
| `err` | Error | `<err>`, `[err]`, `[error]` | yes |
| `wrn` | Warning | `<wrn>`, `[wrn]`, `[warn]`, `[warning]` | yes |
| `inf` | Info | `<inf>`, `[inf]`, `[info]` | yes |
| `dbg` | Debug | `<dbg>`, `[dbg]`, `[debug]` | **no** |

Tags must appear at the start of the line, optionally after a Zephyr timestamp in any `log_output` format (uptime ticks, `[00:00:01.234,567]`, Linux `[    1.234567]`, date `[1970-01-01 00:00:01.234,567]`, ISO 8601, or the dictionary decoder's `[2024-01-01T00:00:00Z]:`). A shell prompt that Zephyr erased before printing the log line (`uart:~$ `) and stray control bytes are skipped. Tags quoted inside a message do not change its level.

Hexdump rows printed by Zephyr after a log message (`    01 02 03 … |....`) inherit the level of the message. Lines without a tag (shell prompts, `printk` output) are **No level** and stay visible. ANSI color codes are ignored when matching.

### Per-source configuration

```yaml
sources:
  DUT:
    type: uart
    path: /dev/ttyUSB0
    levels: zephyr        # default; same as omitting the key
  HOST:
    type: udp
    port: 16000
    levels: off           # no detection, no Levels button
  APP:
    type: udp
    port: 16001
    levels: app           # named profile from level_profiles
```

`levels` accepts `zephyr`, `off`, the name of a profile from `level_profiles`, or an inline profile with the same fields.

### Custom profiles

A profile either gives each level its own `match` regex, or one `pattern` with a named group `level`.

```yaml
level_profiles:
  app:
    case_insensitive: true      # default false
    unmatched: inherit          # none (default) | inherit
    continuation: '^\s+at '     # optional: lines that keep the previous level
    levels:
      - id: fatal
        label: Fatal
        match: '\bFATAL\b'
        color: '#ff4d4f'
      - id: err
        match: '\bERROR\b'      # built-in ids err/wrn/dbg keep the theme colors
      - id: wrn
        match: '\bWARN(ING)?\b'
      - id: inf
        match: '\bINFO\b'
      - id: trace
        label: Trace
        match: '\bTRACE\b'
        default_visible: false

  syslog:
    pattern: '^\S+ \S+ (?P<level>[A-Z]+):'
    aliases: { E: err, W: wrn, WARNING: wrn, I: inf, D: dbg }
    levels:
      - { id: err, label: Error }
      - { id: wrn, label: Warning }
      - { id: inf, label: Info }
      - { id: dbg, label: Debug, default_visible: false }
```

| Field | Default | Notes |
| --- | --- | --- |
| `levels` | required | Ordered list; the order is the order in the Levels menu. With per-level `match`, the first level whose regex matches wins. |
| `levels[].id` | required | 1–32 letters, digits, `_` or `-`; unique (case-insensitive). Ids `err`/`error`/`fatal`/`crit`, `wrn`/`warn`/`warning` and `dbg`/`debug` reuse the theme's line colors. |
| `levels[].label` | id | Name shown in the menu. |
| `levels[].match` | — | Regex; required for every level unless the profile has `pattern`. |
| `levels[].color` | theme | `#rgb`, `#rrggbb` (with optional alpha), a CSS color name or `var(--name)`. |
| `levels[].default_visible` | `true` | Initial checkbox state until the viewer changes it. |
| `pattern` | — | One regex with a `(?P<level>...)` group; the captured text is matched against level ids and `aliases` case-insensitively. |
| `aliases` | `{}` | Captured text → level id. Requires `pattern`. |
| `case_insensitive` | `false` | Applies to `match`, `pattern` and `continuation`. |
| `unmatched` | `none` | `none`: a line without a match is **No level**. `inherit`: it keeps the level of the previous line of the same source (multi-line messages, stack traces). |
| `continuation` | — | Lines matching this regex always keep the previous level, even with `unmatched: none`. |

Regexes use Rust [`regex`](https://docs.rs/regex/latest/regex/#syntax) syntax: no look-around or backreferences, matching time is linear in the line length. Each regex is limited to 1024 characters. Matching runs on the decoded line text without ANSI codes and without the timestamp added by Embed-log. Invalid profiles are rejected at startup with the YAML location, for example `level_profiles.app.levels[1].match is not a valid regex`. The names `zephyr` and `off` are reserved.

Records injected through the control API keep an explicit `level` from their metadata. HTML exports embed the level definitions, so the Levels button works offline; sessions recorded by older versions fall back to Zephyr tag detection in the browser.

## Restarts and crashes

Embed-log detects device restarts and crashes in received console lines. Each pane header shows a counter such as **⟳ 2 ✖ 1** once something was detected; clicking it lists the events (newest first) and jumps to the line. Restart lines get a separator above them and crash lines a red marker. A merged pane adds up its sources. In live sessions the counts cover the whole session, including events older than the lines loaded in the browser; HTML reports count the lines they contain.

| Event | Detected from |
| --- | --- |
| Restart | Boot banners: `*** Booting Zephyr OS …`, `*** Booting nRF Connect SDK …`, `*** Booting MCUboot …` |
| Crash | `>>> ZEPHYR FATAL ERROR`, `ASSERTION FAIL`, Cortex-M fault banners (`***** HARD FAULT *****`, `***** MPU FAULT *****`, …), `Fatal fault in ISR/thread`, `Kernel panic` / `Kernel oops` |

Lines of the same kind less than 5 seconds apart are one event, so a single fault dump counts as one crash and a reset through MCUboot (two banners) as one restart. A restart ends a crash group, and a crash ends a restart group, so fast crash/reboot loops retain every restart. TX lines are never events. Detection runs on the server; the record keeps `"event": "boot"` or `"crash"` in `combined.jsonl` and in HTML exports.

Clicking an event temporarily reveals its exact line even if a text, level or boot filter hides it. The filters stay set; editing a filter or clearing the pane removes the temporary reveal.

In the event list, **Only this boot** next to a restart shows only the lines from that restart until the next restart of the same device; the chip in the filter bar steps to the previous or next boot and ✕ shows everything again.

### Custom rules

```yaml
sources:
  DUT:
    type: uart
    path: /dev/ttyACM0
    events:
      builtin: true              # keep the Zephyr rules (default true)
      boot: ['^APP START v\d+', 'Reset cause: \w+']
      crash: ['Guru Meditation Error', 'PANIC:']
      group_window_ms: 3000      # default 5000
  HOST:
    type: udp
    port: 16000
    events: off                  # no restart/crash detection
```

`events` accepts `zephyr` (the default), `off`, or rules. Rules use Rust [`regex`](https://docs.rs/regex/latest/regex/#syntax) syntax and are matched against the line without ANSI codes; a boot rule is checked before the crash rules. Invalid rules fail at startup with their location, for example `sources.DUT.events.crash[1] is not a valid regex`.

### Decoding crashes with the firmware ELF

```yaml
sources:
  DUT:
    type: uart
    path: /dev/ttyACM0
    elf: build/zephyr/zephyr.elf   # relative to this file
```

With `elf` set, crash rows in the event list get a **Decode** button. The fault dump (same source, at most 80 lines total and within 5 s of the crash banner) is searched for code addresses. Register and call-trace lines immediately before the banner are included for software panic/oops dumps; context never crosses another restart or crash. Each address is resolved to function, file and line with the ELF's DWARF information, including inlined functions:

| Dump line | Address |
| --- | --- |
| `Faulting instruction address (r15/pc): 0x…` (Cortex-M) | `pc` |
| `r14/lr: 0x…` (Cortex-M) | `lr`, looked up as a return address |
| `mepc: 0x…` (RISC-V), `ELR_ELn: 0x…` (ARM64), `EIP: 0x…` (x86) | `pc` |
| `N: fp: 0x… ra: 0x…` / `N: sp: 0x… ra: 0x…` call trace (`CONFIG_EXCEPTION_STACK_TRACE`) | `frame N` |

Build the firmware with debug information (Zephyr's default `CONFIG_DEBUG_INFO=y`). The ELF is copied into the session as `symbols/<SOURCE-PREFIX>-<SHA256-of-source-id>.elf` when the session starts (and on rotation). The hash keeps distinct source IDs separate even when their filename-safe prefixes coincide or the filesystem ignores case. The manifest stores the actual snapshot path; older manifests using `symbols/<SOURCE>.elf` still work. This means a crash can be decoded later with the binary that was running; rebuild-then-decode of an old session needs `--elf` on the CLI. An address outside the ELF is shown as "not in this ELF", which usually means the firmware and the ELF do not match. A missing ELF file only disables decoding; a file that is not an ELF with debug information is a configuration error.

The same decoding is available from the CLI (`embed-log sessions decode`), the control API (`crash.decode`) and the Python SDK (`decode_crash`).

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
