# CLI reference

The CLI binary is named `embed-log`.

```bash
embed-log --help
```

## Embedded agent skill

```bash
embed-log skill live
embed-log skill recorded
embed-log skill live --json
```

`skill live` and `skill recorded` print the selected canonical skill embedded at build time, so an agent can load focused, version-matched guidance without locating this repository or installing a plugin. Raw Markdown is the token-efficient default. `--json` returns the selected skill name, `schema_version`, `embed_log_version`, `format`, and escaped `content` in one document. The command needs no config, daemon, network access, or machine-specific paths.

## Machine-readable capability discovery

`schema` is the agent/wrapper discovery interface. It does not require a config or running daemon and writes exactly one JSON document:

```bash
embed-log schema                         # compact capability index
embed-log schema sessions.read           # one command's actual Clap arguments plus semantics
embed-log schema sessions around         # split and dotted paths are equivalent
embed-log schema tx --json               # optional familiar spelling; JSON is already the default
embed-log schema tx --pretty             # indented JSON for human inspection
embed-log schema errors                  # currently stable machine error codes
embed-log schema config                  # compact config capabilities
```

The index advertises `schema_version`, `embed_log_version`, commands, interfaces, source/parser types, defaults, and hard limits. A command descriptor adds usage, options, types, enums, defaults, conflicts, known numeric constraints, mutation status, execution mode, targeting requirements, output behavior, stable errors, and semantic notes. Arguments are read from the built Clap command graph so hidden/internal commands are excluded and public option changes are reflected automatically.

Discovery is progressive and token-bounded: call bare `schema` first, then request only the relevant command. Static output contains no daemon state or machine-specific paths and can be cached by the pair `(schema_version, embed_log_version)`. Continue using `status --json` for current instances, sessions, sources, and write capabilities.

`schema errors` reports `"coverage":"all_json_invocations"`. A failed invocation requesting JSON writes exactly one `{ "ok": false, "error": { "code", "message", "details" } }` document to stdout and exits nonzero; `COMMAND_FAILED` is the stable fallback when no narrower classification applies. Human-mode failures remain concise stderr text. `schema config` is a compact capability descriptor, not yet a formal JSON Schema document.

Global options:

| Option | Meaning |
| --- | --- |
| `-c, --config <PATH>` | Config file. Falls back to `EMBED_LOG_CONFIG_YML_PATH`, then `embed-log.yml`. |
| `--frontend-dir <PATH>` | Filesystem frontend directory for development. Defaults to `frontend`. Release binaries can use embedded assets. |
| `--tui` | Launch the terminal UI instead of the browser UI. |
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

Quick runs create the same session artifacts as config-based runs, under `./logs/` by default or the `--log-dir` path when supplied. All normal run flags work in this mode, including `--tui`, `--no-open-browser`, `--log-dir`, `--host`, and `--port`. See [Quick start](quickstart.md) for the shortest examples.

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

- fails with config/quick-run guidance if the resolved config does not exist
- starts `LogServer`
- serves UI/API on the configured `server.listen` endpoint
- opens the browser unless `--no-open-browser` is passed
- writes session artifacts under `logs.dir`
- exports `session.html` on Ctrl-C shutdown

Useful runtime overrides:

```bash
embed-log run --config embed-log.yml --host 0.0.0.0 --port 9090 --log-dir /tmp/embed-log-runs
```

`--host` and `--port` override the host and port from `server.listen` in memory. `--log-dir` overrides `logs.dir` and is resolved relative to the current working directory.

### Daemon instances

Start a named config-based daemon and wait for its status API to become ready:

```bash
embed-log run --daemon --instance bench-a --config embed-log.yml --json
```

Daemon startup requires explicit `--config` and `--instance`. Its endpoint uses `--host`/`--port` overrides, then `server.listen` from YAML, then the default `127.0.0.1:18080`; it never scans for or selects another port. Repeating the same instance, endpoint, and unchanged config is idempotent and returns `reused: true`; endpoint/config conflicts fail. Instance records contain the PID, endpoint, config path and fingerprint, logs directory, diagnostic log, executable, and start time. They live under `$XDG_RUNTIME_DIR/embed-log`, with a user-state fallback; tests may override this with `EMBED_LOG_RUNTIME_DIR`.

Inspect or stop it:

```bash
embed-log status --instance bench-a --json
embed-log stop --instance bench-a --json
```

Read-only `status` resolves `--instance`, then `EMBED_LOG_INSTANCE`, then the only running instance. Mutating commands such as `stop` and `sessions new` require `--instance`, `EMBED_LOG_INSTANCE`, or an explicit URL where supported. Query an unregistered or remote server directly with `embed-log status --url http://127.0.0.1:18080 --json`.

`stop` verifies that the recorded PID still refers to the same executable before signaling it, waits for clean shutdown, and removes the registry record. Stale-record removal is reported on stderr, while malformed registry files fail visibly instead of being ignored. Daemon shutdown does not automatically export HTML. CLI-only source definitions are not yet accepted with `--daemon`.

### UART TX and atomic expectations

Write a line through a UART already owned by the daemon:

```bash
embed-log tx --instance bench-a --source DUT_UART --line status --json
```

`--line` strips existing CR/LF terminators and writes one trailing carriage return. Use `--raw TEXT`, `--file PATH`, or `--stdin` to send exact bytes without line-ending normalization. Exactly one input mode is required.

Arm an RX expectation before writing and return bounded live context:

```bash
embed-log tx --instance bench-a --source DUT_UART \
  --line reset --expect "boot complete" \
  --timeout 30s --context 20 --json
```

Substring matching is the default; `--expect-regex` enables a regular expression. TX entries never satisfy an expectation. A timeout exits unsuccessfully and, with `--json`, emits an `EXPECT_TIMEOUT` object containing the successful byte count and bounded context observed after the command was armed. A control-stream gap fails instead of claiming a potentially unsafe result. Successful expectations expose the match sequence as `next_cursor`. TX requires `--instance`, `EMBED_LOG_INSTANCE`, or `--url http://host:port`; it never infers the sole daemon.

### Temporary watches

Use a watch when the trigger is external to UART TX:

```bash
watch_id=$(embed-log watch add --instance bench-a \
  --source DUT_UART --contains "session established" \
  --ttl 30s --json | jq -r '.watch.id')

embed-log watch wait "$watch_id" \
  --instance bench-a --timeout 30s --json

embed-log watch remove "$watch_id" --instance bench-a --json
```

`watch add` accepts exactly one of literal `--contains` or `--regex`. Watches are server-side, process-local, temporary, one-shot conditions. They match committed RX records directly and do not stream ordinary logs to the waiting CLI. A match is retained in memory, so `watch wait` still succeeds if the record arrived before it connected. Watch state is never persisted to the session.

`--ttl` controls how long the server actively matches; it defaults to 30 seconds and is capped at 24 hours. Matched or expired state remains queryable until `watch remove` or process shutdown. `watch wait --timeout` only limits that CLI invocation and does not alter server TTL. JSON failures use `WATCH_EXPIRED`, `WATCH_WAIT_TIMEOUT`, or `WATCH_NOT_FOUND`. All watch mutations require `--instance`, `EMBED_LOG_INSTANCE`, or `--url`; they never infer the sole daemon. Matched watch output includes the triggering record's session-global `sequence` and exposes it as `next_cursor` for bounded follow-up reads.

### Create an experiment session

Rotate a running server without restarting source tasks or releasing UARTs:

```bash
embed-log sessions new \
  --instance bench-a \
  --title "EDHOC reconnect attempt 3" \
  --json
```

The original title is stored in `manifest.json` and returned by the session APIs. The directory/session ID includes a filesystem-safe slug, for example `2026-08-03_14-22-10_edhoc-reconnect-attempt-3`. Titles must be non-empty, contain a letter or number, and be at most 120 characters. Use `--url http://host:port` instead of `--instance` for an unregistered server.

Rotation broadcasts `session_rotated`; connected browser and TUI clients clear their old panes and continue on the new session. Foreground rotation exports the completed session HTML. Daemon rotation leaves raw artifacts only unless export is explicitly requested. Disconnecting the final browser never triggers an export.

## Validate config

```bash
embed-log validate --config embed-log.yml
embed-log validate --config embed-log.yml --json
```

Loads the config, runs validation, and prints the resolved server/log/source/tab summary.

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

`doctor` reports the binary version, host system info, and config resolution:
- which OS / architecture the binary is running on
- `config env: EMBED_LOG_CONFIG_YML_PATH=...` — shown whenever that env var is set, so you can tell why a given config got picked
- `resolved config: <path>` — always shown; the exact config path `run` would load (`--config` → `EMBED_LOG_CONFIG_YML_PATH` → `embed-log.yml`), even if you didn't pass `--config` to `doctor` itself
- config summary (sources and tabs) if the resolved config file exists and loads; a missing config is reported as normal, not a warning
- configured UART paths, plus explicitly requested repeatable `--serial <path>` checks

Serial checks only test filesystem-level readability/writability and never configure or reset an attached UART. A missing path or permission denial produces an actionable warning.

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

### Global sequence, bounded reads, and context

Every record captured by the current version receives a session-global `sequence` in the same serialized order used by `combined.jsonl`, replay, and live publication. `line_idx` remains source-local. A compact line therefore identifies both positions:

```text
T+00:12.453 719 DUT_UART#428 boot complete
```

Read only a bounded page:

```bash
embed-log sessions read latest --dir logs --limit 100
embed-log sessions read latest --dir logs --after 100 --limit 50 --json
embed-log sessions read latest --dir logs --source DUT_UART --last 20 --time none --json
```

Forward reads default to 100 records and all limits are capped at 1000. `--after` is the global cursor even when `--source` filters the returned records. Selecting a configured virtual merge dynamically expands to its member sources while returned records retain their original `source_id`, `line_idx`, and `sequence`. JSON compact output hoists its schema once:

```json
{"ok":true,"session_id":"...","fields":["relative_time","sequence","source_id","line_idx","message"],"records":[["T+00:12.453",719,"DUT_UART",428,"boot complete"]],"truncated":false,"next_cursor":719,"invalid_records":0}
```

Timestamp display is independent of output representation:

- `--time relative` (default): `T+00:12.453`;
- `--time none`: omit time for the smallest output;
- `--time absolute`: include RFC3339 wall-clock time.

Use `--format full-json` when complete stored records, including all timestamp and parser metadata, are required. It always emits a JSON envelope. Sessions captured before global sequencing fail with an actionable compatibility error instead of inventing cursors.

Fetch deterministic cross-source context by sequence:

```bash
embed-log sessions around latest --sequence 719 --before 10 --after 20 --json
```

The total around window is capped at 1000 records. Sequence and source-local line counters reset to 1 and 0 respectively on titled rotation. Materialized merge records from older sessions are excluded from combined output, bounded reads/context, search, summaries, and exports by default; pass `--include-materialized-merges` to the applicable read command only when the redundant compatibility records are specifically required. TX expectations, watch matches, browser records, and TUI records carry the same sequence.

### Legacy search/combined output format: `--format`

`sessions search` and `sessions combined` take `--format`, useful for keeping agent/script output small:

| Format | What it looks like | Size vs. `jsonl`\* |
| --- | --- | --- |
| `jsonl` (default) | The full JSONL record, byte-for-byte as stored. | baseline |
| `compact` | One human-readable line: `1:23.644 D#1234 panic: watchdog reset`. | ~81% smaller |
| `mini-jsonl` | Small JSON object with short keys: `{"t":"1:23.644","s":"D","i":1234,"m":"panic: watchdog reset"}` (adds `src`/`dst`/`len` for packet entries). | ~77% smaller |

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
```

Read the session-wide combined JSONL stream:

```bash
embed-log sessions combined <SESSION_ID> --dir logs
embed-log sessions combined <SESSION_ID> --dir logs --lines 50
embed-log sessions tail-combined <SESSION_ID> --dir logs --follow
embed-log sessions combined latest --follow --format compact
```

Show a token-efficient overview of one session — the recommended first call before searching, especially for agents:

```bash
embed-log sessions summary latest
embed-log sessions summary latest --json
```

Prints per-source line counts and first/last timestamps, session duration, and the last 5 combined-log lines — a small, bounded summary instead of scanning the full log.

Search across session combined streams:

```bash
embed-log sessions search --dir logs --source DUT
embed-log sessions search --dir logs --source DUT --from 2026-07-03T09:00:00 --to 2026-07-03T15:00:00
embed-log sessions search --dir logs --job nightly-42 --kind udp --contains timeout
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

## Environment variables

| Variable | Used by | Meaning |
| --- | --- | --- |
| `EMBED_LOG_CONFIG_YML_PATH` | CLI | Config path fallback. |
| `RUST_LOG` | tracing | Log filtering, e.g. `RUST_LOG=debug`. |
