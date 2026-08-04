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

## 2026-07-11 23:38:40 UTC / 2026-07-12 01:38:40 GMT+2 (Warsaw)

- **Commit:** `76e41be` — `Clarify event timeline lanes and hover behavior`
- **Task:** Improve event tooltip dismissal and source-qualified lanes
- **Started:** 2026-07-11 23:37:35 UTC / 2026-07-12 01:37:35 GMT+2 (Warsaw)
- **Completed:** 2026-07-11 23:38:40 UTC / 2026-07-12 01:38:40 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`76e41be`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `frontend/events.js` | 19 | 6 | Qualifies event lanes by source/rule and shortens hover-tooltip dismissal. |
| `tests-ui/regression-tests/events.spec.js` | 27 | 0 | Covers source-qualified lane labels and prompt hover-tooltip hiding. |

## 2026-07-11 23:41:32 UTC / 2026-07-12 01:41:32 GMT+2 (Warsaw)

- **Commit:** `8bdac4d` — `Align event timestamps with display mode`
- **Task:** Align event tooltip timestamps with display mode
- **Started:** 2026-07-11 23:39:25 UTC / 2026-07-12 01:39:25 GMT+2 (Warsaw)
- **Completed:** 2026-07-11 23:41:32 UTC / 2026-07-12 01:41:32 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`8bdac4d`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `frontend/events.js` | 8 | 1 | Renders event tooltip timestamps in the active absolute/relative display mode. |
| `tests-ui/regression-tests/events.spec.js` | 25 | 0 | Verifies event tooltip timestamps switch with the UI setting. |

## 2026-07-11 23:43:00 UTC / 2026-07-12 01:43:00 GMT+2 (Warsaw)

- **Commit:** `40fcf64` — `Order events chronologically and document agent plan`
- **Task:** Order event timeline interactions chronologically and publish automation plan
- **Started:** 2026-07-11 23:42:16 UTC / 2026-07-12 01:42:16 GMT+2 (Warsaw)
- **Completed:** 2026-07-11 23:43:00 UTC / 2026-07-12 01:43:00 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`40fcf64`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `docs/automation-agent-plan.md` | 202 | 0 | Adds phased design for agent investigation, dynamic rules, and protocol discovery. |
| `docs/index.md` | 1 | 0 | Links the automation and agent roadmap from the documentation map. |
| `frontend/events.js` | 13 | 6 | Uses one timestamp-sorted event order for rendered interactions and comparisons. |
| `tests-ui/regression-tests/events.spec.js` | 12 | 0 | Verifies timeline dots are emitted in chronological timestamp order. |

## 2026-07-11 23:45:05 UTC / 2026-07-12 01:45:05 GMT+2 (Warsaw)

- **Commit:** `45d214a` — `Keep event filters aligned with timeline data`
- **Task:** Derive event filters from recorded events and rules
- **Started:** 2026-07-11 23:44:11 UTC / 2026-07-12 01:44:11 GMT+2 (Warsaw)
- **Completed:** 2026-07-11 23:45:05 UTC / 2026-07-12 01:45:05 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`45d214a`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `frontend/events.js` | 27 | 7 | Builds filter chips from both configured rules and currently recorded event data. |
| `tests-ui/regression-tests/events.spec.js` | 18 | 0 | Verifies each timeline source and severity is filterable. |

## 2026-07-11 23:47:11 UTC / 2026-07-12 01:47:11 GMT+2 (Warsaw)

- **Commit:** `0da7204` — `Improve event timeline accessibility`
- **Task:** Finish remaining frontend event usability improvements
- **Started:** 2026-07-11 23:46:07 UTC / 2026-07-12 01:46:07 GMT+2 (Warsaw)
- **Completed:** 2026-07-11 23:47:11 UTC / 2026-07-12 01:47:11 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`0da7204`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `frontend/events.js` | 20 | 2 | Activates Events from received events, suppresses duplicate deltas, and adds keyboard-accessible dots. |
| `tests-ui/regression-tests/events.spec.js` | 15 | 1 | Covers keyboard selection and updated recurring-event tooltip behavior. |

## 2026-07-11 23:54:24 UTC / 2026-07-12 01:54:24 GMT+2 (Warsaw)

- **Commit:** `2dae0a3` — `Add runtime event rule control API`
- **Task:** Add runtime event-rule matching and control API
- **Started:** 2026-07-11 23:50:21 UTC / 2026-07-12 01:50:21 GMT+2 (Warsaw)
- **Completed:** 2026-07-11 23:54:24 UTC / 2026-07-12 01:54:24 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`2dae0a3`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `crates/embed-log-core/src/net/control_ws.rs` | 115 | 0 | Adds validated runtime event-rule create/list/delete commands and CRUD coverage. |
| `crates/embed-log-core/src/net/ws_server.rs` | 4 | 1 | Stores the shared runtime event-rule registry in server state. |
| `crates/embed-log-core/src/runtime/server.rs` | 23 | 4 | Matches runtime rules in source writers through the existing event persistence path. |

## 2026-07-11 23:58:55 UTC / 2026-07-12 01:58:55 GMT+2 (Warsaw)

- **Commit:** `8ffa5de` — `Create runtime event rules from selected logs`
- **Task:** Add selection-based runtime event rule creation
- **Started:** 2026-07-11 23:58:49 UTC / 2026-07-12 01:58:49 GMT+2 (Warsaw)
- **Completed:** 2026-07-11 23:58:55 UTC / 2026-07-12 01:58:55 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`8ffa5de`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `crates/embed-log-core/src/net/control_ws.rs` | 3 | 3 | Exposes runtime event-rule handlers to the browser WebSocket server. |
| `crates/embed-log-core/src/net/ws_server.rs` | 7 | 1 | Routes browser event-rule CRUD commands through the shared handlers. |
| `frontend/selection.js` | 32 | 0 | Adds a selected-line action that prompts for and submits a runtime event rule. |

## 2026-07-12 08:06:42 UTC / 2026-07-12 10:06:42 GMT+2 (Warsaw)

- **Commit:** `b6d9628` — `Expose static and runtime event rules together`
- **Task:** Add event rules manager preview export and promotion
- **Started:** 2026-07-12 00:02:29 UTC / 2026-07-12 02:02:29 GMT+2 (Warsaw)
- **Completed:** 2026-07-12 08:06:42 UTC / 2026-07-12 10:06:42 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`b6d9628`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `crates/embed-log-core/src/net/control_ws.rs` | 13 | 6 | Returns unified full-detail static and runtime rule records. |
| `crates/embed-log-core/src/net/ws_server.rs` | 3 | 0 | Stores static event rules in shared server state. |
| `crates/embed-log-core/src/runtime/server.rs` | 4 | 0 | Preserves loaded static rules for runtime API discovery. |

## 2026-07-12 08:09:54 UTC / 2026-07-12 10:09:54 GMT+2 (Warsaw)

- **Commit:** `54152ea` — `Export active event rules as YAML`
- **Task:** Export active event rules as companion YAML
- **Started:** 2026-07-12 08:08:19 UTC / 2026-07-12 10:08:19 GMT+2 (Warsaw)
- **Completed:** 2026-07-12 08:09:54 UTC / 2026-07-12 10:09:54 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`54152ea`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `crates/embed-log-core/src/net/control_ws.rs` | 36 | 1 | Serializes unified active rules into companion YAML for export. |
| `crates/embed-log-core/src/net/ws_server.rs` | 2 | 1 | Routes YAML export requests through the browser WebSocket. |

## 2026-07-12 08:42:43 UTC / 2026-07-12 10:42:43 GMT+2 (Warsaw)

- **Commit:** `0690b59` — `Add event rules manager panel`
- **Task:** Add event rules manager panel
- **Started:** 2026-07-12 08:42:43 UTC / 2026-07-12 10:42:43 GMT+2 (Warsaw)
- **Completed:** 2026-07-12 08:42:43 UTC / 2026-07-12 10:42:43 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`0690b59`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `frontend/events.js` | 45 | 1 | Adds active-rules panel, runtime deletion, and YAML download actions. |
| `frontend/viewer.css` | 6 | 0 | Styles rule-manager rows and actions. |
| `frontend/ws.js` | 5 | 0 | Forwards event-rule protocol responses to the UI. |

## 2026-07-12 08:51:59 UTC / 2026-07-12 10:51:59 GMT+2 (Warsaw)

- **Commit:** `ca89763` — `Promote event rules and prepare 1.0 release`
- **Task:** Promote runtime event rules into companion YAML
- **Started:** 2026-07-12 08:44:31 UTC / 2026-07-12 10:44:31 GMT+2 (Warsaw)
- **Completed:** 2026-07-12 08:51:59 UTC / 2026-07-12 10:51:59 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`ca89763`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `Cargo.lock` | 4 | 4 | Updates workspace package metadata for version 1.0.0. |
| `Cargo.toml` | 1 | 1 | Bumps the workspace release version to 1.0.0. |
| `crates/embed-log-core/src/net/control_ws.rs` | 26 | 0 | Adds duplicate-safe atomic promotion of runtime rules into companion YAML. |
| `crates/embed-log-core/src/net/ws_server.rs` | 5 | 1 | Routes promotion requests from the browser WebSocket. |
| `crates/embed-log-core/src/runtime/server.rs` | 4 | 0 | Supplies the preferred companion event-rule path to server state. |

## 2026-07-12 09:23:46 UTC / 2026-07-12 11:23:46 GMT+2 (Warsaw)

- **Commit:** `a84b302` — `Simplify event rule creation workflow`
- **Task:** Simplify event rule creation UX
- **Started:** 2026-07-12 09:22:47 UTC / 2026-07-12 11:22:47 GMT+2 (Warsaw)
- **Completed:** 2026-07-12 09:23:46 UTC / 2026-07-12 11:23:46 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`a84b302`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `frontend/selection.js` | 10 | 9 | Replaces technical prompts with one-click natural-language pattern watching. |

## 2026-07-12 09:29:05 UTC / 2026-07-12 11:29:05 GMT+2 (Warsaw)

- **Commit:** `dd84644` — `Add save-for-future-runs event action`
- **Task:** Add save-for-future-runs event rule action
- **Started:** 2026-07-12 09:28:10 UTC / 2026-07-12 11:28:10 GMT+2 (Warsaw)
- **Completed:** 2026-07-12 09:29:05 UTC / 2026-07-12 11:29:05 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`dd84644`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `frontend/events.js` | 11 | 2 | Adds natural promotion and stop-watching actions with user-facing save feedback. |

## 2026-07-12 09:32:55 UTC / 2026-07-12 11:32:55 GMT+2 (Warsaw)

- **Commit:** `9c1353d` — `Use natural language in event rules panel`
- **Task:** Use natural language in event rules panel
- **Started:** 2026-07-12 09:32:07 UTC / 2026-07-12 11:32:07 GMT+2 (Warsaw)
- **Completed:** 2026-07-12 09:32:55 UTC / 2026-07-12 11:32:55 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`9c1353d`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `frontend/events.js` | 5 | 1 | Replaces static/runtime jargon with saved/watching status labels. |

## 2026-07-12 09:35:28 UTC / 2026-07-12 11:35:28 GMT+2 (Warsaw)

- **Commit:** `b4cc1f1` — `Test runtime event rule promotion persistence`
- **Task:** Test runtime event rule promotion persistence
- **Started:** 2026-07-12 09:33:56 UTC / 2026-07-12 11:33:56 GMT+2 (Warsaw)
- **Completed:** 2026-07-12 09:35:28 UTC / 2026-07-12 11:35:28 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`b4cc1f1`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `crates/embed-log-core/src/net/control_ws.rs` | 25 | 0 | Covers companion YAML creation, duplicate rejection, and staged-file cleanup. |

## 2026-07-12 09:37:27 UTC / 2026-07-12 11:37:27 GMT+2 (Warsaw)

- **Commit:** `504bf95` — `Test event rules panel export workflow`
- **Task:** Add Playwright coverage for event rules panel
- **Started:** 2026-07-12 09:36:47 UTC / 2026-07-12 11:36:47 GMT+2 (Warsaw)
- **Completed:** 2026-07-12 09:37:27 UTC / 2026-07-12 11:37:27 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`504bf95`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `tests-ui/regression-tests/events.spec.js` | 15 | 0 | Covers rules-panel loading, saved-rule wording, and companion-file download. |

## 2026-07-12 09:41:25 UTC / 2026-07-12 11:41:25 GMT+2 (Warsaw)

- **Commit:** `88927ae` — `Prepare 1.0.0 release candidate`
- **Task:** Format and install local 1.0.0 release candidate
- **Started:** 2026-07-12 09:40:26 UTC / 2026-07-12 11:40:26 GMT+2 (Warsaw)
- **Completed:** 2026-07-12 09:41:25 UTC / 2026-07-12 11:41:25 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`88927ae`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `.github/workflows/release-cli.yml` | 1 | 1 | Rustfmt/release-preparation change; see implementation commit. |
| `crates/embed-log-cli/src/commands/sessions.rs` | 75 | 24 | Rustfmt/release-preparation change; see implementation commit. |
| `crates/embed-log-cli/src/main.rs` | 9 | 1 | Rustfmt/release-preparation change; see implementation commit. |
| `crates/embed-log-core/src/config/loader.rs` | 4 | 1 | Rustfmt/release-preparation change; see implementation commit. |
| `crates/embed-log-core/src/config/paths.rs` | 1 | 4 | Rustfmt/release-preparation change; see implementation commit. |
| `crates/embed-log-core/src/net/control_ws.rs` | 306 | 55 | Rustfmt/release-preparation change; see implementation commit. |
| `crates/embed-log-core/src/net/ws_server.rs` | 28 | 8 | Rustfmt/release-preparation change; see implementation commit. |
| `crates/embed-log-core/src/parsers/slip_coap.rs` | 23 | 5 | Rustfmt/release-preparation change; see implementation commit. |
| `crates/embed-log-core/src/parsers/zephyr_dict.rs` | 93 | 35 | Rustfmt/release-preparation change; see implementation commit. |
| `crates/embed-log-core/src/postprocess.rs` | 7 | 6 | Rustfmt/release-preparation change; see implementation commit. |
| `crates/embed-log-core/src/runtime/server.rs` | 66 | 52 | Rustfmt/release-preparation change; see implementation commit. |
| `crates/embed-log-tauri/tauri.conf.json` | 1 | 1 | Rustfmt/release-preparation change; see implementation commit. |
| `docs/releasing.md` | 5 | 5 | Rustfmt/release-preparation change; see implementation commit. |

## 2026-07-12 14:08:14 UTC / 2026-07-12 16:08:14 GMT+2 (Warsaw)

- **Commit:** `89964a8` — `Add REST status capabilities endpoint`
- **Task:** Add REST status capabilities endpoint
- **Started:** 2026-07-12 14:05:16 UTC / 2026-07-12 16:05:16 GMT+2 (Warsaw)
- **Completed:** 2026-07-12 14:08:14 UTC / 2026-07-12 16:08:14 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`89964a8`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `crates/embed-log-core/src/net/ws_server.rs` | 35 | 0 | Adds REST readiness, version, session, source-capability, and stats discovery. |
| `docs/api-status.md` | 59 | 0 | Documents the status endpoint schema and orchestration usage. |
| `docs/index.md` | 1 | 0 | Links the new status API reference. |

## 2026-07-12 14:32:09 UTC / 2026-07-12 16:32:09 GMT+2 (Warsaw)

- **Commit:** `d2cf55c` — `Document ready agent capabilities`
- **Task:** Document ready-to-use agent capabilities
- **Started:** 2026-07-12 14:31:20 UTC / 2026-07-12 16:31:20 GMT+2 (Warsaw)
- **Completed:** 2026-07-12 14:32:09 UTC / 2026-07-12 16:32:09 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`d2cf55c`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `docs/agent-capabilities.md` | 135 | 0 | Documents available session, status, live-event, and event-rule agent workflows. |
| `docs/index.md` | 1 | 0 | Links the ready-to-use agent reference. |

## 2026-07-13 16:59:16 UTC / 2026-07-13 18:59:16 GMT+2 (Warsaw)

- **Commit:** `dc040a1` — `Fix updater repository and isolate E2E UDP ports`
- **Task:** Fix Rust demo UDP browser E2E delivery
- **Started:** 2026-07-13 16:55:07 UTC / 2026-07-13 18:55:07 GMT+2 (Warsaw)
- **Completed:** 2026-07-13 16:59:16 UTC / 2026-07-13 18:59:16 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`dc040a1`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `crates/embed-log-cli/src/commands/misc.rs` | 1 | 1 | Points self-update release lookup at the actual origin repository. |
| `tests-ui/rust-demo-server.mjs` | 3 | 3 | Moves E2E UDP sources to an isolated port range. |
| `tests-ui/tests/rust-demo.spec.js` | 6 | 6 | Sends E2E fixtures to the isolated test ports. |

## 2026-07-13 17:24:51 UTC / 2026-07-13 19:24:51 GMT+2 (Warsaw)

- **Commit:** `44e2aa6` — `Add STM hardware integration workflow template`
- **Task:** Add hardware integration workflow template
- **Started:** 2026-07-13 17:23:18 UTC / 2026-07-13 19:23:18 GMT+2 (Warsaw)
- **Completed:** 2026-07-13 17:24:51 UTC / 2026-07-13 19:24:51 GMT+2 (Warsaw)
- **Model-token delta:** ~0 (input: ~0, output: ~0, cache read: ~0, cache write: ~0)

### File changes (`44e2aa6`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `.github/workflows/hardware-integration.yml` | 102 | 0 | Adds hosted artifact build and serialized STM-lab hardware validation workflow. |
| `docs/hardware-ci.md` | 46 | 0 | Documents runner labels, variables, operation, and hardware-runner security. |
| `docs/index.md` | 1 | 0 | Links the hardware CI guide. |

## 2026-07-13 17:50:55 UTC / 2026-07-13 19:50:55 CEST (Warsaw)

- **Commit:** `a3396d2` — `Add STM32G0 multi-UART hardware integration test`
- **Task:** Wire the STM32G0/FT4232H rig into the hardware workflow with four UART sources and Python UDP forwarding.
- **Started:** unavailable; no `/worklog-start` checkpoint was recorded.
- **Completed:** 2026-07-13 17:50:55 UTC / 2026-07-13 19:50:55 CEST (+0200) (Warsaw)
- **Validation:** `PATH=/tmp/embed-log-hw-package/bin:$PATH EMBED_LOG_STM32G0_HARDWARE=1 EMBED_LOG_STM32G0_ARTIFACT_DIR=/tmp/embed-log-stm32g0-artifacts /tmp/embed-log-hw-venv/bin/python -m pytest sdk/python/tests/test_backend_hardware_stm32g0_multi_uart.py -q` — passed (1 passed); `cd /home/krezo/Programming/embed-sandbox && just verify-multi-uart` — passed (USART1: 314, USART3: 192, USART4: 129 matching payloads).
- **Model-token delta:** unavailable; no before checkpoint exists.

### File changes (`a3396d2`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `.github/workflows/hardware-integration.yml` | 37 | 13 | Configures stable four-UART variables, pinned firmware flash/preflight, exact package testing, and capture upload. |
| `docs/hardware-ci.md` | 19 | 15 | Documents the STM32G0 rig, required variables, pinned sandbox checkout, and test flow. |
| `sdk/python/tests/test_backend_hardware_stm32g0_multi_uart.py` | 296 | 0 | Adds gated four-UART control, source-isolation, session, and Python UDP-forwarding coverage. |

## 2026-07-13 17:58:08 UTC / 2026-07-13 19:58:08 CEST (Warsaw)

- **Commit:** `9c81362` — `Exercise STM32G0 mixed-baud UART traffic`
- **Task:** Exercise the STM32G0 hardware integration with 115200, 460800, and 1000000 baud generator streams and higher traffic volume.
- **Started:** unavailable; no `/worklog-start` checkpoint was recorded.
- **Completed:** 2026-07-13 17:58:08 UTC / 2026-07-13 19:58:08 CEST (+0200) (Warsaw)
- **Validation:** `PATH=/tmp/embed-log-hw-package/bin:$PATH EMBED_LOG_STM32G0_HARDWARE=1 EMBED_LOG_STM32G0_ARTIFACT_DIR=/tmp/embed-log-stm32g0-high-baud-artifacts /tmp/embed-log-hw-venv/bin/python -m pytest sdk/python/tests/test_backend_hardware_stm32g0_multi_uart.py -q` — passed (1 passed). Captures contained 689 USART1, 596 USART3, 500 USART4, and 1782 forwarded UDP records.
- **Model-token delta:** unavailable; no before checkpoint exists.

### File changes (`9c81362`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `docs/hardware-ci.md` | 1 | 1 | Documents the mixed-baud profile and minimum 500-record capture. |
| `sdk/python/tests/test_backend_hardware_stm32g0_multi_uart.py` | 32 | 16 | Configures per-source baud profiles, increases traffic, and restores 115200 teardown state. |

## 2026-07-13 18:00:55 UTC / 2026-07-13 20:00:55 CEST (Warsaw)

- **Commit:** `4e29f40` — `Run hardware CI against pre-flashed STM32G0 rig`
- **Task:** Make hardware CI run the mixed-baud test against a connected, pre-flashed STM32G0 rig without sandbox firmware setup.
- **Started:** unavailable; no `/worklog-start` checkpoint was recorded.
- **Completed:** 2026-07-13 18:00:55 UTC / 2026-07-13 20:00:55 CEST (+0200) (Warsaw)
- **Validation:** `PATH=/tmp/embed-log-hw-package/bin:$PATH EMBED_LOG_STM32G0_HARDWARE=1 EMBED_LOG_STM32G0_ARTIFACT_DIR=/tmp/embed-log-stm32g0-ci-artifacts /tmp/embed-log-hw-venv/bin/python -m pytest sdk/python/tests/test_backend_hardware_stm32g0_multi_uart.py -q` — passed (1 passed); workflow YAML parsed with the pre-flashed-rig job shape.
- **Model-token delta:** unavailable; no before checkpoint exists.

### File changes (`4e29f40`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `.github/workflows/hardware-integration.yml` | 8 | 25 | Uses verified by-id defaults and removes firmware flashing/preflight. |
| `docs/hardware-ci.md` | 4 | 6 | Documents the connected pre-flashed rig workflow and optional overrides. |

## 2026-07-13 18:04:31 UTC / 2026-07-13 20:04:31 CEST (Warsaw)

- **Commit:** `91f3408` — `Allow UDP datagram loss in hardware forwarding test`
- **Task:** Make high-rate UDP forwarding validation reflect UDP datagram delivery semantics.
- **Started:** unavailable; no `/worklog-start` checkpoint was recorded.
- **Completed:** 2026-07-13 18:04:31 UTC / 2026-07-13 20:04:31 CEST (+0200) (Warsaw)
- **Validation:** `PATH=/tmp/embed-log-ci-package/bin:$PATH EMBED_LOG_STM32G0_HARDWARE=1 EMBED_LOG_STM32G0_ARTIFACT_DIR=/tmp/embed-log-stm32g0-push-verify /tmp/embed-log-hw-venv/bin/python -m pytest sdk/python/tests/test_backend_hardware_stm32g0_multi_uart.py -q` — passed (1 passed). Artifacts contain 689 USART1, 596 USART3, 500 USART4, and 1759 forwarded UDP records.
- **Model-token delta:** unavailable; no before checkpoint exists.

### File changes (`91f3408`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `sdk/python/tests/test_backend_hardware_stm32g0_multi_uart.py` | 9 | 1 | Requires minimum, ordered, unique UDP deliveries while retaining contiguous UART checks. |

## 2026-07-13 18:12:08 UTC / 2026-07-13 20:12:08 CEST (Warsaw)

- **Commit:** `8f05923` — `Run full validation locally on STM lab runner`
- **Task:** Run build, unit, Python integration, Playwright, and STM hardware validation locally on the trusted lab runner; omit Tauri Linux temporarily.
- **Started:** unavailable; no `/worklog-start` checkpoint was recorded.
- **Completed:** 2026-07-13 18:12:08 UTC / 2026-07-13 20:12:08 CEST (+0200) (Warsaw)
- **Validation:** `cargo test --locked --package embed-log-core --package embed-log-cli` — passed (315 tests); `python -m pytest sdk/python/tests -q --ignore=sdk/python/tests/test_backend_hardware_uart.py --ignore=sdk/python/tests/test_backend_hardware_stm32g0_multi_uart.py` — passed (53 tests); `npm --prefix tests-ui run test:e2e` — passed (4 tests); `npm --prefix tests-ui run test:regression` — passed (80 tests, 4 skipped).
- **Model-token delta:** unavailable; no before checkpoint exists.

### File changes (`8f05923`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `.github/workflows/ci.yml` | 3 | 1 | Disables the Tauri Linux job pending runner dependencies. |
| `.github/workflows/hardware-integration.yml` | 59 | 45 | Replaces hosted packaging with one local STM-lab build, unit, integration, Playwright, and hardware flow. |
| `crates/embed-log-cli/src/commands/misc.rs` | 2 | 2 | Aligns release URL test expectations with the configured repository. |
| `docs/hardware-ci.md` | 9 | 8 | Documents local runner validation order and branch trigger. |
| `sdk/python/tests/test_e2e.py` | 2 | 2 | Aligns PTY expectation with Zephyr-shell CR TX normalization. |

## 2026-07-13 18:18:17 UTC / 2026-07-13 20:18:17 CEST (Warsaw)

- **Commit:** `5c8f46d` — `Fix CI lint and installed binary cleanup`
- **Task:** Fix failures reported by the CI unit-test and installed-binary jobs.
- **Started:** unavailable; no `/worklog-start` checkpoint was recorded.
- **Completed:** 2026-07-13 18:18:17 UTC / 2026-07-13 20:18:17 CEST (+0200) (Warsaw)
- **Validation:** `cargo clippy --locked --package embed-log-core --package embed-log-cli --all-targets -- -D warnings` — passed; `cargo test --locked --package embed-log-core --package embed-log-cli` — passed (315 tests); installed-binary cleanup workflow shape verified.
- **Model-token delta:** unavailable; no before checkpoint exists.

### File changes (`5c8f46d`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `.github/workflows/ci.yml` | 2 | 5 | Cleans up only the job-installed CLI instead of rejecting unrelated PATH entries. |
| `crates/embed-log-cli/src/commands/misc.rs` | 2 | 2 | Uses iterator idiom in archive extraction. |
| `crates/embed-log-cli/src/commands/run.rs` | 3 | 0 | Documents the intentional quick-run argument shape for Clippy. |
| `crates/embed-log-core/src/config/loader.rs` | 2 | 2 | Removes needless generic-argument borrows. |
| `crates/embed-log-core/src/parsers/zephyr_dict.rs` | 9 | 13 | Applies Clippy-safe byte slicing, matching, and vector initialization. |
| `crates/embed-log-core/src/session/log_parse.rs` | 5 | 8 | Uses a `while let` prefix-stripping loop. |
| `crates/embed-log-core/src/sources/network.rs` | 3 | 0 | Documents the config-shaped capture constructor and its argument allowance. |
| `crates/embed-log-tui/src/draw.rs` | 2 | 2 | Uses explicit size clamping for the help overlay. |

## 2026-07-13 18:28:00 UTC / 2026-07-13 20:28:00 CEST (Warsaw)

- **Commit:** `799c2b9` — `Move STM32G0 hardware test into CI workflow`
- **Task:** Consolidate hardware validation into the regular CI workflow and prevent successful skips or stale capture reuse.
- **Started:** unavailable; no `/worklog-start` checkpoint was recorded.
- **Completed:** 2026-07-13 18:28:00 UTC / 2026-07-13 20:28:00 CEST (+0200) (Warsaw)
- **Validation:** `PATH=/tmp/embed-log-ci-package/bin:$PATH EMBED_LOG_STM32G0_HARDWARE=1 EMBED_LOG_STM32G0_ARTIFACT_DIR=/tmp/embed-log-stm32g0-single-ci /tmp/embed-log-hw-venv/bin/python -m pytest sdk/python/tests/test_backend_hardware_stm32g0_multi_uart.py -q` — passed (1 passed); resulting artifact directory contained exactly one session.
- **Model-token delta:** unavailable; no before checkpoint exists.

### File changes (`799c2b9`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `.github/workflows/ci.yml` | 38 | 9 | Replaces the skipped legacy UART job with serialized STM32G0 hardware validation. |
| `.github/workflows/hardware-integration.yml` | 0 | 123 | Removes the redundant standalone hardware workflow. |
| `docs/hardware-ci.md` | 15 | 16 | Documents the CI-integrated hardware job. |
| `sdk/python/tests/test_backend_hardware_stm32g0_multi_uart.py` | 3 | 1 | Fails absent enabled hardware paths and clears configured captures before each run. |

## 2026-07-13 18:29:25 UTC / 2026-07-13 20:29:25 CEST (Warsaw)

- **Commit:** `b7c6b03` — `Verify packaged CLI before hardware test`
- **Task:** Ensure the CI hardware test runs the exact packaged CLI rather than an arbitrary runner installation.
- **Started:** unavailable; no `/worklog-start` checkpoint was recorded.
- **Completed:** 2026-07-13 18:29:25 UTC / 2026-07-13 20:29:25 CEST (+0200) (Warsaw)
- **Validation:** configured package-path check plus `embed-log version --json` — passed; `EMBED_LOG_HARDWARE_BINARY=/tmp/embed-log-ci-package/bin/embed-log ... pytest sdk/python/tests/test_backend_hardware_stm32g0_multi_uart.py -q` — passed (1 passed).
- **Model-token delta:** unavailable; no before checkpoint exists.

### File changes (`b7c6b03`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `.github/workflows/ci.yml` | 10 | 0 | Pins the hardware harness to the downloaded package and verifies PATH/version before pytest. |
| `sdk/python/tests/test_backend_hardware_stm32g0_multi_uart.py` | 8 | 0 | Honors and validates an explicitly configured hardware-test binary path. |

## 2026-07-13 18:51:56 UTC / 2026-07-13 20:51:56 CEST (Warsaw)

- **Commit:** `402193b` — `Use registered runner labels for hardware CI`
- **Task:** Match the hardware CI job to the labels actually registered by the dedicated embed-log runner.
- **Started:** unavailable; no `/worklog-start` checkpoint was recorded.
- **Completed:** 2026-07-13 18:51:56 UTC / 2026-07-13 20:51:56 CEST (+0200) (Warsaw)
- **Validation:** inspected the active `embed-log-runner` service and successful CI jobs, both using `self-hosted, Linux`; workflow YAML assertion confirmed the hardware job uses those labels.
- **Model-token delta:** unavailable; no before checkpoint exists.

### File changes (`402193b`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `.github/workflows/ci.yml` | 1 | 1 | Removes unavailable `stm-lab` label from the hardware job. |
| `docs/hardware-ci.md` | 2 | 3 | Documents the runner's registered labels and dedicated-rig role. |

## 2026-07-13 21:08:45 UTC / 2026-07-13 23:08:45 CEST (Warsaw)

- **Commit:** `1499d1d` — `Add UDP delivery headroom to hardware test`
- **Task:** Make the high-rate hardware test resilient to expected loopback UDP datagram loss while retaining the 500-record delivery requirement.
- **Started:** unavailable; no `/worklog-start` checkpoint was recorded.
- **Completed:** 2026-07-13 21:08:45 UTC / 2026-07-13 23:08:45 CEST (+0200) (Warsaw)
- **Validation:** `EMBED_LOG_HARDWARE_BINARY=/tmp/embed-log-ci-package/bin/embed-log EMBED_LOG_STM32G0_HARDWARE=1 EMBED_LOG_STM32G0_ARTIFACT_DIR=/tmp/embed-log-stm32g0-udp-headroom ... pytest sdk/python/tests/test_backend_hardware_stm32g0_multi_uart.py -q` — passed (1 passed). Captures contained 740 USART1, 647 USART3, 551 USART4, and 1934 forwarded UDP records.
- **Model-token delta:** unavailable; no before checkpoint exists.

### File changes (`1499d1d`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `docs/hardware-ci.md` | 1 | 1 | Documents UDP headroom and the retained delivery threshold. |
| `sdk/python/tests/test_backend_hardware_stm32g0_multi_uart.py` | 5 | 3 | Captures 550 records and resolves configured artifact directories. |

## 2026-07-13 21:58:23 UTC / 2026-07-13 23:58:23 CEST (Warsaw)

- **Commit:** `89f6d37` — `Add installed CLI TUI integration tests`
- **Task:** Add a separate CI job that validates the installed CLI TUI, tab cycling, pane synchronization, UDP log persistence, and clean interactive exit.
- **Started:** unavailable; no `/worklog-start` checkpoint was recorded.
- **Completed:** 2026-07-13 21:58:23 UTC / 2026-07-13 23:58:23 CEST (+0200) (Warsaw)
- **Validation:** `cargo test --locked --package embed-log-tui` — passed (74 tests); `python scripts/test_tui_integration.py --binary /tmp/embed-log-ci-package/bin/embed-log` — passed (installed CLI connected, switched tabs, persisted both UDP sources, and quit cleanly).
- **Model-token delta:** unavailable; no before checkpoint exists.

### File changes (`89f6d37`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `.github/workflows/ci.yml` | 58 | 0 | Adds a package-dependent, self-hosted TUI integration job that verifies the installed CLI. |
| `crates/embed-log-tui/src/keys.rs` | 36 | 0 | Tests wrapping tab navigation and active-pane reset. |
| `crates/embed-log-tui/src/state.rs` | 37 | 0 | Tests timestamp synchronization across the active tab's panes. |
| `docs/development.md` | 2 | 0 | Documents local TUI test commands. |
| `scripts/test_tui_integration.py` | 145 | 0 | Runs a PTY-installed-CLI TUI integration scenario with two UDP tabs. |

## 2026-07-13 22:03:16 UTC / 2026-07-14 00:03:16 CEST (Warsaw)

- **Commit:** `44f8e61` — `Add STM32G0 TUI hardware integration job`
- **Task:** Run the installed CLI TUI against the real STM32G0 rig after the regular UART hardware job, with shared simulated/real backends in the TUI harness.
- **Started:** unavailable; no `/worklog-start` checkpoint was recorded.
- **Completed:** 2026-07-13 22:03:16 UTC / 2026-07-14 00:03:16 CEST (+0200) (Warsaw)
- **Validation:** `python scripts/test_tui_integration.py --binary /tmp/embed-log-ci-package/bin/embed-log --backend stm32g0 --artifact-dir /tmp/tui-stm32g0-final` — passed; `cargo test --locked --package embed-log-tui` — passed (74 tests).
- **Model-token delta:** unavailable; no before checkpoint exists.

### File changes (`44f8e61`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `.github/workflows/ci.yml` | 78 | 0 | Adds a package-pinned TUI hardware job after UART hardware validation. |
| `docs/hardware-ci.md` | 3 | 2 | Documents the sequential TUI hardware validation and its captures. |
| `scripts/test_tui_integration.py` | 228 | 85 | Adds PTY UART simulation and STM32G0 backends, TUI TX shell control, and post-reset counter validation. |

## 2026-07-13 22:07:50 UTC / 2026-07-14 00:07:50 CEST (Warsaw)

- **Commit:** `b7cac46` — `Verify CONTROL UART TX through hardware API`
- **Task:** Explicitly validate embed-log control-API TX against the STM32G0 shell UART before driving the hardware generators.
- **Started:** unavailable; no `/worklog-start` checkpoint was recorded.
- **Completed:** 2026-07-13 22:07:50 UTC / 2026-07-14 00:07:50 CEST (+0200) (Warsaw)
- **Validation:** `EMBED_LOG_HARDWARE_BINARY=/tmp/embed-log-ci-package/bin/embed-log EMBED_LOG_STM32G0_HARDWARE=1 ... pytest sdk/python/tests/test_backend_hardware_stm32g0_multi_uart.py -q` — passed (1 passed). The CONTROL `uart list` API TX response reported USART1/3/4 enabled.
- **Model-token delta:** unavailable; no before checkpoint exists.

### File changes (`b7cac46`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `sdk/python/tests/test_backend_hardware_stm32g0_multi_uart.py` | 33 | 11 | Exercises CONTROL `tx_write`, verifies shell responses, and ignores pre-reset traffic blocks. |

## 2026-07-13 22:21:55 UTC / 2026-07-14 00:21:55 CEST (Warsaw)

- **Commit:** `369a61c` — `Stabilize simulated TUI integration startup`
- **Task:** Eliminate flaky simulated TUI integration startup and source-log selection.
- **Started:** unavailable; no `/worklog-start` checkpoint was recorded.
- **Completed:** 2026-07-13 22:21:55 UTC / 2026-07-14 00:21:55 CEST (+0200) (Warsaw)
- **Validation:** `python scripts/test_tui_integration.py --binary /tmp/tui-ci-repro/bin/embed-log --backend simulated` — passed three consecutive runs.
- **Model-token delta:** unavailable; no before checkpoint exists.

### File changes (`369a61c`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `scripts/test_tui_integration.py` | 7 | 4 | Retries PTY source writes and selects exact per-source session filenames. |

## 2026-07-13 22:39:24 UTC / 2026-07-14 00:39:24 CEST (Warsaw)

- **Commit:** `cb7353f` — `Build and test release artifacts on hosted runners`
- **Task:** Replace the self-hosted release path with a single native hosted-runner build/test/package/publish workflow and align installer documentation with the repository origin.
- **Started:** unavailable; no `/worklog-start` checkpoint was recorded.
- **Completed:** 2026-07-13 22:39:24 UTC / 2026-07-14 00:39:24 CEST (+0200) (Warsaw)
- **Validation:** release workflow matrix/YAML ordering assertion — passed; `cargo test --locked --package embed-log-core --package embed-log-cli --package embed-log-tui` — passed (389 tests); Linux package extraction and `embed-log version --json` smoke test — passed; `sh -n install.sh` — passed.
- **Model-token delta:** unavailable; no before checkpoint exists.

### File changes (`cb7353f`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `.github/workflows/release-cli.yml` | 85 | 98 | Adds hosted native Linux/macOS/Windows test/package matrix and gated release publishing. |
| `Cargo.toml` | 1 | 1 | Corrects workspace repository metadata. |
| `README.md` | 3 | 3 | Points install and plugin commands at the repository origin. |
| `RELEASE_AND_UPDATE.md` | 4 | 4 | Corrects release installer URLs. |
| `docs/getting-up-to-speed.md` | 1 | 1 | Corrects the installer URL. |
| `docs/releasing.md` | 18 | 18 | Documents the hosted matrix and tested release flow. |
| `install.ps1` | 1 | 1 | Uses the repository origin by default. |
| `install.sh` | 1 | 1 | Uses the repository origin by default. |
| `sdk/python/README.md` | 1 | 1 | Corrects the SDK repository link. |
| `sdk/python/pyproject.toml` | 1 | 1 | Corrects package repository metadata. |

## 2026-07-13 22:43:08 UTC / 2026-07-14 00:43:08 CEST (Warsaw)

- **Commit:** `f54b88f` — `Add release matrix dry-run mode`
- **Task:** Make the hosted release build/test matrix manually runnable without publishing a GitHub Release.
- **Started:** unavailable; no `/worklog-start` checkpoint was recorded.
- **Completed:** 2026-07-13 22:43:08 UTC / 2026-07-14 00:43:08 CEST (+0200) (Warsaw)
- **Validation:** release workflow YAML assertion confirmed the manual publish flag defaults false, checkout uses the selected branch, and publishing is gated to tags or an explicit flag.
- **Model-token delta:** unavailable; no before checkpoint exists.

### File changes (`f54b88f`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `.github/workflows/release-cli.yml` | 12 | 4 | Adds publish opt-in and correct branch checkout for manual matrix dry runs. |
| `docs/releasing.md` | 1 | 1 | Documents the non-publishing hosted release test dispatch. |

## 2026-07-14 03:36:39 UTC / 2026-07-14 05:36:39 CEST (Warsaw)

- **Commit:** `3362e78` — `Verify installed CLI self-update in CI`
- **Task:** Add a deterministic installed-CLI self-update integration test to CI.
- **Started:** unavailable; no `/worklog-start` checkpoint was recorded.
- **Completed:** 2026-07-14 03:36:39 UTC / 2026-07-14 05:36:39 CEST (+0200) (Warsaw)
- **Validation:** `python scripts/test_update_integration.py --binary /tmp/embed-log-ci-package/bin/embed-log` — passed; `cargo test --locked --package embed-log-cli` — passed (90 tests); CI workflow ordering assertion — passed.
- **Model-token delta:** unavailable; no before checkpoint exists.

### File changes (`3362e78`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `.github/workflows/ci.yml` | 8 | 0 | Runs the self-update fixture against the installed release package before installed UI E2E. |
| `docs/development.md` | 1 | 0 | Documents the local self-update integration command. |
| `scripts/test_update_integration.py` | 121 | 0 | Serves a checksummed local release fixture and verifies check, update, replacement, and executable health. |

## 2026-07-14 03:51:38 UTC / 2026-07-14 05:51:38 CEST (Warsaw)

- **Commit:** `3e9295c` — `Test updater in every release build`
- **Task:** Run the deterministic updater verification for every supported release-matrix build.
- **Started:** unavailable; no `/worklog-start` checkpoint was recorded.
- **Completed:** 2026-07-14 03:51:38 UTC / 2026-07-14 05:51:38 CEST (+0200) (Warsaw)
- **Validation:** `python scripts/test_update_integration.py --binary /tmp/embed-log-ci-package/bin/embed-log` — passed; `cargo test --locked --package embed-log-cli` — passed (90 tests); release-matrix updater-step ordering assertion — passed.
- **Model-token delta:** unavailable; no before checkpoint exists.

### File changes (`3e9295c`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `.github/workflows/release-cli.yml` | 14 | 0 | Runs Unix updater replacement fixtures and Windows updater-guidance checks before artifact upload. |
| `docs/releasing.md` | 1 | 1 | Documents release-matrix updater validation. |
| `scripts/test_update_integration.py` | 21 | 7 | Selects the correct Linux/macOS update target dynamically. |

## 2026-07-14 05:05:38 UTC / 2026-07-14 07:05:38 CEST (Warsaw)

- **Commit:** `4078025` — `Remove internal transport name from roadmap`
- **Task:** Remove the remaining internal transport name from tracked documentation.
- **Started:** unavailable; no `/worklog-start` checkpoint was recorded.
- **Completed:** 2026-07-14 05:05:38 UTC / 2026-07-14 07:05:38 CEST (+0200) (Warsaw)
- **Validation:** case-insensitive tracked-text scan for `gwl`, `lnk`, `mcu-link`, `reader controller`, and `lnk121` found no remaining textual matches.
- **Model-token delta:** unavailable; no before checkpoint exists.

### File changes (`4078025`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `docs/non-session-roadmap.md` | 1 | 1 | Replaces the internal transport name with neutral custom-transport wording. |

## 2026-08-03 16:21:21 UTC / 2026-08-03 18:21:21 CEST (Warsaw)

- **Commit:** `230a3e3` — `Document Embed-log MVP overhaul`
- **Task:** Capture the agreed Embed-log MVP overhaul and Linux acceptance plan in an implementer handoff.
- **Started:** 2026-08-03 16:20:08 UTC / 2026-08-03 18:20:08 CEST (+0200) (Warsaw)
- **Completed:** 2026-08-03 16:21:21 UTC / 2026-08-03 18:21:21 CEST (+0200) (Warsaw)
- **Validation:** `git diff --check`; `wc -l -w mvp-embed-log-todo.md`; heading inventory with `rg -n '^## ' mvp-embed-log-todo.md` — passed (668 lines, 2,083 words, all planned sections present).
- **Model-token delta:** unavailable; the `/worklog-start` extension command was not available in this API session.

### File changes (`230a3e3`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `mvp-embed-log-todo.md` | 668 | 0 | Documents the MVP scope, retained and removed features, daemon/instance/session CLI, config v2, parser migration, and Linux/agent validation plans. |

## 2026-08-03 16:30:40 UTC / 2026-08-03 18:30:40 CEST (Warsaw)

- **Commit:** `c267f0f` — `Remove Tauri desktop surface`
- **Task:** Remove the Tauri desktop application, launch path, frontend bridge, CI/release surface, and current documentation while retaining browser and TUI modes.
- **Started:** 2026-08-03 16:24:50 UTC / 2026-08-03 18:24:50 CEST (+0200) (Warsaw)
- **Completed:** 2026-08-03 16:30:40 UTC / 2026-08-03 18:30:40 CEST (+0200) (Warsaw)
- **Validation:** `cargo fmt --all -- --check` — passed; `cargo test --locked --package embed-log-core --package embed-log-cli --package embed-log-tui` — passed (88 CLI, 225 core, and 74 TUI tests); `cargo clippy --locked --package embed-log-core --package embed-log-cli --package embed-log-tui --all-targets -- -D warnings` — passed; `npm --prefix tests-ui run test:unit` — passed (19 tests); locked workspace metadata and rebuilt CLI `--ui` rejection checks — passed.
- **Model-token delta:** unavailable; the `/worklog-start` extension command was not available in this API session.

### File changes (`c267f0f`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `.github/workflows/ci.yml` | 0 | 57 | Removes the disabled Tauri Linux CI job. |
| `Cargo.lock` | 185 | 3033 | Prunes the Tauri desktop dependency graph while retaining locked active dependencies. |
| `Cargo.toml` | 1 | 2 | Removes the Tauri crate from the workspace. |
| `README.md` | 4 | 5 | Describes browser and TUI products without the desktop app. |
| `RELEASE_AND_UPDATE.md` | 0 | 2 | Removes obsolete Tauri updater notes. |
| `crates/embed-log-cli/src/commands/mod.rs` | 0 | 1 | Removes the UI launcher module. |
| `crates/embed-log-cli/src/commands/run.rs` | 3 | 3 | Removes Tauri-specific onboarding comments. |
| `crates/embed-log-cli/src/commands/ui.rs` | 0 | 139 | Deletes Tauri binary discovery and launch behavior. |
| `crates/embed-log-cli/src/main.rs` | 3 | 15 | Removes `--ui` and adds a regression test that rejects it. |
| `crates/embed-log-core/src/config/paths.rs` | 2 | 2 | Makes config path documentation frontend-neutral. |
| `crates/embed-log-core/src/onboarding.rs` | 7 | 17 | Removes desktop-specific onboarding contracts and comments. |
| `crates/embed-log-tauri/Cargo.toml` | 0 | 22 | Deletes the desktop crate manifest. |
| `crates/embed-log-tauri/build.rs` | 0 | 3 | Deletes the Tauri build script. |
| `crates/embed-log-tauri/gen/schemas/acl-manifests.json` | 0 | 1 | Deletes generated desktop ACL data. |
| `crates/embed-log-tauri/gen/schemas/capabilities.json` | 0 | 1 | Deletes generated desktop capability data. |
| `crates/embed-log-tauri/gen/schemas/desktop-schema.json` | 0 | 2612 | Deletes generated desktop schema data. |
| `crates/embed-log-tauri/gen/schemas/macOS-schema.json` | 0 | 2612 | Deletes generated macOS schema data. |
| `crates/embed-log-tauri/gen/schemas/windows-schema.json` | 0 | 2612 | Deletes generated Windows schema data. |
| `crates/embed-log-tauri/icons/128x128.png` | binary | binary | Deletes a Tauri application icon. |
| `crates/embed-log-tauri/icons/256x256.png` | binary | binary | Deletes a Tauri application icon. |
| `crates/embed-log-tauri/icons/32x32.png` | binary | binary | Deletes a Tauri application icon. |
| `crates/embed-log-tauri/src/lib.rs` | 0 | 544 | Deletes the desktop application runtime. |
| `crates/embed-log-tauri/src/main.rs` | 0 | 5 | Deletes the desktop binary entry point. |
| `crates/embed-log-tauri/tauri.conf.json` | 0 | 35 | Deletes desktop packaging configuration. |
| `crates/embed-log-tui/src/lib.rs` | 3 | 3 | Describes the TUI directly against the browser-compatible server. |
| `demo.sh` | 10 | 22 | Removes desktop demo mode. |
| `docs/architecture.md` | 12 | 27 | Removes the desktop shell from architecture and documents TUI instead. |
| `docs/cli.md` | 3 | 14 | Removes `--ui`, desktop launch, and desktop environment variables. |
| `docs/configuration.md` | 1 | 3 | Removes desktop-specific config path behavior. |
| `docs/development.md` | 4 | 25 | Removes desktop prerequisites, recipes, and workspace entries. |
| `docs/index.md` | 1 | 2 | Removes the desktop documentation link. |
| `docs/non-session-roadmap.md` | 0 | 1 | Removes deferred desktop packaging work. |
| `docs/releasing.md` | 0 | 10 | Removes the desktop release section. |
| `docs/tauri.md` | 0 | 146 | Deletes desktop application documentation. |
| `frontend/onboarding.js` | 1 | 4 | Removes Tauri invocation fallback from onboarding. |
| `frontend/ui.js` | 1 | 14 | Opens session URLs with browser behavior only. |
| `justfile` | 8 | 18 | Removes desktop build/run/demo recipes and clarifies no-browser mode. |
| `tui-frontend-plan.md` | 0 | 419 | Deletes the completed plan containing obsolete desktop integration assumptions. |

## 2026-08-03 16:37:40 UTC / 2026-08-03 18:37:40 CEST (Warsaw)

- **Commit:** `f6867e8` — `Remove production demo and init modes`
- **Task:** Remove production demo/init commands and embedded traffic generation while moving browser coverage onto a test-owned fixture that launches the normal run path.
- **Started:** 2026-08-03 16:30:40 UTC / 2026-08-03 18:30:40 CEST (+0200) (Warsaw)
- **Completed:** 2026-08-03 16:37:40 UTC / 2026-08-03 18:37:40 CEST (+0200) (Warsaw)
- **Validation:** `cargo fmt --all -- --check` — passed; `cargo test --locked --package embed-log-core --package embed-log-cli --package embed-log-tui` — passed (86 CLI, 220 core, and 74 TUI tests); `cargo clippy --locked --package embed-log-core --package embed-log-cli --package embed-log-tui --all-targets -- -D warnings` — passed; `npm --prefix tests-ui run test:unit` — passed (19 tests); `npm --prefix tests-ui run test:e2e` — passed twice, including after fixture/spec renames (4 tests); rebuilt CLI rejection and run-based fixture checks — passed.
- **Model-token delta:** unavailable; the `/worklog-start` extension command was not available in this API session.

### File changes (`f6867e8`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `.github/workflows/ci.yml` | 2 | 2 | Renames the browser fixture traffic interval variable. |
| `README.md` | 2 | 9 | Removes demo/init instructions and points to normal run/config samples. |
| `crates/embed-log-cli/src/commands/misc.rs` | 1 | 11 | Removes the embedded-config init implementation. |
| `crates/embed-log-cli/src/commands/run.rs` | 1 | 53 | Removes demo server startup and generated traffic wiring. |
| `crates/embed-log-cli/src/demo_config.rs` | 0 | 143 | Deletes the embedded production demo configuration. |
| `crates/embed-log-cli/src/main.rs` | 9 | 45 | Removes demo/init command definitions and tests their rejection. |
| `crates/embed-log-core/src/demo.rs` | 0 | 475 | Deletes production synthetic traffic generation. |
| `crates/embed-log-core/src/lib.rs` | 0 | 1 | Removes the demo module export. |
| `crates/embed-log-tui/src/draw.rs` | 2 | 2 | Renames test fixture application labels. |
| `crates/embed-log-tui/src/lib.rs` | 3 | 5 | Removes demo-mode references and a deleted planning link. |
| `crates/embed-log-tui/src/main.rs` | 3 | 3 | Documents only the retained run-based integrated mode. |
| `crates/embed-log-tui/src/protocol.rs` | 2 | 2 | Renames protocol test fixture labels. |
| `demo.events.yml` | 0 | 23 | Deletes demo event rules. |
| `demo.sh` | 0 | 23 | Deletes the production demo launcher. |
| `demo.yml` | 0 | 90 | Deletes the production demo config. |
| `demo_traffic.py` | 0 | 240 | Deletes the external demo traffic generator. |
| `docs/architecture.md` | 1 | 2 | Removes the demo module and commands from architecture. |
| `docs/cli.md` | 0 | 22 | Removes demo and init command documentation. |
| `docs/configuration.md` | 1 | 1 | Uses a neutral application name in the example. |
| `docs/development.md` | 1 | 10 | Removes demo recipes and module layout. |
| `docs/getting-up-to-speed.md` | 1 | 1 | Directs users to a checked-in config sample instead of init. |
| `docs/tui.md` | 1 | 2 | Removes demo/init launch guidance. |
| `embed-log.yml` | 1 | 1 | Uses a neutral application name. |
| `event-detection-plan.md` | 2 | 2 | Points event validation at test-owned regression fixtures. |
| `justfile` | 2 | 23 | Removes the demo recipe and defaults run to `embed-log.yml`. |
| `tests-ui/playwright.config.js` | 3 | 3 | Starts the renamed ordinary-run browser fixture. |
| `tests-ui/playwright.regression.config.js` | 3 | 3 | Starts the renamed ordinary-run regression fixture. |
| `tests-ui/regression-inventory.json` | 2 | 2 | Tracks the renamed test spec and test-owned traffic. |
| `tests-ui/rust-test-server.mjs` | 5 | 5 | Renames and reframes the fixture as test infrastructure using `embed-log run`. |
| `tests-ui/tests/rust-server.spec.js` | 0 | 0 | Renames the browser backend test away from demo terminology. |

## 2026-08-03 16:41:34 UTC / 2026-08-03 18:41:34 CEST (Warsaw)

- **Commit:** `543b85e` — `Remove interactive onboarding`
- **Task:** Remove automatic and explicit browser onboarding and replace missing-config interaction with a direct actionable CLI error.
- **Started:** 2026-08-03 16:38:29 UTC / 2026-08-03 18:38:29 CEST (+0200) (Warsaw)
- **Completed:** 2026-08-03 16:41:34 UTC / 2026-08-03 18:41:34 CEST (+0200) (Warsaw)
- **Validation:** `cargo fmt --all -- --check` — passed; `cargo test --locked --package embed-log-core --package embed-log-cli --package embed-log-tui` — passed (87 CLI, 216 core, and 74 TUI tests); `cargo clippy --locked --package embed-log-core --package embed-log-cli --package embed-log-tui --all-targets -- -D warnings` — passed; `npm --prefix tests-ui run test:unit` — passed (19 tests); `npm --prefix tests-ui run test:e2e` — passed (4 tests); rebuilt CLI missing-config hint and removed `onboard` rejection checks — passed.
- **Model-token delta:** unavailable; the `/worklog-start` extension command was not available in this API session.

### File changes (`543b85e`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `crates/embed-log-cli/src/commands/run.rs` | 22 | 64 | Removes onboarding startup and adds tested non-interactive missing-config guidance. |
| `crates/embed-log-cli/src/main.rs` | 2 | 26 | Removes the onboard command and extends removed-surface regression coverage. |
| `crates/embed-log-core/src/lib.rs` | 0 | 1 | Removes the onboarding module export. |
| `crates/embed-log-core/src/onboarding.rs` | 0 | 659 | Deletes quick-config generation and the onboarding HTTP server. |
| `crates/embed-log-tui/src/lib.rs` | 2 | 2 | Removes onboarding-specific TUI wording. |
| `docs/architecture.md` | 0 | 3 | Removes onboarding modules, behavior, and frontend assets. |
| `docs/cli.md` | 2 | 35 | Documents direct missing-config failure and removes onboarding reference. |
| `docs/getting-up-to-speed.md` | 1 | 1 | Uses a copied checked-in sample instead of browser setup. |
| `docs/non-session-roadmap.md` | 1 | 1 | Keeps quick-run parity without onboarding scope. |
| `docs/quickstart.md` | 1 | 1 | Directs advanced setup to saved YAML samples. |
| `docs/tui.md` | 1 | 1 | Directs config-based TUI users to YAML samples. |
| `frontend/onboarding.js` | 0 | 336 | Deletes the browser setup frontend. |

## 2026-08-03 16:56:43 UTC / 2026-08-03 18:56:43 CEST (Warsaw)

- **Commit:** `b60b9ee` — `Remove network capture and pcap support`
- **Task:** Remove network-capture/pcap sources, dependencies, diagnostics, packet-search/UI behavior, fixtures, and documentation while retaining explicit UDP sources.
- **Started:** 2026-08-03 16:43:53 UTC / 2026-08-03 18:43:53 CEST (+0200) (Warsaw)
- **Completed:** 2026-08-03 16:56:43 UTC / 2026-08-03 18:56:43 CEST (+0200) (Warsaw)
- **Validation:** `cargo fmt --all -- --check` and `git diff --check` — passed; `cargo test --locked --package embed-log-core --package embed-log-cli --package embed-log-tui` — passed (84 CLI, 207 core, and 73 TUI tests); `cargo clippy --locked --package embed-log-core --package embed-log-cli --package embed-log-tui --all-targets -- -D warnings` — passed; `npm --prefix tests-ui run test:unit` — passed (19 tests); `npm --prefix tests-ui run test:e2e` — passed (4 tests); `npm --prefix tests-ui run test:regression:data` — passed (5 tests); `PYTHONPATH=sdk/python python3 -m pytest sdk/python/tests -q` — passed (53 passed, 2 skipped); rebuilt CLI root-config validation, removed network-source rejection, removed packet-filter rejection, and lockfile checks — passed.
- **Model-token delta:** unavailable; the `/worklog-start` extension command was not available in this API session.

### File changes (`b60b9ee`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `Cargo.lock` | 3 | 94 | Prunes pcap and dynamic-library dependency entries. |
| `README.md` | 1 | 34 | Limits sources to UART/UDP/file and removes pcap setup. |
| `config-samples/reference_full_annotated.yml` | 0 | 9 | Removes the network-capture source and tab. |
| `config-samples/single_network_single_tab.yml` | 0 | 17 | Deletes the mock network source sample. |
| `config-samples/single_pcap_udp_single_tab.yml` | 0 | 22 | Deletes the pcap source sample. |
| `crates/embed-log-cli/Cargo.toml` | 0 | 2 | Removes libloading and the pcap feature. |
| `crates/embed-log-cli/src/commands/misc.rs` | 1 | 196 | Removes packet-capture validation and doctor diagnostics. |
| `crates/embed-log-cli/src/commands/run.rs` | 0 | 9 | Removes obsolete network fields from generated sources. |
| `crates/embed-log-cli/src/commands/sessions.rs` | 13 | 88 | Removes packet-specific search filters and mini output fields. |
| `crates/embed-log-core/Cargo.toml` | 0 | 5 | Removes the pcap dependency and feature. |
| `crates/embed-log-core/src/config/loader.rs` | 34 | 127 | Rejects removed network configs and removes backend validation. |
| `crates/embed-log-core/src/config/models.rs` | 1 | 59 | Removes network, pcap, UDP-filter, and payload config models. |
| `crates/embed-log-core/src/net/ws_server.rs` | 0 | 31 | Removes the network filter WebSocket command. |
| `crates/embed-log-core/src/runtime/server.rs` | 1 | 26 | Removes network source resolution. |
| `crates/embed-log-core/src/sources/mod.rs` | 0 | 2 | Removes the network source module/export. |
| `crates/embed-log-core/src/sources/network.rs` | 0 | 541 | Deletes mock and pcap packet-capture implementations. |
| `crates/embed-log-tui/src/app.rs` | 0 | 3 | Removes filter-result handling. |
| `crates/embed-log-tui/src/protocol.rs` | 1 | 20 | Removes network filter protocol messages and tests. |
| `docs/api-status.md` | 1 | 1 | Lists only retained source kinds. |
| `docs/architecture.md` | 2 | 4 | Removes network source and filter architecture. |
| `docs/automation-agent-plan.md` | 0 | 1 | Removes optional pcap scope. |
| `docs/cli.md` | 3 | 6 | Removes packet diagnostics and search examples. |
| `docs/configuration.md` | 3 | 58 | Removes network-capture schema and examples. |
| `docs/getting-up-to-speed.md` | 0 | 2 | Removes pcap setup guidance. |
| `embed-log.yml` | 0 | 13 | Removes the obsolete network source/tab and invalid browser field. |
| `frontend/renderPane.js` | 1 | 1 | Uses regex filtering for every retained pane. |
| `frontend/ui.js` | 0 | 8 | Removes BPF filter dispatch. |
| `frontend/ws.js` | 0 | 12 | Removes filter-result handling. |
| `justfile` | 0 | 6 | Removes the pcap build recipe. |
| `sdk/python/embed_log_sdk/config.py` | 1 | 1 | Documents retained SDK source kinds. |
| `sdk/python/embed_log_sdk/models.py` | 1 | 1 | Documents retained API source kinds. |
| `skills/embed-log/SKILL.md` | 1 | 1 | Uses a retained UDP search example. |
| `tests-ui/config-regression.yml` | 0 | 12 | Removes the mock network fixture/tab. |
| `tests-ui/regression-categories.mjs` | 0 | 1 | Removes the network regression spec from the data group. |
| `tests-ui/regression-inventory.json` | 0 | 6 | Removes network-capture test inventory. |
| `tests-ui/regression-tests/network-capture.spec.js` | 0 | 186 | Deletes BPF/network UI regression scenarios. |

## 2026-08-03 17:19:30 UTC / 2026-08-03 19:19:30 CEST (Warsaw)

- **Commit:** `af0655c` — `Remove CBOR datagram parser`
- **Task:** Remove the CBOR datagram parser, dependency, configurations, fixtures, tests, and documentation while retaining text and protocol-specific parsers.
- **Started:** 2026-08-03 16:57:30 UTC / 2026-08-03 18:57:30 CEST (+0200) (Warsaw)
- **Completed:** 2026-08-03 17:19:30 UTC / 2026-08-03 19:19:30 CEST (+0200) (Warsaw)
- **Validation:** `cargo fmt --all -- --check` and `git diff --check` — passed; `cargo test --locked --package embed-log-core --package embed-log-cli --package embed-log-tui` — passed (84 CLI, 201 core, and 73 TUI tests); `cargo clippy --locked --package embed-log-core --package embed-log-cli --package embed-log-tui --all-targets -- -D warnings` — passed; `npm --prefix tests-ui run test:unit` — passed (19 tests); `npm --prefix tests-ui run test:e2e -- --workers=1` — passed (4 tests); `npm --prefix tests-ui run test:regression -- --workers=1` — 72 passed and 4 skipped, with one known timing-sensitive sync-highlight failure; isolated rerun of that test passed; root config doctor validation, removed-CBOR diagnostic, and lockfile dependency checks — passed.
- **Model-token delta:** unavailable; the `/worklog-start` extension command was not available in this API session.

### File changes (`af0655c`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `Cargo.lock` | 0 | 28 | Removes ciborium and its transitive lockfile entries. |
| `config-samples/reference_full_annotated.yml` | 0 | 8 | Removes the CBOR source and sensor tab. |
| `config-samples/three_udp_cbor_two_tabs.yml` | 0 | 28 | Deletes the CBOR-focused sample configuration. |
| `crates/embed-log-core/Cargo.toml` | 0 | 1 | Removes the ciborium dependency. |
| `crates/embed-log-core/src/config/loader.rs` | 31 | 43 | Rejects removed CBOR parser configs and updates parser/sample validation. |
| `crates/embed-log-core/src/parsers/cbor.rs` | 0 | 153 | Deletes CBOR datagram decoding and unit tests. |
| `crates/embed-log-core/src/parsers/mod.rs` | 0 | 3 | Removes CBOR parser registration and export. |
| `crates/embed-log-core/src/parsers/traits.rs` | 1 | 1 | Generalizes stream-buffering documentation. |
| `crates/embed-log-core/src/sources/udp.rs` | 0 | 30 | Removes the UDP CBOR integration test. |
| `docs/architecture.md` | 3 | 4 | Documents only retained parser behavior. |
| `docs/configuration.md` | 1 | 24 | Removes CBOR schema, examples, and reference configuration. |
| `docs/getting-up-to-speed.md` | 1 | 1 | Lists only retained parsers. |
| `embed-log.yml` | 0 | 11 | Removes the CBOR source and tab from the root configuration. |
| `tests-ui/config-regression.yml` | 0 | 10 | Removes the browser CBOR fixture source and tab. |
| `tests-ui/regression-categories.mjs` | 0 | 1 | Removes the deleted CBOR regression from the data category. |
| `tests-ui/regression-inventory.json` | 0 | 6 | Removes CBOR browser-test inventory. |
| `tests-ui/regression-tests/cbor-decoder.spec.js` | 0 | 83 | Deletes CBOR browser regression scenarios. |
| `tests-ui/regression-tests/copy-format.spec.js` | 3 | 1 | Ignores marker delimiters when checking compact log formatting. |
| `tests-ui/regression-tests/demo-smoke.spec.js` | 13 | 12 | Updates selection checks and stabilizes virtual-history assertions. |
| `tests-ui/regression-tests/export-replay.spec.js` | 2 | 2 | Updates unwrapped tab traversal for the retained seven panes. |
| `tests-ui/regression-tests/layout-sync.spec.js` | 3 | 3 | Updates unwrapped tab traversal for the retained seven panes. |
| `tests-ui/regression-tests/scope-selection.spec.js` | 8 | 9 | Removes CBOR pane assumptions from selection and export checks. |
| `tests-ui/rust-test-server.mjs` | 1 | 46 | Replaces synthetic CBOR encoding/traffic with plain UDP text. |
| `tests-ui/tests/rust-server.spec.js` | 2 | 20 | Replaces CBOR browser coverage with retained UDP text coverage. |

## 2026-08-03 17:35:59 UTC / 2026-08-03 19:35:59 CEST (Warsaw)

- **Commit:** `a2cf3fb` — `Remove self-update command`
- **Task:** Remove the obsolete `embed-log update` command, updater implementation/dependencies/tests, and self-update release and documentation surfaces while retaining installer checksum verification.
- **Started:** 2026-08-03 17:32:15 UTC / 2026-08-03 19:32:15 CEST (+0200) (Warsaw; measured from the first implementation file write)
- **Completed:** 2026-08-03 17:35:59 UTC / 2026-08-03 19:35:59 CEST (+0200) (Warsaw)
- **Validation:** `cargo fmt --all -- --check` and `git diff --check` — passed; `cargo test --locked --package embed-log-core --package embed-log-cli --package embed-log-tui` — passed (78 CLI, 201 core, and 73 TUI tests); `cargo clippy --locked --package embed-log-core --package embed-log-cli --package embed-log-tui --all-targets -- -D warnings` — passed; `npm --prefix tests-ui run test:unit` — passed (19 tests); `npm --prefix tests-ui run test:e2e -- --workers=1` — passed (4 tests); release CLI build and `scripts/package-cli.sh x86_64-unknown-linux-gnu` — passed; packaged `version --json`, removed-command rejection, workflow YAML parsing, stale-reference search, and lockfile dependency checks — passed.
- **Model-token delta:** unavailable; the `/worklog-start` extension command was not available in this API session.

### File changes (`a2cf3fb`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `.github/workflows/ci.yml` | 0 | 8 | Removes installed-binary self-update setup and integration steps. |
| `.github/workflows/release-cli.yml` | 0 | 23 | Removes Unix updater and Windows updater-guidance release checks. |
| `Cargo.lock` | 4 | 538 | Prunes updater HTTP, TLS, semantic-version, hashing, and transitive dependencies. |
| `Cargo.toml` | 0 | 3 | Removes updater-only workspace dependencies. |
| `RELEASE_AND_UPDATE.md` | 0 | 523 | Deletes the obsolete built-in self-update plan. |
| `crates/embed-log-cli/Cargo.toml` | 1 | 4 | Removes updater dependencies and corrects retained-source package metadata. |
| `crates/embed-log-cli/src/commands/misc.rs` | 0 | 350 | Removes release lookup, download, verification, replacement, and updater tests. |
| `crates/embed-log-cli/src/main.rs` | 1 | 26 | Removes the update command/dispatch and adds rejection regression coverage. |
| `docs/cli.md` | 1 | 13 | Removes self-update usage and corrects stale doctor wording. |
| `docs/development.md` | 0 | 1 | Removes the deleted updater integration-test command. |
| `docs/getting-up-to-speed.md` | 1 | 10 | Removes the self-update workflow and renumbers the team workflow. |
| `docs/non-session-roadmap.md` | 1 | 8 | Removes deferred built-in updater work. |
| `docs/releasing.md` | 1 | 1 | Removes self-update fixture claims from release validation. |
| `scripts/test_update_integration.py` | 0 | 135 | Deletes the fake GitHub Release self-update integration fixture. |

## 2026-08-03 17:47:36 UTC / 2026-08-03 19:47:36 CEST (Warsaw)

- **Commit:** `e1243fa` — `Remove raw log merge command`
- **Task:** Remove the obsolete top-level `embed-log merge` command, raw-log merge implementation/tests, and its documentation while preserving session export and config-level merged sources.
- **Started:** 2026-08-03 17:45:36 UTC / 2026-08-03 19:45:36 CEST (+0200) (Warsaw)
- **Completed:** 2026-08-03 17:47:36 UTC / 2026-08-03 19:47:36 CEST (+0200) (Warsaw)
- **Validation:** `cargo fmt --all -- --check` and `git diff --check` — passed; `cargo test --locked --package embed-log-core --package embed-log-cli --package embed-log-tui` — passed (72 CLI, 201 core, and 73 TUI tests); `cargo clippy --locked --package embed-log-core --package embed-log-cli --package embed-log-tui --all-targets -- -D warnings` — passed; `npm --prefix tests-ui run test:unit` — passed (19 tests); `npm --prefix tests-ui run test:e2e -- --workers=1` — passed (4 tests); release CLI build and `scripts/package-cli.sh x86_64-unknown-linux-gnu` — passed; packaged merge-command rejection, stale-reference search, and retained `sessions export`/`parse` help checks — passed.
- **Model-token delta:** unavailable; the `/worklog-start` extension command was not available in this API session.

### File changes (`e1243fa`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `README.md` | 1 | 1 | Removes raw-log merge from the CLI capability summary. |
| `crates/embed-log-cli/src/commands/misc.rs` | 0 | 177 | Removes merge export construction, argument grouping, and unit tests. |
| `crates/embed-log-cli/src/main.rs` | 1 | 35 | Removes merge arguments/dispatch and adds command-rejection coverage. |
| `docs/architecture.md` | 1 | 1 | Removes merge from the top-level CLI utility list. |
| `docs/cli.md` | 0 | 26 | Removes raw-log merge usage and timestamp examples. |
| `docs/development.md` | 1 | 1 | Removes `merged.html` from generated artifact examples. |

## 2026-08-03 17:59:31 UTC / 2026-08-03 19:59:31 CEST (Warsaw)

- **Commit:** `266047f` — `Remove HTML parse command`
- **Task:** Remove the obsolete top-level `embed-log parse` command, exported-HTML extraction implementation/tests, and documentation while preserving session read and export operations.
- **Started:** 2026-08-03 17:57:44 UTC / 2026-08-03 19:57:44 CEST (+0200) (Warsaw)
- **Completed:** 2026-08-03 17:59:31 UTC / 2026-08-03 19:59:31 CEST (+0200) (Warsaw)
- **Validation:** `cargo fmt --all -- --check` and `git diff --check` — passed; `cargo test --locked --package embed-log-core --package embed-log-cli --package embed-log-tui` — passed (68 CLI, 201 core, and 73 TUI tests); `cargo clippy --locked --package embed-log-core --package embed-log-cli --package embed-log-tui --all-targets -- -D warnings` — passed; `npm --prefix tests-ui run test:unit` — passed (19 tests); `npm --prefix tests-ui run test:e2e -- --workers=1` — passed (4 tests); release CLI build and `scripts/package-cli.sh x86_64-unknown-linux-gnu` — passed; packaged parse-command rejection, stale-reference search, and retained `sessions export`/`sessions combined` help checks — passed.
- **Model-token delta:** unavailable; the `/worklog-start` extension command was not available in this API session.

### File changes (`266047f`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `crates/embed-log-cli/src/commands/misc.rs` | 2 | 97 | Removes HTML extraction/grouping code and four parse-specific unit tests. |
| `crates/embed-log-cli/src/main.rs` | 2 | 14 | Removes parse arguments/dispatch, adds rejection coverage, and narrows the validate parser test. |
| `docs/architecture.md` | 1 | 1 | Removes parse from the top-level CLI utility list. |
| `docs/cli.md` | 0 | 8 | Removes exported-HTML parse usage and behavior. |
| `docs/development.md` | 1 | 1 | Removes the parsed-output directory from generated artifact examples. |

## 2026-08-03 18:14:26 UTC / 2026-08-03 20:14:26 CEST (Warsaw)

- **Commit:** `63091d2` — `Remove session import command`
- **Task:** Remove `embed-log sessions import`, its RFC3339 external-log mutation logic/tests, and documentation while retaining file/UDP capture and session export.
- **Started:** 2026-08-03 18:11:44 UTC / 2026-08-03 20:11:44 CEST (+0200) (Warsaw)
- **Completed:** 2026-08-03 18:14:26 UTC / 2026-08-03 20:14:26 CEST (+0200) (Warsaw)
- **Validation:** `cargo fmt --all -- --check` and `git diff --check` — passed; `cargo test --locked --package embed-log-core --package embed-log-cli --package embed-log-tui` — passed (67 CLI, 201 core, and 73 TUI tests); `cargo clippy --locked --package embed-log-core --package embed-log-cli --package embed-log-tui --all-targets -- -D warnings` — passed; `npm --prefix tests-ui run test:unit` — passed (19 tests); `npm --prefix tests-ui run test:e2e -- --workers=1` — passed (4 tests); release CLI build and `scripts/package-cli.sh x86_64-unknown-linux-gnu` — passed; packaged import-command rejection, stale-reference search, and retained file-capture/session-export help checks — passed.
- **Model-token delta:** unavailable; the `/worklog-start` extension command was not available in this API session.

### File changes (`63091d2`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `crates/embed-log-cli/src/commands/sessions.rs` | 0 | 155 | Removes import arguments/dispatch, timestamp parsing, session mutation, and parser test. |
| `crates/embed-log-cli/src/main.rs` | 1 | 0 | Adds nested removed-command regression coverage for `sessions import`. |
| `docs/cli.md` | 0 | 9 | Removes external timestamped-log import usage. |
| `docs/getting-up-to-speed.md` | 1 | 17 | Replaces post-capture import guidance with configured file/UDP capture. |

## 2026-08-03 18:33:41 UTC / 2026-08-03 20:33:41 CEST (Warsaw)

- **Commit:** `40b06b2` — `Remove session bundle command`
- **Task:** Remove `embed-log sessions bundle`, archive generation/tests, support-bundle documentation, and direct archive dependencies while preserving HTML/raw/JSONL session exports.
- **Started:** 2026-08-03 18:31:01 UTC / 2026-08-03 20:31:01 CEST (+0200) (Warsaw)
- **Completed:** 2026-08-03 18:33:41 UTC / 2026-08-03 20:33:41 CEST (+0200) (Warsaw)
- **Validation:** `cargo fmt --all -- --check` and `git diff --check` — passed; `cargo test --locked --package embed-log-core --package embed-log-cli --package embed-log-tui` — passed (66 CLI, 201 core, and 73 TUI tests); `cargo clippy --locked --package embed-log-core --package embed-log-cli --package embed-log-tui --all-targets -- -D warnings` — passed; `npm --prefix tests-ui run test:unit` — passed (19 tests); `npm --prefix tests-ui run test:e2e -- --workers=1` — passed (4 tests); release CLI build and `scripts/package-cli.sh x86_64-unknown-linux-gnu` — passed; packaged bundle-command rejection, stale-reference search, retained export-format help, and direct archive-dependency checks — passed.
- **Model-token delta:** unavailable; the `/worklog-start` extension command was not available in this API session.

### File changes (`40b06b2`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `Cargo.lock` | 0 | 23 | Removes the direct CLI archive dependency edges and unused tar/xattr packages. |
| `Cargo.toml` | 0 | 2 | Removes direct workspace archive dependencies. |
| `crates/embed-log-cli/Cargo.toml` | 0 | 2 | Removes direct CLI flate2 and tar dependencies. |
| `crates/embed-log-cli/src/commands/sessions.rs` | 0 | 71 | Removes bundle arguments/dispatch, tarball generation, diagnostics injection, and archive test. |
| `crates/embed-log-cli/src/main.rs` | 1 | 1 | Moves bundle from accepted session commands to removed-command regression coverage. |
| `docs/automation-agent-plan.md` | 1 | 1 | Replaces stale import/bundle wording with current mutation safeguards. |
| `docs/cli.md` | 0 | 7 | Removes support-bundle usage and behavior. |
| `docs/getting-up-to-speed.md` | 1 | 4 | Uses retained session exports for sharing and diagnosis. |

## 2026-08-03 18:41:52 UTC / 2026-08-03 20:41:52 CEST (Warsaw)

- **Commit:** `f48c155` — `Remove session prune command`
- **Task:** Remove `embed-log sessions prune`, recursive retention/deletion logic/tests, and prune documentation while preserving explicit session listing and exports.
- **Started:** 2026-08-03 18:39:54 UTC / 2026-08-03 20:39:54 CEST (+0200) (Warsaw)
- **Completed:** 2026-08-03 18:41:52 UTC / 2026-08-03 20:41:52 CEST (+0200) (Warsaw)
- **Validation:** `cargo fmt --all -- --check` and `git diff --check` — passed; `cargo test --locked --package embed-log-core --package embed-log-cli --package embed-log-tui` — passed (65 CLI, 201 core, and 73 TUI tests); `cargo clippy --locked --package embed-log-core --package embed-log-cli --package embed-log-tui --all-targets -- -D warnings` — passed; `npm --prefix tests-ui run test:unit` — passed (19 tests); `npm --prefix tests-ui run test:e2e -- --workers=1` — passed (4 tests); release CLI build and `scripts/package-cli.sh x86_64-unknown-linux-gnu` — passed; packaged prune-command rejection, stale-reference search, and retained session-list/export help checks — passed.
- **Model-token delta:** unavailable; the `/worklog-start` extension command was not available in this API session.

### File changes (`f48c155`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `crates/embed-log-cli/src/commands/sessions.rs` | 0 | 83 | Removes prune arguments/dispatch, recursive size/deletion logic, and retention test. |
| `crates/embed-log-cli/src/main.rs` | 1 | 9 | Moves prune from accepted session commands to removed-command regression coverage. |
| `docs/agent-capabilities.md` | 1 | 1 | Replaces stale prune/import guardrails with general session-data safeguards. |
| `docs/cli.md` | 0 | 7 | Removes prune usage and behavior. |
| `docs/getting-up-to-speed.md` | 1 | 10 | Removes built-in retention workflow and points to project retention tooling. |
| `docs/non-session-roadmap.md` | 1 | 1 | Removes obsolete retention backlog wording. |

## 2026-08-03 19:22:54 UTC / 2026-08-03 21:22:54 CEST (Warsaw)

- **Commit:** `a0b98cf` — `Remove session marker inspection command`
- **Task:** Remove the offline `embed-log sessions marker list/show` CLI surface, dedicated filtering/formatting code, tests, and documentation while retaining marker persistence, browser/TUI marker behavior, control API creation, exports, and session marker counts.
- **Started:** unavailable; the `/worklog-start` extension command was not available in this API session.
- **Completed:** 2026-08-03 19:22:54 UTC / 2026-08-03 21:22:54 CEST (+0200) (Warsaw)
- **Validation:** `cargo fmt --all -- --check` and `git diff --check` — passed; `cargo test --locked --workspace` — passed (59 CLI, 201 core, and 73 TUI tests); `cargo clippy --locked --workspace --all-targets -- -D warnings` — passed; `PYTHONPATH=sdk/python python3 -m pytest sdk/python/tests -q` — passed (52 tests, 2 skipped); `npm --prefix tests-ui run test:unit` — passed (19 tests); `npm --prefix tests-ui test` — passed sequentially (4 Playwright tests with 1 worker); `cargo build --locked --release` and `scripts/package-cli.sh x86_64-unknown-linux-gnu` — passed; packaged `sessions marker` rejection and stale CLI-reference search — passed.
- **Model-token delta:** unavailable; the `/worklog-start` extension command was not available in this API session.

### File changes (`a0b98cf`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `README.md` | 0 | 11 | Removes marker inspection command examples. |
| `crates/embed-log-cli/src/commands/sessions.rs` | 6 | 287 | Removes marker list/show arguments, dispatch, rendering/filtering helpers, and dedicated tests while retaining marker artifact loading for exports and counts. |
| `crates/embed-log-cli/src/main.rs` | 1 | 0 | Adds removed-command regression coverage for `sessions marker`. |
| `sdk-control-api-summary.md` | 1 | 17 | Removes claims that the retired marker inspection CLI remains available. |
| `sdk/python/tests/test_e2e.py` | 0 | 76 | Removes end-to-end assertions that invoke the retired CLI; existing marker persistence and broadcast coverage remains. |
| `skills/embed-log/SKILL.md` | 1 | 1 | Removes `marker` from accepted `latest` session commands. |

## 2026-08-03 19:35:56 UTC / 2026-08-03 21:35:56 CEST (Warsaw)

- **Commit:** `3aa8fce` — `Introduce YAML config v2 and port 18080`
- **Task:** Introduce canonical YAML config v2 with `server.listen`, named source mappings, source-local UART baud/path fields, optional `ui.tabs`, v2 config generation, and the new `127.0.0.1:18080` default while retaining temporary v1 read compatibility for frontend-plugin migration.
- **Started:** unavailable; the `/worklog-start` extension command was not available in this API session.
- **Completed:** 2026-08-03 19:35:56 UTC / 2026-08-03 21:35:56 CEST (+0200) (Warsaw)
- **Validation:** `cargo fmt --all -- --check` and `git diff --check` — passed; `cargo test --locked --workspace` — passed (59 CLI, 207 core, and 73 TUI tests); `cargo clippy --locked --workspace --all-targets -- -D warnings` — passed; `PYTHONPATH=sdk/python python3 -m pytest sdk/python/tests -q` — passed (53 tests, 2 skipped), including a real backend/PTTY v2 configuration flow; `npm --prefix tests-ui run test:unit` — passed (19 tests); `npm --prefix tests-ui run test:e2e -- --workers=1` — passed (4 browser tests against a v2 server config); `cargo build --locked --release` and `scripts/package-cli.sh x86_64-unknown-linux-gnu` — passed; packaged binary validation of `embed-log.yml` confirmed host `127.0.0.1` and port `18080`.
- **Model-token delta:** unavailable; the `/worklog-start` extension command was not available in this API session.

### File changes (`3aa8fce`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `README.md` | 3 | 3 | Updates default browser, control, and TUI endpoints to port 18080. |
| `config-samples/double_uart_udp_two_tabs.yml` | 15 | 15 | Migrates the multi-source sample to canonical v2. |
| `config-samples/dual_uart_zephyr_dict.yml` | 17 | 23 | Migrates dictionary UART paths, baud, and UI layout to v2. |
| `config-samples/reference_full_annotated.yml` | 11 | 17 | Replaces the plugin-oriented v1 reference with retained v2 settings. |
| `config-samples/single_file_single_tab.yml` | 8 | 8 | Migrates file `path` and UI keys to v2. |
| `config-samples/single_uart_single_tab.yml` | 9 | 9 | Migrates UART `path`/`baud` and UI keys to v2. |
| `crates/embed-log-cli/src/commands/run.rs` | 3 | 3 | Writes saved quick-run configurations through the canonical v2 serializer. |
| `crates/embed-log-cli/src/main.rs` | 2 | 2 | Makes `--port` canonical while retaining `--ws-port` as an alias. |
| `crates/embed-log-core/src/config/loader.rs` | 452 | 21 | Adds strict v2 parsing/normalization, v2 serialization, actionable validation, generated tabs, and unit coverage. |
| `crates/embed-log-core/src/config/mod.rs` | 1 | 1 | Exports v2 configuration serialization. |
| `crates/embed-log-core/src/config/models.rs` | 2 | 2 | Changes runtime defaults to config version 2 and port 18080. |
| `crates/embed-log-tui/src/main.rs` | 1 | 1 | Updates standalone TUI endpoint help. |
| `docs/agent-capabilities.md` | 2 | 2 | Updates status and control endpoint examples. |
| `docs/api-status.md` | 2 | 2 | Updates status API examples. |
| `docs/automation-agent-plan.md` | 1 | 1 | Updates the documented control endpoint. |
| `docs/cli.md` | 4 | 4 | Documents `server.listen` and canonical `--port` overrides. |
| `docs/configuration.md` | 93 | 309 | Rewrites configuration documentation around the concise v2 schema and migration table. |
| `docs/getting-up-to-speed.md` | 11 | 10 | Replaces the old UART example with v2 and updates the control endpoint. |
| `docs/tui.md` | 2 | 2 | Updates standalone TUI endpoint examples. |
| `embed-log.yml` | 19 | 43 | Replaces the checked-in root configuration with valid v2. |
| `sdk/python/embed_log_sdk/config.py` | 31 | 10 | Parses v2 listen/source mappings and defaults SDK discovery to port 18080. |
| `sdk/python/embed_log_sdk/watcher.py` | 1 | 1 | Updates the watcher default control endpoint. |
| `sdk/python/examples/watcher.yml` | 1 | 1 | Updates the watcher example endpoint. |
| `sdk/python/tests/test_backend_hardware_stm32g0_multi_uart.py` | 1 | 1 | Uses canonical `--port` in hardware integration startup. |
| `sdk/python/tests/test_backend_hardware_uart.py` | 1 | 1 | Uses canonical `--port` in hardware integration startup. |
| `sdk/python/tests/test_client.py` | 14 | 14 | Updates mocked client endpoints to the new default. |
| `sdk/python/tests/test_config.py` | 22 | 1 | Adds SDK v2 mapping/listen coverage and updates the default-port assertion. |
| `sdk/python/tests/test_e2e.py` | 10 | 9 | Runs the real backend/PTTY SDK integration fixture from a v2 config. |
| `sdk/python/tests/test_events.py` | 6 | 6 | Updates event client endpoint fixtures. |
| `sdk/python/tests/test_watcher.py` | 6 | 6 | Updates watcher endpoint fixtures and expectations. |
| `test-mvp.yml` | 10 | 16 | Migrates the checked-in MVP fixture to v2. |
| `tests-ui/rust-test-server.mjs` | 11 | 17 | Starts browser E2E tests from a generated v2 configuration. |

## 2026-08-03 19:47:14 UTC / 2026-08-03 21:47:14 CEST (Warsaw)

- **Commit:** `6fb9306` — `Add named daemon lifecycle`
- **Task:** Add config-based background daemons with named instance registration/discovery, automatic port selection, readiness polling, direct or registered status queries, stale PID cleanup, safe graceful stop, diagnostics, and daemon-specific HTML shutdown policy.
- **Started:** unavailable; the `/worklog-start` extension command was not available in this API session.
- **Completed:** 2026-08-03 19:47:14 UTC / 2026-08-03 21:47:14 CEST (+0200) (Warsaw)
- **Validation:** `cargo fmt --all -- --check` and `git diff --check` — passed; `cargo test --locked --workspace` plus the final daemon-focused rerun — passed (62 CLI unit tests, 2 Linux process integration tests, 207 core tests, and 73 TUI tests); `cargo clippy --locked --workspace --all-targets -- -D warnings` — passed; `PYTHONPATH=sdk/python python3 -m pytest sdk/python/tests -q` — passed (53 tests, 2 skipped); `npm --prefix tests-ui run test:unit` — passed (19 tests); `npm --prefix tests-ui run test:e2e -- --workers=1` — passed (4 tests); `cargo build --locked --release` and `scripts/package-cli.sh x86_64-unknown-linux-gnu` — passed; packaged daemon start/status/stop lifecycle passed and confirmed daemon shutdown produced no automatic HTML.
- **Model-token delta:** unavailable; the `/worklog-start` extension command was not available in this API session.

### File changes (`6fb9306`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `README.md` | 12 | 0 | Adds concise named-daemon startup, status, stop, and selection usage. |
| `crates/embed-log-cli/src/commands/daemon.rs` | 518 | 0 | Implements registry records, stale cleanup, free-port selection, child startup/readiness, HTTP status, instance resolution, safe signaling, and stop cleanup. |
| `crates/embed-log-cli/src/commands/mod.rs` | 1 | 0 | Registers the daemon command implementation module. |
| `crates/embed-log-cli/src/commands/run.rs` | 4 | 1 | Passes daemon shutdown policy into the core server. |
| `crates/embed-log-cli/src/main.rs` | 65 | 2 | Adds `run --daemon --instance`, `status`, `stop`, JSON flags, and hidden child dispatch. |
| `crates/embed-log-cli/tests/daemon_lifecycle.rs` | 204 | 0 | Adds Linux process E2E coverage for readiness, status paths, stale/duplicate records, ambiguity, distinct ports, clean stop, and skipped daemon HTML. |
| `crates/embed-log-core/src/runtime/server.rs` | 16 | 5 | Makes clean-shutdown HTML export configurable and disables it for daemon children. |
| `docs/cli.md` | 21 | 0 | Documents registry location, resolution order, direct URLs, auto ports, diagnostics, and stop safety. |

## 2026-08-04 07:23:46 UTC / 2026-08-04 09:23:46 CEST (Warsaw)

- **Commit:** `ee6b360` — `Add titled session rotation`
- **Task:** Add instance-aware `sessions new --title` rotation that preserves source tasks/UART ownership, stores the original title, creates slugged session IDs, updates browser/TUI clients, and applies foreground-versus-daemon HTML policy.
- **Started:** unavailable; the `/worklog-start` extension command was not available in this API session.
- **Completed:** 2026-08-04 07:23:46 UTC / 2026-08-04 09:23:46 CEST (+0200) (Warsaw)
- **Validation:** `cargo fmt --all -- --check` and `git diff --check` — passed; `cargo test --locked --workspace` plus final focused title/daemon tests — passed (63 CLI unit tests, 2 Linux process integration tests, 208 core tests, and 73 TUI tests); `cargo clippy --locked --workspace --all-targets -- -D warnings` — passed; `PYTHONPATH=sdk/python python3 -m pytest sdk/python/tests -q` — passed (53 tests, 2 skipped); `npm --prefix tests-ui run test:unit` — passed (19 tests); `npm --prefix tests-ui run test:e2e -- --workers=1` — passed (4 tests), including titled browser rotation and post-rotation log routing; release build and `scripts/package-cli.sh x86_64-unknown-linux-gnu` — passed; packaged daemon start/titled-rotate/status/stop verified stable PID, current session, title manifest, slug, and no automatic daemon HTML.
- **Model-token delta:** unavailable; the `/worklog-start` extension command was not available in this API session.

### File changes (`ee6b360`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `README.md` | 2 | 1 | Adds titled experiment rotation to the daemon workflow. |
| `crates/embed-log-cli/src/commands/daemon.rs` | 34 | 9 | Exposes instance endpoint resolution and adds reusable JSON POST support. |
| `crates/embed-log-cli/src/commands/sessions.rs` | 91 | 0 | Adds `sessions new`, title validation, instance/URL targeting, and bounded JSON/text results. |
| `crates/embed-log-cli/src/main.rs` | 11 | 0 | Adds parser regression coverage for the new session command surface. |
| `crates/embed-log-cli/tests/daemon_lifecycle.rs` | 90 | 5 | Extends process E2E coverage with titled rotation, title manifest/slug, stable PID, continued source routing, and failure cleanup guards. |
| `crates/embed-log-core/src/net/ws_server.rs` | 14 | 4 | Accepts optional rotation titles and passes them through the broadcast/API path. |
| `crates/embed-log-core/src/runtime/server.rs` | 142 | 75 | Validates titles, allocates titled IDs, rotates shared writers/session state, and skips rotation HTML for daemons. |
| `crates/embed-log-core/src/session/manager.rs` | 10 | 0 | Persists and exposes the original session title in manifests and session APIs. |
| `docs/cli.md` | 15 | 0 | Documents titled rotation, validation, client continuity, and HTML behavior. |
| `tests-ui/tests/rust-server.spec.js` | 7 | 1 | Verifies titled HTTP rotation, slug/title response, pane clearing, and subsequent live routing. |

## 2026-08-04 07:53:21 UTC / 2026-08-04 09:53:21 CEST (Warsaw)

- **Commit:** `740fcfa` — `Make daemon targeting explicit`
- **Task:** Replace hidden daemon port/target policies with required config, instance, and port inputs; add verified idempotent reuse; require explicit mutation targets; surface registry cleanup/errors; remove disconnect-triggered export; and propagate foreground bind failures.
- **Started:** unavailable; the `/worklog-start` extension command was not available in this API session.
- **Completed:** 2026-08-04 07:53:21 UTC / 2026-08-04 09:53:21 CEST (+0200) (Warsaw)
- **Validation:** `cargo fmt --all -- --check` and `git diff --check` — passed; `cargo test --locked --workspace` plus final daemon-focused reruns — passed (63 CLI unit tests, 3 Linux process integration tests, 207 core tests, and 73 TUI tests); `cargo clippy --locked --workspace --all-targets -- -D warnings` — passed; `PYTHONPATH=sdk/python python3 -m pytest sdk/python/tests -q` — passed (53 tests, 2 skipped); `npm --prefix tests-ui run test:unit` — passed (19 tests); final `npm --prefix tests-ui run test:e2e -- --workers=1` — passed (5 tests), including no export after final-browser disconnect; release build and packaging passed; packaged checks verified missing-port rejection, explicit start, verified reuse with stable PID, implicit-mutation rejection, and explicit stop.
- **Model-token delta:** unavailable; the `/worklog-start` extension command was not available in this API session.

### File changes (`740fcfa`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `README.md` | 2 | 2 | Replaces automatic port/instance wording with explicit idempotent daemon usage. |
| `crates/embed-log-cli/src/commands/daemon.rs` | 108 | 32 | Requires an explicit port, fingerprints requested identity, reuses exact instances, rejects conflicts, requires mutation targets, and surfaces registry actions/errors. |
| `crates/embed-log-cli/src/commands/sessions.rs` | 2 | 2 | Makes titled rotation use explicit mutating endpoint resolution. |
| `crates/embed-log-cli/src/main.rs` | 6 | 2 | Requires daemon config, instance, and port through CLI argument constraints. |
| `crates/embed-log-cli/tests/daemon_lifecycle.rs` | 90 | 8 | Covers missing/occupied ports, idempotent reuse, changed-config conflict, endpoint ownership, explicit mutations, malformed registry, and foreground bind exit. |
| `crates/embed-log-core/src/net/control_ws.rs` | 1 | 8 | Removes obsolete no-client-export state from control API test fixtures. |
| `crates/embed-log-core/src/net/ws_server.rs` | 4 | 105 | Removes browser-disconnect export scheduling and leaves client-count tracking side-effect free. |
| `crates/embed-log-core/src/runtime/server.rs` | 14 | 11 | Propagates HTTP server/bind failure instead of waiting indefinitely and removes obsolete export state. |
| `docs/cli.md` | 5 | 5 | Documents explicit targeting, verified reuse, visible registry handling, and no disconnect export. |
| `mvp-embed-log-todo.md` | 8 | 6 | Updates the implementation contract to the reviewed explicit policy. |
| `tests-ui/tests/rust-server.spec.js` | 12 | 0 | Adds browser E2E proof that final-client disconnect does not export HTML. |

## 2026-08-04 08:18:31 UTC / 2026-08-04 10:18:31 CEST (Warsaw)

- **Commit:** `8b40c24` — `Add atomic UART transmit command`
- **Task:** Add explicit top-level UART TX with line/raw/file/stdin input, arm-before-write substring or regex expectations, bounded live evidence, timeout JSON, exact write acknowledgements, and Linux PTY process coverage.
- **Started:** unavailable; the `/worklog-start` extension command was not available in this API session.
- **Completed:** 2026-08-04 08:18:31 UTC / 2026-08-04 10:18:31 CEST (+0200) (Warsaw)
- **Validation:** `cargo fmt --all -- --check` and `git diff --check` — passed; final `cargo test --locked --workspace` — passed (67 CLI unit tests, 3 daemon process tests, 1 TX/PTY process test, 207 core tests, and 73 TUI tests); `cargo clippy --locked --workspace --all-targets -- -D warnings` — passed; `PYTHONPATH=sdk/python python3 -m pytest sdk/python/tests -q` — passed (53 tests, 2 skipped); `npm --prefix tests-ui run test:unit` — passed (19 tests); `npm --prefix tests-ui run test:e2e -- --workers=1` — passed (5 tests); `cargo build --locked --release` and `scripts/package-cli.sh x86_64-unknown-linux-gnu` — passed; packaged binary PTY check verified daemon startup, `tx --line probe --expect 'packaged ready'`, bounded JSON, actual byte count, and explicit stop.
- **Model-token delta:** unavailable; the `/worklog-start` extension command was not available in this API session.

### File changes (`8b40c24`)

| File | Added | Removed | Summary |
| --- | ---: | ---: | --- |
| `Cargo.lock` | 2 | 0 | Records the CLI's existing workspace WebSocket dependencies. |
| `README.md` | 3 | 1 | Adds the atomic TX/expect daemon workflow. |
| `crates/embed-log-cli/Cargo.toml` | 2 | 0 | Adds futures and Tokio Tungstenite client dependencies. |
| `crates/embed-log-cli/src/commands/daemon.rs` | 0 | 2 | Removes a racy post-release port assertion found during parallel validation. |
| `crates/embed-log-cli/src/commands/mod.rs` | 1 | 0 | Registers the TX command module. |
| `crates/embed-log-cli/src/commands/tx.rs` | 522 | 0 | Implements explicit WebSocket TX, exact inputs, atomic expectations, bounded context, timeout evidence, and protocol guards. |
| `crates/embed-log-cli/src/main.rs` | 123 | 0 | Defines and dispatches the top-level `tx` CLI with exclusive input and expectation options. |
| `crates/embed-log-cli/tests/tx_cli.rs` | 320 | 0 | Exercises line/raw/file/stdin TX, matching, timeout, target/writability rejection, wire bytes, and persistence through a PTY daemon. |
| `crates/embed-log-core/src/net/control_ws.rs` | 38 | 9 | Accepts exact byte arrays, explicit line normalization, and reports actual acknowledged wire bytes. |
| `crates/embed-log-core/src/net/ws_server.rs` | 1 | 0 | Preserves browser UART line-normalization behavior explicitly. |
| `crates/embed-log-core/src/sources/traits.rs` | 4 | 2 | Extends TX commands with line-ending policy and byte-count acknowledgements. |
| `crates/embed-log-core/src/sources/uart.rs` | 8 | 2 | Writes exact or normalized payloads and acknowledges the actual byte count. |
| `docs/agent-capabilities.md` | 12 | 0 | Documents authorized atomic UART experiments and bounded evidence. |
| `docs/cli.md` | 20 | 0 | Documents all TX inputs, matching, timeout, gap, output, and targeting semantics. |
| `mvp-embed-log-todo.md` | 3 | 1 | Marks the UART experiment milestone implemented and clarifies line behavior. |
| `sdk/python/tests/test_e2e.py` | 2 | 2 | Expects the actual normalized wire-byte count from TX acknowledgement. |
| `skills/embed-log/SKILL.md` | 12 | 0 | Teaches agents the guarded, token-bounded TX/expect workflow. |
