# Skill: embed-log sessions CLI

## Intent

This skill covers the `embed-log sessions` subcommand tree — inspecting, exporting, and managing recorded session artifacts.

---

## Overview

Sessions are directories under `<log-dir>/<session-id>/` containing:

- `<source>.log` — raw log data per source
- `manifest.json` — session metadata (start time, sources, tabs, markers count, HTML status)
- `session.html` — exported static HTML (if generated)
- `markers.json` — saved markers (if any)
- `snippets/` — saved selection snippets (if any)

Each session has a full ID (`2026-01-01_00-00-00`) and a short alias (`31f0`). Either form is accepted wherever a `session-id` is required.



## Common workflows

```bash
# List all sessions
embed-log sessions list

# Get session details
embed-log sessions info 31f0

# Export a session as HTML (server-side, from log files → static HTML)
embed-log sessions export 31f0

# Open session HTML in the default browser
embed-log sessions open 31f0

# Print session logs to stdout
embed-log sessions logs 31f0

# List markers for a session
embed-log sessions marker list 31f0

# Show a specific marker
embed-log sessions marker show 31f0 2

# List saved selection snippets
embed-log sessions snippet list 31f0

# Show the most recent snippet
embed-log sessions snippet show 31f0

# Delete a session
embed-log sessions delete 31f0 --yes

# Delete all sessions older than 7 days
embed-log sessions delete --older-than 7d --yes
```

### Search workflows

```bash
# Find a session by app name or job ID
embed-log sessions list --search build-123

# Find sessions with markers but no HTML export
embed-log sessions list --with-markers --no-html

# Search one session for an error string
embed-log sessions logs 31f0 --grep "timeout"

# Filter logs to one pane and one time range
embed-log sessions logs 31f0 --pane SENSOR_A --after 5m --before 1m

# Regex search within a session
embed-log sessions logs 31f0 --regex "temp=.*[5-9][0-9]"

# Case-insensitive search with bounded output
embed-log sessions logs 31f0 --grep "error" --ignore-case --tail 50

# Search markers by description
embed-log sessions marker list 31f0 --search timeout

# Filter markers by pane
embed-log sessions marker list 31f0 --pane SENSOR_A

---

## Subcommands

### list

List recorded sessions.

```
embed-log sessions list
embed-log sessions list --sort name
embed-log sessions list --limit 5
embed-log sessions list --json
embed-log sessions list --search build-123
embed-log sessions list --with-markers
embed-log sessions list --app demo --limit 10
embed-log sessions list --after 2026-05-01 --before 2026-05-30
```

| Flag | Default | Description |
|---|---|---|
| `--sort` | `date` | `date` or `name` |
| `--limit` | — | Max number of sessions to show |
| `--json` | `false` | Machine-readable JSON output |
| `--search` | — | Free-text match against session ID, alias, app name, job ID, config path |
| `--app` | — | Filter by app name |
| `--with-markers` | `false` | Only sessions that have markers |
| `--no-html` | `false` | Only sessions without HTML export |
| `--html-ready` | `false` | Only sessions with ready HTML export |
| `--after` | — | Only sessions started after this date/ISO time |
| `--before` | — | Only sessions started before this date/ISO time |

The table includes a `MRK` column showing the marker count.

### info

Show detailed session information.

```
embed-log sessions info <session-id>
embed-log sessions info <session-id> --json
```

Output includes: session ID, alias, app name, start time, job ID, config path, HTML export status, sources with line counts, and tabs.


### logs

Print session log file contents to stdout.

```
embed-log sessions logs <session-id>
embed-log sessions logs <session-id> --pane SENSOR_A
embed-log sessions logs <session-id> --grep "timeout"
embed-log sessions logs <session-id> --grep "error" --ignore-case --tail 50
embed-log sessions logs <session-id> --regex "temp=.*[5-9][0-9]"
embed-log sessions logs <session-id> --after 5m --before 1m --grep "reset"
embed-log sessions logs <session-id> --grep "panic" --context 3
```

| Flag | Description |
|---|---|
| `--pane` | Filter by pane/source name |
| `--grep` | Search for text in log lines (substring, or regex with `--regex`) |
| `--regex` | Treat `--grep` as a Python regex |
| `--ignore-case` | Case-insensitive search |
| `--tail` | Show only last N matching lines |
| `--head` | Show only first N matching lines. Mutually exclusive with `--tail` |
| `--context` | Show N lines of context around matches (requires `--grep`) |
| `--after` | Only lines after this time (relative: 5m, 2h, 30s or ISO timestamp) |
| `--before` | Only lines before this time (relative or ISO) |

### export

Export session data. Supports two formats:

**HTML** (default): rebuilds a static `.html` file from the log files, usable as a standalone replay.

```
embed-log sessions export <session-id>
embed-log sessions export <session-id> --output report.html
embed-log sessions export --missing
```

**Raw merged log**: produces a merged, time-ordered text file.

```
embed-log sessions export <session-id> --format raw
embed-log sessions export <session-id> --format raw --after 5m --output recent.log
```

| Flag | Default | Description |
|---|---|---|
| `session_id` | — | Session ID or alias. Omit with `--missing` |
| `--missing` | `false` | Export all sessions that lack HTML |
| `--output` | — | Output file path |
| `--format` | `html` | `html` or `raw` |
| `--after` | — | Only lines after this time (e.g. `5m`, `2h`, `2026-01-01T00:00:00`) |
| `--before` | — | Only lines before this time |
| `--first` | — | First N minutes/hours (e.g. `10m`, `1h`) |
| `--last` | — | Last N minutes/hours (e.g. `30m`). Exclusive with `--first` |
| `--pane` | all | Include only this pane (repeatable: `--pane A --pane B`) |
| `--first-log-at` | — | Override the absolute timestamp of the first log line for HTML export |

### open

Open the session HTML in the default browser.

```
embed-log sessions open <session-id>
embed-log sessions open <session-id> marker 2
```

Opening with `marker N` jumps to the Nth marker in the exported HTML (1-based, sorted by timestamp).

### delete

Delete recorded session(s) from disk.

```
embed-log sessions delete <session-id>
embed-log sessions delete <session-id> --yes
embed-log sessions delete --older-than 7d
embed-log sessions delete --older-than 30d --yes
embed-log sessions delete --all
```

| Flag | Description |
|---|---|
| `session_id` | Session ID or alias to delete |
| `--older-than` | Delete sessions older than this duration (e.g. `7d`, `30d`, `24h`) |
| `--all` | Delete ALL sessions |
| `--yes` / `-y` | Skip confirmation prompt. **Required for `--all` without `--older-than`** |

### marker

List or show markers (notes) saved on log lines.

```
embed-log sessions marker list <session-id>
embed-log sessions marker show <session-id> 2
```

| Subcommand | Arguments | Description |
|---|---|---|
| `list` | `session-id` | List markers. Supports `--search TEXT` (filter by description) and `--pane NAME` (filter by pane) |
| `show` | `session-id`, `marker-index` | Detailed view of one marker (pane, lines, description, timestamp, created-at) |

Marker index is 1-based from the `list` output.

### snippet

Manage saved selection snippets.

```
embed-log sessions snippet list <session-id>
embed-log sessions snippet show <session-id>
embed-log sessions snippet show <session-id> --index 2
embed-log sessions snippet show <session-id> <filename>
embed-log sessions snippet delete <session-id> --index 2
embed-log sessions snippet delete <session-id> --all
```

| Subcommand | Arguments | Description |
|---|---|---|
| `list` | `session-id` | List saved snippets |
| `show` | `session-id` `[snippet-id]` | Print snippet content. Omitting `snippet-id` shows the most recent. Use `--index N` for Nth from list, or pass a filename/prefix |
| `delete` | `session-id` | Delete by `--index N` or `--all` |


## Shared flags

All subcommands accept:

| Flag | Default | Description |
|---|---|---|
| `--log-dir` | `logs/` | Path to the log directory tree |

The `--json` flag is available on these subcommands only: `list`, `info`, `snippet list`.

---

## Notes

- Session IDs can be shortened to the unique 4-character alias shown in `list` output
- The `--log-dir` flag must point to the root of the log tree, not to an individual session directory
- `marker` and `snippet` commands operate on data already written to disk by the server at runtime
- `--yes` / `-y` skips the confirmation prompt on `delete`; without it the command prompts interactively before removing anything
- `--yes` is not required — it only suppresses the prompt
- `sessions export --missing` only works with `--format html` (not raw)
- `sessions export --first-log-at` only affects HTML export (ignored in raw mode)
- `sessions open` requires the session HTML to already exist; run `sessions export <id>` first if it does not
- `sessions snippet show` defaults to the most recent snippet when no `snippet-id`, `--index`, or `--last` is given
- `sessions snippet show <filename>` matches by suffix and substring containment; if multiple files match the command fails and lists them
