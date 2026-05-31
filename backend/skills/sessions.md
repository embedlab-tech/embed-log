# Skill: embed-log sessions CLI
> **Agent: load this skill via `read skill://sessions` or `embed-log skill show sessions`**
## Agent usage

**Always use the CLI over direct file reads.** Raw data (`markers.json`, `manifest.json`, `.log` files) is accessible via structured subcommands. The CLI handles path resolution, alias expansion, and output formatting — reading files directly is fragile and unnecessary.

- Use `--json` for machine-parseable output (piped through `python3 -c "import json, sys; …"`)
- Without `--json`, output is human-readable tables

### Typical discovery workflow

```
embed-log sessions list --json           → list all sessions, parse markers field
embed-log sessions marker list <sid>     → list markers for one session
embed-log sessions marker show <sid> <N> → show marker details
```

---



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

---

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

---

## Subcommands

### list

List recorded sessions.

```
embed-log sessions list
embed-log sessions list --sort name
embed-log sessions list --limit 5
embed-log sessions list --json
```

| Flag | Default | Description |
|---|---|---|
| `--sort` | `date` | `date` or `name` |
| `--limit` | — | Max number of sessions to show |
| `--json` | `false` | Machine-readable JSON output |

The table includes a `MRK` column showing the marker count.
**Agent note:** The `--json` output includes a `markers` field (integer count per session). Use this to discover which sessions have markers before drilling into individual sessions.


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
```

| Flag | Description |
|---|---|
| `--pane` | Filter by pane/source name |

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
| `list` | `session-id` | List all markers with index, pane, lines, description |
| `show` | `session-id`, `marker-index` | Detailed view of one marker (pane, lines, description, timestamp, created-at) |

Marker index is 1-based from the `list` output.

---

### Cross-session queries

**Find all sessions that have markers:**

```bash
embed-log sessions list --json | python3 -c "
import json, sys
for s in json.load(sys.stdin):
    if s['markers'] > 0:
        print(s['session_id'], s['_alias'], s['markers'])
"
```

**Print all markers from every session:**

```bash
embed-log sessions list --json | python3 -c "
import json, subprocess, sys
sessions = [s for s in json.load(sys.stdin) if s['markers'] > 0]
for s in sessions:
    subprocess.run(['python3', '-m', 'backend.cli', 'sessions', 'marker', 'list', s['session_id']])
"
```


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

---

## Shared flags

All subcommands accept:

| Flag | Default | Description |
|---|---|---|
| `--log-dir` | `logs/` | Path to the log directory tree |
| `--json` | `false` | Machine-readable JSON output |

---

## Notes

- Session IDs can be shortened to the unique 4-character alias shown in `list` output
- The `--log-dir` flag must point to the root of the log tree, not to an individual session directory
- `marker` and `snippet` commands operate on data already written to disk by the server at runtime
- The `--json` flag works with `list`, `info`, and `export` commands
- Confirmation (`--yes`) is required for destructive `delete` operations; without it the command prompts interactively

---

## Quick install

Install embed-log, generate a config, and start the UI — no source checkout needed.

### macOS / Linux

```bash
curl -fsSL https://raw.githubusercontent.com/krezolekcoder/embed-log/main/install.sh | bash
```

### Windows (PowerShell 7+)

```powershell
iex ((New-Object System.Net.WebClient).DownloadString('https://raw.githubusercontent.com/krezolekcoder/embed-log/main/install.ps1'))
```

### After install

```bash
# Generate a config file (interactive)
embed-log sample-config

# Or pick a template directly:
embed-log sample-config --sample single-tab-dual-pane.yml

# Start the server
embed-log run --config embed-log.yml

# Open the UI in your browser
# Default: http://127.0.0.1:8080/

# No hardware yet? Run the simulated demo:
embed-log demo
embed-log demo --profile curated   # more realistic sensor data
embed-log demo --fast              # higher speed, fewer cycles
```

**Requirements:** Python ≥ 3.10 (installer handles pipx setup automatically).
