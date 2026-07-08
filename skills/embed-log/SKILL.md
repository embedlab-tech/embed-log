---
description: Query embed-log session logs (UART/UDP device logs, pytest test runs, event detections) via the embed-log CLI instead of grepping raw log files. Use whenever the user asks to find something in embed-log logs/sessions, debug a test failure recorded by embed-log, or inspect device/test output captured by embed-log.
---

# embed-log CLI

`embed-log` records UART/UDP device logs plus pytest/test-framework output into per-run
"sessions" under a logs directory, as a `combined.jsonl` stream (one structured record per
line, all sources interleaved chronologically) plus an `events.jsonl` of detected
severity/pattern hits. Full reference: `docs/cli.md` in the embed-log repo.

## Golden rule: never grep/cat the raw log files

Raw session directories contain a `combined.jsonl` (all sources merged, still full/uncompact
JSON), per-source `.log` files, and a `session.html` (a large embedded viewer — do not read
this file, it can be tens of MB of minified JSON). `grep -r` across a logs directory matches
inside all of these, including `session.html`, which is slow, noisy, and easy to blow out an
agent's context window on. Use the `embed-log sessions` subcommands below instead — they are
structured, bounded, and orders of magnitude smaller.

## Which logs directory?

`sessions` subcommands resolve which directory to inspect in this order: explicit `--dir`,
else `--config <path>` (or `EMBED_LOG_CONFIG_YML_PATH`, or `./embed-log.yml`) read for its
`logs.dir`, else `./logs`. Whenever the directory wasn't given explicitly via `--dir`, one
note is printed to **stderr** saying which directory was picked — this is informational, not
an error; stdout output is unaffected. If unsure which directory a project uses, run
`embed-log doctor` — it prints `resolved config: <path>` (the config `run` would actually
load) and, if set, `config env: EMBED_LOG_CONFIG_YML_PATH=...`.

## Recommended workflow

1. **Find the session.** `embed-log sessions list --limit 10` (newest first), or if you
   already know it's the most recent run, skip straight to using `latest` as the session id
   anywhere one is accepted (`info`, `export`, `combined`, `events`, `summary`, `marker`, and
   `search --session latest`).
2. **Get the shape of it first.** `embed-log sessions summary <SESSION_ID or latest>` —
   per-source line counts, first/last timestamps, event severity counts, session duration,
   and the last 5 lines. This is a single small, bounded call — always do this before
   searching, it tells you which sources exist and roughly what happened.
3. **Search for the specific thing.** `embed-log sessions search` with `--regex`/`--contains`
   plus `--format compact` (or `mini-jsonl` for structured output) to get a small, readable
   answer instead of raw JSON.
4. **Only pull more context if needed.** `-C N` (or `-B`/`-A`) around a match, or
   `sessions combined <id> --lines N --format compact` for the tail of a specific source.

## Command reference

```bash
# List sessions (newest first)
embed-log sessions list --limit 10 [--dir <path> | --config <path>]

# Token-efficient overview of one session — do this before searching
embed-log sessions summary latest
embed-log sessions summary <SESSION_ID> --json

# Search combined logs — the main tool
embed-log sessions search --session latest --regex 'timeout|panic|fatal' --format compact
embed-log sessions search --dir logs --source PYTEST --contains "FAILED" --format compact
embed-log sessions search --dir logs --job nightly-42 --kind network_capture --dst-port 5683
embed-log sessions search --session latest --source DUT --last 50 --format compact   # newest N matches
embed-log sessions search --dir logs --regex 'timeout' --since 1h                    # relative time window
embed-log sessions search --dir logs --regex panic -C 10 --format compact            # +/- 10 lines of context
embed-log sessions search --dir logs --contains panic --count                        # just the count

# Event-detection hits (only useful if the project's events.yml has rules configured)
embed-log sessions events latest --severity fatal --format compact

# Session manifest / raw tail / export
embed-log sessions info latest
embed-log sessions combined latest --lines 50 --format compact
embed-log sessions export <SESSION_ID> --format raw --output merged.txt

# Hand a whole session to another tool/agent for offline analysis — lossless,
# ~48% smaller than combined.jsonl (dedupes repeated/constant fields, no content changes)
embed-log sessions export <SESSION_ID> --format jsonl-deduped --output session.jsonl

# Where is the config/logs dir actually resolving to?
embed-log doctor
```

`--format` (on `search`/`combined`/`events`): `jsonl` (default, full record, byte-exact) |
`compact` (`1:23.644 C#1234 message`, best for reading — ~81% smaller than jsonl) | `mini-jsonl`
(short-keyed JSON, best for further programmatic filtering — ~77% smaller). `compact`/`mini-jsonl`
are, by default: **denoised** (ANSI escape codes, a message's duplicate leading timestamp, padded
log-level brackets, and redundant device uptime counters are stripped) and **compacted**
(timestamp shown is elapsed time since session start, not wall-clock — `sessions summary <id>`
has the absolute anchor; source names are shortcoded — initials of `_`/`-`-separated words, e.g.
`CONTROLLER`→`C`, `MCU_LINK_RX`→`MLR`, falling back to a longer prefix on a rare collision, so
they're mnemonic rather than arbitrary). The first use of each timestamp convention/shortcode in a
command's output gets a one-line explanation on **stderr** (never stdout — safe to parse output
without filtering anything out), e.g. `sessions: source code C = CONTROLLER`. `jsonl` is the
untouched escape hatch if you need exact bytes, original timestamps, or full source names. If a
search spans multiple sessions, scope it with `--session <id>` for unambiguous elapsed times
(otherwise each entry's elapsed time is relative to its own session, which can span different
absolute times).

`--since` takes `<N>s|m|h|d` (e.g. `10m`, `1h`, `2d`); conflicts with `--from`. `--last N`
conflicts with `--limit` (first-N vs last-N). Context flags (`-C`/`-B`/`-A`) conflict with
`--count` and with `--last`.

## Example: "find the run where X happened"

```bash
embed-log sessions list --limit 20
embed-log sessions search --dir logs --regex '(?i)edhoc.*timeout|timeout.*edhoc' --format compact
# -> one compact line naming the session, source, and timestamp; use that session id for
#    summary/combined/-C follow-ups instead of opening the session's raw files.
```
