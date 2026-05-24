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
- `slice` is still present even though it is not trusted
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
- `slice` (until removed)
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
  slice           experimental / legacy
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

## slice removal plan

`slic`e is not trusted and should not be part of the user-facing product.

### Short term
- remove `slice` from primary help output
- mark it as experimental/internal if still callable

### Medium term
- remove or replace it with a simpler, trustworthy workflow

### Important
Do not design new user flows around `slice`.

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
- hide/de-emphasize `slice`
- refine `create-config` UX based on real usage
- clean docs so `create-config` is the primary path everywhere

### Phase 3
- add `doctor`
- decide whether direct source CLI flags remain public or become advanced-only
- remove `slice`

---

## Acceptance criteria for the CLI simplification effort

The CLI is simplified when:
- a new user can understand the basic workflow from the first help screen
- `create-config`, `validate`, and `run` are clearly dominant
- advanced/internal features no longer compete with the main path
- docs follow the same mental model as the CLI
- unsupported or low-trust commands are no longer presented as normal usage
