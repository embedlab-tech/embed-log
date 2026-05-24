# CLI Simplify Plan

## Goal
Make `embed-log` feel obvious for first-time users:

1. create config
2. validate config
3. run config

Everything else should be secondary.

---

## Current state

Good:
- `embed-log create-config` wizard now exists
- `init` still works as an alias
- config-first flow is possible

Still problematic:
- top-level CLI still exposes too many advanced flags
- help text is still closer to an engineer/debugger interface than a user workflow
- direct CLI source definition (`--source`, `--inject`, `--forward`, `--tab`) competes with YAML config as a second configuration model
- advanced/internal commands are not clearly separated from normal usage

---

## Desired UX

### First-run user story
User installs `embed-log` and runs:

```bash
embed-log
```

Expected behavior:
- if no config is present, show a short guided message:

```text
No config found.

Start here:
  embed-log create-config
  embed-log validate --config embed-log.yml
  embed-log run --config embed-log.yml
```

- if `embed-log.yml` exists, suggest:

```text
Config found: embed-log.yml
Run it with:
  embed-log run --config embed-log.yml
```

Optional later improvement:
- ask whether to launch the config directly

---

## Proposed command model

### Core commands
These should be the only commands shown prominently in help:
- `embed-log create-config`
- `embed-log validate`
- `embed-log run`

### Secondary commands
These are useful, but not part of the main onboarding path:
- `embed-log parse`
- future export/import/session utilities

### Advanced/internal commands
These should be hidden from the main help or placed under an advanced section:
- direct source wiring flags
- low-level compatibility/debug paths

---

## Help redesign

## Top-level help should become task-oriented

Target shape:

```text
embed-log — collect UART/UDP logs and view them in a browser UI

Common workflow:
  embed-log create-config
  embed-log validate --config embed-log.yml
  embed-log run --config embed-log.yml

Commands:
  create-config   interactively create a config file
  validate        validate a config file
  run             start the log server from a config file

Advanced:
  parse           parse exported HTML/session artifacts
```

### Rules
- show the 3-step workflow first
- do not put advanced examples before the common path
- do not lead with long lists of flags

---

## create-config follow-up improvements

The wizard is in place, but can be improved.

### Candidate follow-ups
- optional config preview before write
- smarter source-name defaults derived from UART/COM device names
- better filtering of noisy serial devices on macOS
- allow manual reordering/editing before final write
- optional non-interactive mode later

### Non-goals for the wizard
Do not turn it into the advanced interface.
Keep these out of the wizard:
- inject ports
- forward ports
- themes
- job id
- custom UI path
- advanced logging/server options

These stay manual YAML edits.

---

## Direct CLI source flags

Current issue:
- `--source`, `--inject`, `--forward`, `--tab` make the CLI feel like a second config format

Recommendation:
- keep them for backward compatibility and power users
- move them into help section labeled `Advanced run options`
- do not present them in onboarding docs

Longer term, evaluate whether they are still worth keeping publicly supported.

---

## Possible new command: doctor

A future `embed-log doctor` command could reduce support load.

### It should check
- Python/runtime availability
- config file exists
- YAML parses
- source names unique
- UDP ports valid
- serial ports exist
- UI port availability
- browser/opening assumptions if relevant

This would be especially useful after `create-config`.

---

## Documentation cleanup

Docs should consistently present:

1. install
2. create config
3. validate
4. run

### Required follow-ups
- remove `init` from primary docs over time
- keep `init` documented only as a compatibility alias if needed
- avoid examples centered on direct `backend/server.py` invocation unless explicitly labeled as developer mode
- keep advanced config/manual edits separate from the beginner path

---

## Recommended implementation order

### Phase 1
- rewrite top-level help around the 3-step workflow
- simplify no-args behavior
- move advanced commands/flags to secondary help sections

### Phase 2
- refine `create-config` UX based on real usage
- clean docs so `create-config` is the primary path everywhere

### Phase 3
- add `doctor`
- implement the `sessions` subcommand tree
- decide whether direct source CLI flags remain public or become advanced-only

---

## Acceptance criteria for the CLI simplification effort

The CLI is simplified when:
- a new user can understand the basic workflow from the first help screen
- `create-config`, `validate`, and `run` are clearly dominant
- advanced/internal features no longer compete with the main path
- docs follow the same mental model as the CLI
- unsupported or low-trust commands are no longer presented as normal usage

## Developer workflow: dev vs. installed CLI

Current problem:
- `embed-log` on PATH resolves to the pipx-installed global version
- after editing code, running `embed-log` does not reflect local changes
- developers must remember `python3 -m backend.server` to test local changes

### Solution 1: venv overrides pipx (document and rely on PATH order)

No tooling change needed. Make the developer flow explicit:

```bash
source .venv/bin/activate
pip install -e .
which embed-log  # should show .venv/bin/embed-log, not pipx's
```

When the venv is activated, `.venv/bin/` is prepended to PATH and takes precedence.
This already works — developers just need to know it.

Document this in README as **the** developer setup.

### Solution 2: primary dev command is `python3 -m backend.server`

Make this the default documented dev workflow:

```bash
python3 -m backend.server create-config
python3 -m backend.server validate --config embed-log.yml
python3 -m backend.server run --config embed-log.yml
```

Advantages:
- always runs from source, never conflicts with pipx
- no venv activation needed (though deps must be installed)
- works in any terminal

Make this the first / primary dev example in both README and INSTALL docs.

### Solution 3: add `dev` subcommand (optional, lower priority)

A hypothetical `embed-log dev` that detects the repo root and runs from source:

```bash
embed-log dev create-config
```

Implementation sketch:
- looks for a repo marker (e.g. `pyproject.toml`) in parent directories
- runs `sys.executable -m backend.server` from repo root
- useful for quick one-offs without mental context switching

### Recommendation

Use **Solution 1 + Solution 2** immediately (documentation only, no code changes).
Consider **Solution 3** only if developers continue to struggle.

For the CLI help, also add a hint when run outside a venv and no config is found:
```text
Development mode:
  python3 -m backend.server <command>  (runs from source, no pipx install needed)
```

## Session/log inspection commands

A future `sessions` subcommand tree would allow CLI users and scripts/agents to inspect recorded session data on disk without starting the server.

All commands work **without a config file** — they default to `logs/` in the current directory and accept `--log-dir` to point elsewhere.

```bash
embed-log sessions list                         # looks in logs/
embed-log sessions list --log-dir /custom/path
embed-log sessions list --config embed-log.yml   # reads log-dir from config
```

### Proposed command tree

```
embed-log sessions
embed-log sessions list [--log-dir DIR] [--sort date|name] [--limit N] [--json]
embed-log sessions info <session-id> [--log-dir DIR] [--json]
embed-log sessions logs <session-id> [--log-dir DIR] [--pane PANE]
embed-log sessions export <session-id> [--log-dir DIR] [--output FILE]
```

### What each does

**`list`** — scans the configured log directory for session subdirectories, reads each `manifest.json`, and prints a table:

```
ID                          APP       SOURCES   HTML
2026-05-24_10-49-04        demo      3         yes
2026-05-24_10-45-12        prod      2         no
```

**`info`** — prints the full `manifest.json` contents in a readable format:

```yaml
session_id: 2026-05-24_10-49-04
app_name: demo
sources: [SENSOR_A, SENSOR_B, SENSOR_C]
```

With `--json`, raw JSON output (agent-friendly).

**`logs`** — cats or tails a log file for a session. Optionally filter by pane name.

```bash
embed-log sessions logs 2026-05-24_10-49-04 --pane SENSOR_A | head -100
```

**`export`** — regenerates `session.html` for a session, optionally to a different output path.

### Why this matters

- Works offline (no server process)
- Pairs well with `create-config` → `run` → `sessions list` → `sessions export` workflow
- Enables CI pipelines and AI agents to inspect results programmatically
- Reduces need to navigate the filesystem manually

### Design constraints

- Read-only commands by default (no modify/delete)
- `--json` flag on every subcommand for machine-readable output
- Accept the same `--log-dir` flag as `run`, defaulting to `logs/`
- Handle partial/corrupted sessions gracefully

### Implementation order

1. `list`
2. `info`
3. `logs`
4. `export`
