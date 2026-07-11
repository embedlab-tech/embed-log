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

## 2026-07-11 10:56:24 UTC / 2026-07-11 12:56:24 GMT+2 (Warsaw)

- **Commit:** `c62e800` — `Expose release build diagnostics`
- **Task:** Add release target and executable metadata to embed-log version
- **Started:** 2026-07-11 10:55:38 UTC / 2026-07-11 12:55:38 GMT+2 (Warsaw)
- **Completed:** 2026-07-11 10:56:24 UTC / 2026-07-11 12:56:24 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`c62e800`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `crates/embed-log-cli/build.rs` | 5 | 1 | Embeds the build target triple for runtime diagnostics. |
| `crates/embed-log-cli/src/commands/misc.rs` | 39 | 9 | Adds structured version reporting with target/executable fields and test coverage. |
| `docs/cli.md` | 2 | 0 | Documents release/support diagnostic output. |

## 2026-07-11 11:01:08 UTC / 2026-07-11 13:01:08 GMT+2 (Warsaw)

- **Commit:** `00e4ffb` — `Add release update availability checks`
- **Task:** Add release update availability checks
- **Started:** 2026-07-11 10:59:38 UTC / 2026-07-11 12:59:38 GMT+2 (Warsaw)
- **Completed:** 2026-07-11 11:01:08 UTC / 2026-07-11 13:01:08 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`00e4ffb`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `Cargo.lock` | 275 | 3 | Locks the HTTP/TLS and semantic-version dependencies. |
| `Cargo.toml` | 2 | 0 | Adds workspace HTTP and semantic-version dependencies. |
| `crates/embed-log-cli/Cargo.toml` | 3 | 0 | Enables update-check dependencies for the CLI. |
| `crates/embed-log-cli/src/commands/misc.rs` | 68 | 0 | Fetches the latest GitHub Release and compares semantic versions. |
| `crates/embed-log-cli/src/main.rs` | 11 | 0 | Adds the `update --check [--json]` command surface. |
| `docs/cli.md` | 9 | 0 | Documents update-check usage and current install limitation. |

## 2026-07-11 11:08:39 UTC / 2026-07-11 13:08:39 GMT+2 (Warsaw)

- **Commit:** `64f0000` — `Implement verified self-update installation`
- **Task:** Implement verified self-update installation for release archives
- **Started:** 2026-07-11 11:06:39 UTC / 2026-07-11 13:06:39 GMT+2 (Warsaw)
- **Completed:** 2026-07-11 11:08:39 UTC / 2026-07-11 13:08:39 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`64f0000`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `Cargo.lock` | 24 | 0 | Locks archive extraction and SHA-256 dependencies. |
| `Cargo.toml` | 3 | 0 | Adds shared archive and hash dependencies. |
| `crates/embed-log-cli/Cargo.toml` | 3 | 0 | Enables updater archive and checksum dependencies. |
| `crates/embed-log-cli/src/commands/misc.rs` | 181 | 32 | Downloads release assets, verifies SHA-256, extracts, backs up, and replaces the executable. |
| `crates/embed-log-cli/src/main.rs` | 15 | 4 | Adds version selection and explicit install confirmation flags. |
| `docs/cli.md` | 3 | 1 | Documents check and verified-install update modes. |

## 2026-07-11 11:12:40 UTC / 2026-07-11 13:12:40 GMT+2 (Warsaw)

- **Commit:** `4bc69d2` — `Add isolated updater rollback tests`
- **Task:** Add updater isolation and rollback tests
- **Started:** 2026-07-11 11:11:52 UTC / 2026-07-11 13:11:52 GMT+2 (Warsaw)
- **Completed:** 2026-07-11 11:12:40 UTC / 2026-07-11 13:12:40 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`4bc69d2`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `crates/embed-log-cli/src/commands/misc.rs` | 64 | 10 | Makes release URL selection and file replacement testable; covers swap and rollback behavior. |

## 2026-07-11 11:15:31 UTC / 2026-07-11 13:15:31 GMT+2 (Warsaw)

- **Commit:** `cde8194` — `Harden updater archive validation`
- **Task:** Harden updater downgrade and archive safety
- **Started:** 2026-07-11 11:14:15 UTC / 2026-07-11 13:14:15 GMT+2 (Warsaw)
- **Completed:** 2026-07-11 11:15:31 UTC / 2026-07-11 13:15:31 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`cde8194`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `crates/embed-log-cli/src/commands/misc.rs` | 74 | 14 | Rejects unexpected/duplicate archive entries and adds extraction/replacement safety tests. |
| `crates/embed-log-cli/src/main.rs` | 5 | 1 | Adds explicit `--allow-downgrade` update override. |
| `docs/cli.md` | 2 | 1 | Documents downgrade protection and override usage. |

## 2026-07-11 11:19:31 UTC / 2026-07-11 13:19:31 GMT+2 (Warsaw)

- **Commit:** `fa579ec` — `Add session report open command`
- **Task:** Add sessions open command for exported session reports
- **Started:** 2026-07-11 11:17:08 UTC / 2026-07-11 13:17:08 GMT+2 (Warsaw)
- **Completed:** 2026-07-11 11:19:31 UTC / 2026-07-11 13:19:31 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`fa579ec`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `crates/embed-log-cli/src/commands/sessions.rs` | 24 | 0 | Adds browser opening and on-demand HTML export for a resolved session. |
| `crates/embed-log-cli/src/main.rs` | 1 | 0 | Covers `sessions open latest` CLI parsing. |
| `docs/cli.md` | 6 | 0 | Documents opening an exported session report. |

## 2026-07-11 11:25:08 UTC / 2026-07-11 13:25:08 GMT+2 (Warsaw)

- **Commit:** `9481c91` — `Import external logs into recorded sessions`
- **Task:** Import external UTC-timestamped logs into existing sessions
- **Started:** 2026-07-11 11:21:47 UTC / 2026-07-11 13:21:47 GMT+2 (Warsaw)
- **Completed:** 2026-07-11 11:25:08 UTC / 2026-07-11 13:25:08 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`9481c91`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `crates/embed-log-cli/src/commands/sessions.rs` | 167 | 15 | Adds RFC3339 import parsing, timestamp-sorted combined-log merge, source metadata, and parser tests. |
| `docs/cli.md` | 8 | 0 | Documents importing external RFC3339 timestamped logs. |

## 2026-07-11 11:28:39 UTC / 2026-07-11 13:28:39 GMT+2 (Warsaw)

- **Commit:** `c09c5af` — `Harden session import workflow`
- **Task:** Document non-session roadmap and finish session import reliability
- **Started:** 2026-07-11 11:27:18 UTC / 2026-07-11 13:27:18 GMT+2 (Warsaw)
- **Completed:** 2026-07-11 11:28:39 UTC / 2026-07-11 13:28:39 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`c09c5af`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `crates/embed-log-cli/src/commands/sessions.rs` | 14 | 5 | Makes import rewrites atomic, rejects malformed existing JSONL, and rejects duplicate source names. |
| `docs/non-session-roadmap.md` | 54 | 0 | Separates deferred distribution, UX, TUI, and Zephyr work from session work. |

## 2026-07-11 11:31:57 UTC / 2026-07-11 13:31:57 GMT+2 (Warsaw)

- **Commit:** `4389a04` — `Add session import dry-run mode`
- **Task:** Complete remaining session import, bundle, and retention workflows
- **Started:** 2026-07-11 11:31:22 UTC / 2026-07-11 13:31:22 GMT+2 (Warsaw)
- **Completed:** 2026-07-11 11:31:57 UTC / 2026-07-11 13:31:57 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`4389a04`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `crates/embed-log-cli/src/commands/sessions.rs` | 10 | 2 | Adds a non-mutating timestamp-validation import preview. |
| `docs/cli.md` | 1 | 0 | Documents import dry-run usage. |

## 2026-07-11 11:38:17 UTC / 2026-07-11 13:38:17 GMT+2 (Warsaw)

- **Commit:** `5171174` — `Add session support bundle export`
- **Task:** Add portable session support-bundle export
- **Started:** 2026-07-11 11:37:17 UTC / 2026-07-11 13:37:17 GMT+2 (Warsaw)
- **Completed:** 2026-07-11 11:38:17 UTC / 2026-07-11 13:38:17 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`5171174`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `crates/embed-log-cli/src/commands/sessions.rs` | 60 | 0 | Archives full session artifacts with build diagnostics and bundle coverage. |
| `crates/embed-log-cli/src/main.rs` | 1 | 0 | Covers `sessions bundle latest` CLI parsing. |
| `docs/cli.md` | 7 | 0 | Documents portable support-bundle export. |

## 2026-07-11 11:41:58 UTC / 2026-07-11 13:41:58 GMT+2 (Warsaw)

- **Commit:** `45c48f8` — `Add session retention pruning`
- **Task:** Add session retention pruning with dry-run
- **Started:** 2026-07-11 11:41:04 UTC / 2026-07-11 13:41:04 GMT+2 (Warsaw)
- **Completed:** 2026-07-11 11:41:58 UTC / 2026-07-11 13:41:58 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`45c48f8`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `crates/embed-log-cli/src/commands/sessions.rs` | 61 | 0 | Adds size-reporting dry-run and deletion retention logic with coverage. |
| `crates/embed-log-cli/src/main.rs` | 1 | 0 | Covers prune command parsing. |
| `docs/cli.md` | 7 | 0 | Documents safe session-retention commands. |

## 2026-07-11 11:47:36 UTC / 2026-07-11 13:47:36 GMT+2 (Warsaw)

- **Commit:** `f171b27` — `Add Embed-log get-up-to-speed guide`
- **Task:** Add comprehensive Embed-log get-up-to-speed guide
- **Started:** 2026-07-11 11:46:15 UTC / 2026-07-11 13:46:15 GMT+2 (Warsaw)
- **Completed:** 2026-07-11 11:47:36 UTC / 2026-07-11 13:47:36 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`f171b27`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `README.md` | 1 | 0 | Links users to the comprehensive guide. |
| `docs/getting-up-to-speed.md` | 241 | 0 | Adds end-to-end onboarding, session, automation, advanced-source, and update guidance. |
| `docs/index.md` | 1 | 0 | Adds the guide to the documentation map. |

## 2026-07-11 12:06:43 UTC / 2026-07-11 14:06:43 GMT+2 (Warsaw)

- **Commit:** `bc5bdd8` — `Guide Windows users to supported update paths`
- **Task:** Add Windows PowerShell installation support
- **Started:** 2026-07-11 12:05:29 UTC / 2026-07-11 14:05:29 GMT+2 (Warsaw)
- **Completed:** 2026-07-11 12:06:43 UTC / 2026-07-11 14:06:43 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`bc5bdd8`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `crates/embed-log-cli/src/commands/misc.rs` | 19 | 1 | Makes Windows update requests return actionable PowerShell/package-manager guidance. |
| `docs/cli.md` | 1 | 1 | Documents that Windows self-replacement is intentionally deferred. |

## 2026-07-11 23:32:57 UTC / 2026-07-12 01:32:57 GMT+2 (Warsaw)

- **Commit:** `1a05fae` — `Show elapsed time between timeline events`
- **Task:** Add event timeline delta-time tooltips with Playwright coverage
- **Started:** 2026-07-11 23:30:28 UTC / 2026-07-12 01:30:28 GMT+2 (Warsaw)
- **Completed:** 2026-07-11 23:32:57 UTC / 2026-07-12 01:32:57 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`1a05fae`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `frontend/events.js` | 36 | 0 | Calculates and displays prior-event and prior-same-rule elapsed durations. |
| `frontend/viewer.css` | 2 | 0 | Styles elapsed-time details in event tooltips. |
| `tests-ui/regression-tests/events.spec.js` | 26 | 0 | Verifies recurring selected events display both delta values. |
