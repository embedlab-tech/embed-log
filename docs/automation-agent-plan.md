# Automation and agent improvement plan

This plan makes Embed-log efficient and safe for agents, scripts, and UI-driven debugging. It builds on the existing session, event, control-WebSocket, and Python SDK foundations; it does not introduce a parallel watcher/event system.

## Goals

- Let an agent find relevant evidence without reading whole logs.
- Let users and agents turn a meaningful log signature into a durable session event.
- Keep one event model across YAML, UI, API, SDK, TUI, reports, and `events.jsonl`.
- Make protocol capabilities discoverable to clients in any language.
- Keep automation observational and bounded by default.

## Existing foundation

Embed-log already provides:

- normal project runs with `embed-log run --config embed-log.yml`;
- session manifest, `combined.jsonl`, `events.jsonl`, markers, and HTML reports;
- compact session summary/search/events commands;
- a live control WebSocket at `ws://127.0.0.1:8080/api/v1/control`;
- Python SDK subscription, TX, injection, marker, and watcher helpers;
- companion `.events.yml` rules compiled into `EventRule`/`PatternMatcher`;
- one event path: match → WebSocket broadcast → `events.jsonl` → event marker → report/TUI.

## Phase 1 — Agent investigation skill

Create a project skill at `.agents/skills/embed-log/SKILL.md`.

### Default investigation sequence

```text
sessions summary
→ events / cheap match count
→ targeted search with context
→ cross-source correlation
→ concise evidence-backed conclusion
```

Commands should default to the project config:

```bash
embed-log sessions summary latest --config embed-log.yml
embed-log sessions events latest --config embed-log.yml --format compact
embed-log sessions search --config embed-log.yml --session latest \
  --source DUT_UART --regex 'watchdog|panic|fatal' --count
embed-log sessions search --config embed-log.yml --session latest \
  --source DUT_UART --regex 'watchdog|panic|fatal' \
  --format compact --limit 5 --context 20
```

### Skill guardrails

- Start with `sessions summary`; report the resolved session and sources.
- Prefer `compact` for reasoning and `mini-jsonl` for structured processing.
- Escalate to raw JSONL only when exact fields are required.
- Do not start a capture, send TX, import/export/bundle, or prune without explicit user intent.
- Bound live observation time, context size, and match count.
- Do not assume source IDs; use those reported by the summary/manifest.

## Phase 2 — Dynamic rules using the existing event pipeline

Do not create a separate runtime watcher/event implementation. Extend the existing `EventRule` and `PatternMatcher` path.

Today event matchers are loaded at startup from `.events.yml` and cloned into writer tasks. Refactor them into a shared mutable source-rule registry, for example:

```text
Arc<RwLock<HashMap<source_id, PatternMatcher>>>
```

Add live control operations:

```text
event_rule.create
event_rule.list
event_rule.delete
event_rule.enable
event_rule.disable
```

Example request:

```json
{
  "type": "event_rule.create",
  "source_id": "DUT_UART",
  "name": "watchdog-reset",
  "pattern": "watchdog reset after \\d+s",
  "severity": "error",
  "scope": "runtime"
}
```

Future matching lines must retain the current behavior:

```text
PatternMatcher match
→ event broadcast
→ append events.jsonl
→ event marker
→ browser/TUI/report visibility
```

Persist runtime-rule metadata in the session manifest for reproducibility. Runtime/session scope is the default; exporting a YAML snippet is preferred over silently rewriting a project `.events.yml` file.

## Phase 3 — Browser rule creation

When a user selects a line, expose distinct actions:

```text
Add marker
Create event rule
Copy as regex
```

The event-rule editor should prefill source, a generated name, selected text/pattern, and severity. It should validate regexes and test the candidate rule against replayed/current logs before enabling it.

Initial options:

- source ID;
- name;
- exact/contains/regex pattern;
- severity;
- runtime/session scope;
- historical match count.

Later controls:

- one-shot rule;
- cooldown;
- maximum matches;
- enable/disable;
- generated YAML snippet;
- user/agent creator metadata.

## Phase 4 — Bounded event evidence

Extend the existing event payload/model with optional context snapshots rather than creating a separate evidence system:

```yaml
context:
  before: 20
  after: 40
```

A persisted event should retain its existing source, line, timestamp, severity, message, and captures, plus bounded before/after context when configured. This gives agents enough evidence to diagnose a match without loading the full log stream.

## Phase 5 — Protocol and capability discovery

Python remains a convenience SDK, not the required automation interface. Browser UI, CLI, SDKs, agents, and third-party clients should use the same language-neutral protocol.

Add runtime discovery:

```bash
embed-log capabilities --json
# or: embed-log api capabilities --config embed-log.yml --json
```

Report:

- API and Embed-log versions;
- control WebSocket URL;
- active session ID;
- source IDs, labels, kinds, and TX/writable status;
- supported subscription, marker, export, and event-rule operations;
- optional features such as pcap;
- future limits/authentication metadata.

Publish protocol schemas:

```text
/api/v1/openapi.json
/api/v1/asyncapi.json
```

OpenAPI describes HTTP request/response operations; AsyncAPI describes WebSocket commands and live messages. Capability discovery answers what the currently running instance enables.

## Token-efficiency policy

| Situation | Preferred input |
|---|---|
| Orient in a session | `sessions summary` |
| Check an idea cheaply | `sessions search --count` |
| Read matches | `search --format compact --context N` |
| Structured tool processing | `--format mini-jsonl` |
| Live monitoring | server-side/client-side filtered WebSocket subscription |
| Exact forensic data | full JSONL or raw source file, only on escalation |

Do not frame this as replacing `grep`. `grep` is ideal for retrospective static-text searches. Embed-log rules are for turning an important recurring signature into a live, timestamped, source-aware, durable session finding.

## Safety defaults

Agent auto mode is observation and annotation only unless explicitly authorized. It may subscribe, search, create bounded runtime event rules, and create markers/events. It must not send UART commands, restart devices, edit project config, import logs, delete sessions, or run indefinitely by default.

## Suggested delivery order

1. Add the Embed-log agent skill and test it against representative recorded sessions.
2. Refactor static event matchers into a shared mutable runtime registry with tests.
3. Add WebSocket event-rule CRUD and persistence/replay metadata.
4. Add browser line action and rule editor.
5. Add optional context/cooldown/max-match behavior to the existing event model.
6. Add capabilities JSON and OpenAPI/AsyncAPI documents.
7. Keep the Python SDK as a thin typed client of the finalized protocol.
