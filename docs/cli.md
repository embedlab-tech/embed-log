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

### Fast serial start (no YAML)

Pass UART device paths directly to `run` for a temporary configuration:

```bash
embed-log run /dev/ttyUSB0
embed-log run /dev/ttyUSB0 /dev/ttyUSB1 --tui
```

For mixed inputs, use repeatable explicit flags:

```bash
embed-log run -s /dev/ttyUSB0 -s /dev/ttyUSB1 -f ./device.log --baud 115200
```

`-s` / `--serial` adds a UART, `-f` / `--file` watches an appended file, and `--baud` applies to every quick-run UART (default: `115200`). Each source gets its own tab. The generated configuration is in memory: no YAML is read or written, and `--config` cannot be combined with quick-run sources. Use `--save-config embed-log.yml` to persist it for later customization.

Quick runs create the same session artifacts as config-based runs, under `./logs/` by default or the `--log-dir` path when supplied. All normal run flags work in this mode, including `--tui`, `--no-open-browser`, `--log-dir`, `--host`, and `--ws-port`. See [Quick start](quickstart.md) for the shortest examples.

### Config-based run

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

Version output includes the package version, Git revision, build time, target triple, and running executable path. Use `--json` for release/support diagnostics.

Doctor:

```bash
embed-log doctor
embed-log doctor --json
embed-log doctor --config embed-log.yml
embed-log doctor --serial /dev/ttyUSB0
```

`doctor` reports the binary version, host system info, config resolution, and packet-capture readiness:
- which OS / architecture the binary is running on
- `config env: EMBED_LOG_CONFIG_YML_PATH=...` — shown whenever that env var is set, so you can tell why a given config got picked
- `resolved config: <path>` — always shown; the exact config path `run` would load (`--config` → `EMBED_LOG_CONFIG_YML_PATH` → `embed-log.yml`), even if you didn't pass `--config` to `doctor` itself
- config summary (sources/tabs/pcap sources) if the resolved config file exists and loads; a missing config is reported as normal, not a warning
- whether the binary was built with the `pcap-capture` feature
- whether the native packet-capture library is installed (`libpcap` on Unix-like systems, `Npcap`/`WinPcap` on Windows)
- whether the inspected config contains `network_capture` sources using `network_backend: pcap`
- configured UART paths, plus explicitly requested repeatable `--serial <path>` checks

Serial checks only test filesystem-level readability/writability and never configure or reset an attached UART. A missing path or permission denial produces an actionable warning.

Check for updates:

```bash
embed-log update --check
embed-log update --check --json
embed-log update --yes
embed-log update --version v1.2.0 --yes
embed-log update --version v0.9.0 --yes --allow-downgrade
```

`--check` reports the latest stable GitHub Release without changing the system. `--yes` downloads the target-matching archive, verifies it against the release `SHA256SUMS`, stages the executable beside the current binary, and replaces it with a rollback backup if replacement fails. Self-update rejects same-version and downgrade installs by default; `--allow-downgrade --yes` is the explicit escape hatch. It currently supports the same release targets as the installer: Linux x86_64 and macOS Apple Silicon/Intel. Package-managed or read-only installations should use their package manager instead.

Serial ports:

```bash
embed-log ports
embed-log ports --json
```

## Sessions

Every `sessions` subcommand takes the same `--dir`/`--config` pair to decide which logs directory it inspects:

| Option | Meaning |
| --- | --- |
| `--dir <PATH>` (alias `--log-dir`) | Logs directory to inspect. Wins over everything else — if given, no config is even read. |
| `-c, --config <PATH>` | Config file to read the logs directory (`logs.dir`) from when `--dir` is not given. Falls back to `EMBED_LOG_CONFIG_YML_PATH`, then `embed-log.yml` — same resolution `run` uses. |

Resolution order when `--dir` is omitted: resolve a config path (`--config` → `EMBED_LOG_CONFIG_YML_PATH` → `embed-log.yml`); if that file exists, use its `logs.dir` (resolved the same way `run` resolves it — absolute paths pass through, relative paths resolve against the config file's own location); otherwise fall back to `./logs` (the historical default). Whenever the directory wasn't given explicitly via `--dir`, a one-line note is printed to **stderr** saying which directory was picked and why, so the choice is never silent — stdout output (JSONL, compact lines, etc.) is unaffected, so scripts and agents parsing it don't need to filter anything out:

```bash
$ embed-log sessions list --config ~/projects/lab-a/embed-log.yml
sessions: using logs dir from /home/you/projects/lab-a/embed-log.yml: /home/you/projects/lab-a/logs
2026-07-06_14-31-18  2026-07-06T14:31:18+02:00  /home/you/projects/lab-a/logs/2026-07-06_14-31-18  0 marker(s)
```

Every subcommand that takes a `<SESSION_ID>` also accepts the literal `latest`, which resolves to the newest session under the selected directory:

```bash
embed-log sessions info latest --dir logs
embed-log sessions summary latest
```

List sessions:

```bash
embed-log sessions list --dir logs
embed-log sessions list --dir logs --limit 10
embed-log sessions list --dir logs --json
embed-log sessions list --config embed-log.yml
```

Show session manifest/info:

```bash
embed-log sessions info <SESSION_ID> --dir logs
embed-log sessions info latest --dir logs --json
```

Import an external text log into an existing session. Lines must start with RFC3339 timestamps in UTC or another explicit offset; imported records are merged into `combined.jsonl` in timestamp order:

```bash
embed-log sessions import latest ./pytest.log --source PYTEST
# 2026-07-11T11:21:47.123Z test started
# [2026-07-11T11:21:48+00:00] assertion passed
```

Open a session report in the default browser. If the HTML export is missing, it is generated first:

```bash
embed-log sessions open latest --dir logs
```

Export a recorded session:

```bash
embed-log sessions export <SESSION_ID> --dir logs --format html --output session.html
embed-log sessions export <SESSION_ID> --dir logs --format raw --output merged.txt
embed-log sessions export <SESSION_ID> --dir logs --format jsonl-deduped --output session.jsonl
```

Formats:

- `html`: self-contained viewer HTML
- `raw`: merged raw text output
- `jsonl-deduped`: a lossless, structurally deduplicated single-file JSONL export — same
  information as `combined.jsonl`, minus pure per-line duplication. `combined.jsonl` repeats
  several fields that never change within a session (`app_name`, `job_id`, `session_id`,
  `source_kind`, `source_label`, `tab_labels`) on every single line, plus a few fields that are
  exact duplicates of another field (`data`≡`message`, `timestamp_num`≡`absNum`,
  `timestamp`≡`absTs`). `jsonl-deduped` hoists the constants into a one-time header line and
  drops the exact duplicates — **~48% smaller on a measured real session, zero information
  lost**. Meant for handing a whole session to another tool or agent for offline analysis,
  without shipping the original `combined.jsonl` (raw session files are never modified — this is
  a read-time export). Output shape:
  ```json
  {"kind":"header","session_id":"...","app_name":"...","job_id":null,"sources":{"DUT":{"kind":"uart","label":"DUT","tabs":["Main"]}}}
  {"absNum":...,"absTs":"...","timestamp_iso":"...","source_id":"DUT","message":"...","line_idx":0, ...}
  ```
  Not to be confused with `--format mini-jsonl` below, which is a smaller, *lossy*, per-line
  rendering for reading a handful of matched lines — `jsonl-deduped` is a lossless, whole-session
  export.

### Output format: `--format`

`sessions search`, `sessions combined`, and `sessions events` all take `--format`, useful for keeping agent/script output small:

| Format | What it looks like | Size vs. `jsonl`\* |
| --- | --- | --- |
| `jsonl` (default) | The full JSONL record, byte-for-byte as stored. | baseline |
| `compact` | One human-readable line: `1:23.644 D#1234 panic: watchdog reset`. | ~81% smaller |
| `mini-jsonl` | Small JSON object with short keys: `{"t":"1:23.644","s":"D","i":1234,"m":"panic: watchdog reset"}` (adds `src`/`dst`/`len` for packet entries, `sev`/`ev` for events). | ~77% smaller |

\* Measured on a real 43k-line session. `compact`/`mini-jsonl` apply two layers on top of the raw
record:

- **Denoised** (always): ANSI/terminal control sequences, a message's duplicate leading timestamp
  (when it repeats the record's own timestamp — common in pytest output), padded log-level
  brackets (`[   ERROR]` → `[ERROR]`), and redundant device uptime counters
  (`[00000002] <inf> ...` → `<inf> ...`, keeping the level tag) are all stripped.
- **Compacted further** (always): the timestamp shown is elapsed time since *that entry's own
  session start* (`1:23.644` = 1 minute 23.644s in), not wall-clock time — shorter for typical
  session lengths since it never encodes hour-of-day, and it directly answers "how far into the
  run is this." The absolute anchor isn't lost — `sessions summary <id>` shows it. Source names
  are shortcoded rather than spelled out — derived from the source's own name (initials of its
  `_`/`-`-separated words: `COUNTER` → `C`, `MCU_LINK_RX` → `MLR`, `NODE-RED-COAP` → `NRC`),
  falling back to a longer prefix on a rare collision, so codes stay mnemonic instead of arbitrary
  and mostly stable across runs. The first time each timestamp convention or source code is used
  in a given command's output, a one-line explanation is printed to **stderr** (never stdout, so
  scripts/agents parsing output see only clean data) — e.g. `sessions: source code C = COUNTER`.
  If a search spans multiple sessions, elapsed times are relative to each entry's *own* session
  start — scope with `--session <id>` for unambiguous
  elapsed times across a single run.

Both layers are on by default for `compact`/`mini-jsonl` — `jsonl` remains the untouched,
byte-exact format (original wall-clock timestamps, full source names) for anyone who needs it.

```bash
embed-log sessions search --dir logs --regex 'panic|fatal' --format compact
embed-log sessions combined latest --lines 50 --format mini-jsonl
embed-log sessions events latest --severity fatal --format compact
```

Read the session-wide combined JSONL stream:

```bash
embed-log sessions combined <SESSION_ID> --dir logs
embed-log sessions combined <SESSION_ID> --dir logs --lines 50
embed-log sessions tail-combined <SESSION_ID> --dir logs --follow
embed-log sessions combined latest --follow --format compact
```

Read event-detection hits from a session:

```bash
embed-log sessions events <SESSION_ID> --dir logs
embed-log sessions events <SESSION_ID> --dir logs --severity fatal
embed-log sessions events <SESSION_ID> --dir logs --source DUT --contains watchdog
embed-log sessions events <SESSION_ID> --dir logs --json
```

Show a token-efficient overview of one session — the recommended first call before searching, especially for agents:

```bash
embed-log sessions summary latest
embed-log sessions summary latest --json
```

Prints per-source line counts and first/last timestamps, event severity counts, session duration, and the last 5 combined-log lines — a small, bounded summary instead of scanning the full log.

Search across session combined streams:

```bash
embed-log sessions search --dir logs --source DUT
embed-log sessions search --dir logs --source DUT --from 2026-07-03T09:00:00 --to 2026-07-03T15:00:00
embed-log sessions search --dir logs --job nightly-42 --kind network_capture --dst-port 5683
embed-log sessions search --dir logs --contains panic --regex 'ERROR|WARN'
embed-log sessions search --dir logs --source DUT --count
embed-log sessions search --session latest --regex 'timeout' --format compact
```

`search` scans `combined.jsonl` files under the selected log directory and prints matching entries. It can filter by session id/prefix (including `latest`), job id, source id, source kind, time window, message substring/regex, and packet fields such as source/destination UDP port or IP address.

Relative time filters, as an alternative to `--from`/`--to`:

```bash
embed-log sessions search --dir logs --regex 'timeout' --since 10m   # last 10 minutes
embed-log sessions search --dir logs --regex 'timeout' --since 1h
embed-log sessions search --dir logs --regex 'timeout' --since 2d
```

`--since` accepts a number followed by `s`/`m`/`h`/`d` and conflicts with `--from` (pick one).

Keep only the most recent matches instead of the first ones:

```bash
embed-log sessions search --session latest --source DUT --last 200
```

`--last N` keeps a bounded ring buffer of the chronologically newest N matches (memory-bounded, correct across multiple sessions) and conflicts with `--limit` (which stops after the first N).

Show surrounding lines around each match, grep-style:

```bash
embed-log sessions search --dir logs --regex panic --context 20      # -C, same before/after
embed-log sessions search --dir logs --regex panic -B 20 -A 40       # different before/after
```

Each match prints a `# match N session=... source=... line=...` header, the surrounding lines, and `<< MATCH` on the matching line. Context flags conflict with `--count` and with `--last` (not supported together yet).

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
