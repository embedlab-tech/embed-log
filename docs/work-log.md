# Work log

Chronological implementation notes for the MVP 1.0 work branch.

## 2026-07-11 10:42:19 UTC / 2026-07-11 12:42:19 CEST (Warsaw)

- **Commit:** `41a29f8` — `Add serial diagnostics to doctor`
- Added repeatable `embed-log doctor --serial <path>` checks.
- `doctor` also inspects UART paths declared in a loaded YAML configuration.
- Reports readable/writable, missing, permission-denied, or unavailable paths in text and JSON output.
- Checks use filesystem access only and do not configure/reset attached serial devices.
- Added CLI/unit coverage for missing serial paths; `cargo test -p embed-log-cli` passed (80 tests).

### File changes (`41a29f8`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `crates/embed-log-cli/src/commands/misc.rs` | 95 | 4 | Serial inspection, JSON/text reporting, and tests. |
| `crates/embed-log-cli/src/main.rs` | 8 | 1 | Repeatable `doctor --serial` CLI argument and dispatch. |
| `docs/cli.md` | 4 | 0 | Serial-doctor usage and safety notes. |

Future entries must include this per-file added/removed-line summary.

## 2026-07-11 10:51:14 UTC / 2026-07-11 12:51:14 CEST (Warsaw)

- **Commit:** `90436be` — `Add Pi work-log checkpoint extension`
- **Task:** Add a project-local Pi extension that snapshots milestone usage and generates structured work-log entries.
- **Validation:** `pi -e .pi/extensions/worklog-checkpoint.ts -p '/worklog-start extension load smoke test'` — passed; checkpoint created.
- **Model-token delta:** unavailable; this extension was introduced after the milestone began, so no before snapshot exists.

### File changes (`90436be`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `.gitignore` | 3 | 0 | Ignores local extension checkpoint state. |
| `.pi/extensions/README.md` | 18 | 0 | Documents extension commands and lifecycle. |
| `.pi/extensions/worklog-checkpoint.ts` | 152 | 0 | Implements start/finish checkpoints, token delta calculation, Git stats, and work-log append. |

## 2026-07-11 10:54:04 UTC / 2026-07-11 12:54:04 CEST (Warsaw)

- **Commit:** `1aff4c6` — `Add milestone work-log skill`
- **Task:** Add an on-demand project skill that standardizes milestone commits, token checkpoints, validation, and work-log entries.
- **Validation:** Reviewed Pi skill frontmatter and explicit skill-loading CLI support (`pi --help`) — passed.
- **Model-token delta:** unavailable; no before checkpoint existed for this task.

### File changes (`1aff4c6`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `.agents/skills/milestone-worklog/SKILL.md` | 67 | 0 | Defines the milestone workflow, extension integration, fallback commands, and guardrails. |
