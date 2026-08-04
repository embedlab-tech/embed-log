# Agent capabilities reference

This reference describes Embed-log capabilities agents can use now. For the broader roadmap, see [Automation and agent plan](automation-agent-plan.md).

## Discover the installed CLI

When no Embed-log integration is installed, load the concise version-matched guidance directly from the binary:

```bash
embed-log skill
```

Do not parse human `--help` output. After loading/caching the skill, use the compact runtime-independent capability index, then inspect only the command needed:

```bash
embed-log schema
embed-log schema sessions.read
embed-log schema tx
embed-log schema errors
```

Descriptors include actual arguments plus mutation, targeting, limits, output, and stable-error semantics. Cache them by `schema_version` and `embed_log_version`. Every invocation requesting JSON returns one structured failure document on stdout with a stable code and nonzero exit status; `COMMAND_FAILED` is the fallback when no narrower classification applies.

## Discover a running server

Static schema does not replace runtime discovery. Query the selected daemon for its current session and sources:

```bash
curl -fsS http://127.0.0.1:18080/api/v1/status
```

The status response identifies the active session, exact source IDs, source type/label, UART write capability, control-API availability, and source counters. See [Status and capabilities API](api-status.md).

Agents must discover source IDs rather than guessing them.

## Send UART commands with bounded expectations

When UART TX is explicitly authorized, use the atomic command rather than opening the serial device separately:

```bash
embed-log tx --instance bench-a --source DUT_UART \
  --line reset --expect "boot complete" \
  --timeout 30s --context 20 --json
```

Embed-log subscribes before writing, ignores TX records for matching, and returns the matching RX record plus bounded live context. Use `--expect-regex` only when substring matching is insufficient. `--raw`, `--file`, and `--stdin` send exact bytes; `--line` safely applies the UART line ending. An `EXPECT_TIMEOUT` response includes bounded evidence and exits unsuccessfully.

For events triggered outside UART TX, add a temporary retained watch:

```bash
watch_id=$(embed-log watch add --instance bench-a --source DUT_UART \
  --contains "session established" --ttl 30s --json | jq -r '.watch.id')
embed-log watch wait "$watch_id" --instance bench-a --timeout 30s --json
embed-log watch remove "$watch_id" --instance bench-a --json
```

The match is retained even if it occurs before `watch wait` starts. Watches are one-shot and temporary; remove completed or expired watches. Prefer literal `--contains` over `--regex` when possible.

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
summary → bounded read/search → exact sequence context → cross-source correlation
```

New sessions have a global sequence cursor. Prefer bounded incremental retrieval:

```bash
embed-log sessions read latest --after 100 --limit 50 --time none --json
embed-log sessions around latest --sequence 119 --before 5 --after 10 --time relative --json
```

Compact text defaults to `T+00:12.453 719 DUT_UART#428 boot complete`, where `719` is global and `#428` is local to `DUT_UART`. Use `--time none` when order is sufficient, `--time relative` for latency, and `--time absolute` for external correlation. Use `--format full-json` only when complete metadata is required.

Prefer `compact` for reasoning and `mini-jsonl` for structured processing. Read full JSONL only when exact fields are required.

## Subscribe to live logs and events

Connect to the control WebSocket:

```text
ws://127.0.0.1:18080/api/v1/control
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
- Do not send UART TX, delete session data, export sensitive logs, or edit project configuration without explicit approval.

A dedicated Embed-log agent skill is planned at `.agents/skills/embed-log/SKILL.md`; until then, use this reference in project agent instructions or task prompts.
