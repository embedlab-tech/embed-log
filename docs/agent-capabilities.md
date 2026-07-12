# Agent capabilities reference

This reference describes Embed-log capabilities agents can use now. For the broader roadmap, see [Automation and agent plan](automation-agent-plan.md).

## Discover a running server

```bash
curl -fsS http://127.0.0.1:8080/api/v1/status
```

The status response identifies the active session, exact source IDs, source type/label, UART write capability, control-API availability, and source counters. See [Status and capabilities API](api-status.md).

Agents must discover source IDs rather than guessing them.

## Inspect recorded sessions efficiently

Start with an overview:

```bash
embed-log sessions summary latest --config embed-log.yml
```

Then inspect persisted events and search only relevant evidence:

```bash
embed-log sessions events latest --config embed-log.yml --format compact

embed-log sessions search --config embed-log.yml \
  --session latest --source DUT_UART \
  --regex 'panic|fatal|watchdog' \
  --format compact --context 20
```

Recommended sequence:

```text
summary → events → narrow search/count → bounded context → cross-source correlation
```

Prefer `compact` for reasoning and `mini-jsonl` for structured processing. Read full JSONL only when exact fields are required.

## Subscribe to live logs and events

Connect to the control WebSocket:

```text
ws://127.0.0.1:8080/api/v1/control
```

Subscribe to sources and backend-detected events:

```json
{
  "id": "sub-1",
  "type": "subscribe",
  "sources": ["DUT_UART", "PYTEST"],
  "events": true
}
```

The server sends `log.entry` and `event` messages. An event contains its rule ID, source, severity, timestamps, line index, message, and regex captures.

## Create runtime event rules

Create a rule without editing YAML:

```json
{
  "id": "rule-1",
  "type": "event_rule.create",
  "source_id": "DUT_UART",
  "name": "agent-watchdog-reset",
  "pattern": "watchdog reset after \\d+s",
  "severity": "error"
}
```

Future matches use the standard path:

```text
broadcast event → events.jsonl → event marker → Events view
```

Runtime rules remain active for the current Embed-log process/session.

## Manage rules

List active static and runtime rules:

```json
{ "id": "rules-1", "type": "event_rule.list" }
```

Each result includes `source_id`, `name`, `pattern`, `severity`, and `origin` (`static` or `runtime`).

Export active rules as companion YAML:

```json
{ "id": "rules-2", "type": "event_rule.export" }
```

Delete a runtime rule:

```json
{
  "id": "rules-3",
  "type": "event_rule.delete",
  "source_id": "DUT_UART",
  "name": "agent-watchdog-reset"
}
```

Persist it for future runs:

```json
{
  "id": "rules-4",
  "type": "event_rule.promote",
  "source_id": "DUT_UART",
  "name": "agent-watchdog-reset"
}
```

Promotion writes `<config-stem>.events.yml`. The runtime rule stays active now; the saved static rule loads on the next run.

## Agent guardrails

- Call `/api/v1/status` before assuming source IDs.
- Start with `sessions summary`.
- Keep live subscriptions and context windows bounded.
- Give temporary rules purpose-specific names and delete them after investigation.
- Promote only rules worth retaining.
- Do not send UART TX, prune sessions, import logs, or edit project configuration without explicit approval.

A dedicated Embed-log agent skill is planned at `.agents/skills/embed-log/SKILL.md`; until then, use this reference in project agent instructions or task prompts.
