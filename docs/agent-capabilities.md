# Agent capabilities reference

This reference describes Embed-log capabilities agents can use now.

## Discover the installed CLI

When no Embed-log integration is installed, load the concise version-matched guidance directly from the binary:

Choose the guidance matching the task:

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

During a live investigation, use the atomic command rather than opening the serial device separately. TX needs no separate confirmation, but its firmware-specific command and expected response must come from the task, project, tests, documentation, or observed interface:

```bash
embed-log tx --instance bench-a --source DUT_UART \
  --line "$DEVICE_COMMAND" --expect "$EXPECTED_REPLY" \
  --timeout 30s --context 20 --json
```

Embed-log subscribes before writing, ignores TX records for matching, and returns the matching RX record plus bounded live context. Use `--expect-regex` only when substring matching is insufficient. `--raw`, `--file`, and `--stdin` send exact bytes; `--line` safely applies the UART line ending. An `EXPECT_TIMEOUT` response includes bounded evidence and exits unsuccessfully.

For conditions triggered outside UART TX, add a temporary retained watch:

```bash
watch_id=$(embed-log watch add --instance bench-a --source DUT_UART \
  --contains "session established" --ttl 30s --json | jq -r '.watch.id')
embed-log watch wait "$watch_id" --instance bench-a --timeout 30s --json
embed-log watch remove "$watch_id" --instance bench-a --json
```

The match is retained even if it occurs before `watch wait` starts. Watches are one-shot and temporary; remove completed or expired watches. Prefer literal `--contains` over `--regex` when possible.

## Inspect recorded sessions efficiently

Use one canonical log-record format for agent reasoning:

```text
+time seq=N src=SOURCE#INDEX | message
```

Keep JSON off log-bearing commands. Use it only for orchestration metadata or scripts that explicitly need cursor/tuple fields.

Start with an overview:

```bash
embed-log sessions summary latest --config embed-log.yml
```

Then search only relevant evidence:

```bash
embed-log sessions search --config embed-log.yml \
  --session latest --source DUT_UART \
  --regex 'panic|fatal|watchdog' \
  --context 20
```

Recommended sequence:

```text
summary → bounded read/search → exact sequence context → cross-source correlation
```

New sessions have a global sequence cursor. Prefer bounded incremental retrieval:

```bash
embed-log sessions read latest --after 100 --limit 50
embed-log sessions around latest --sequence 119 --before 5 --after 10
```

Concise text defaults to `+12.453 seq=719 src=DUT_UART#428 | boot complete`, where `719` is global and `#428` is local to `DUT_UART`. A configured merged source can be selected directly; Embed-log expands it to member records while preserving their original identities and excludes redundant materialized merge records from legacy sessions by default. Use default text for agent reasoning; add `--json` only when a script needs tuple fields or cursor metadata. The JSON envelope always uses `time`, `sequence`, `source`, `index`, and `message` fields.

## Subscribe to live logs

Connect to the control WebSocket:

```text
ws://127.0.0.1:18080/api/v1/control
```

Subscribe only to the sources relevant to the investigation:

```json
{
  "id": "sub-1",
  "type": "subscribe",
  "sources": ["DUT_UART", "PYTEST"]
}
```

The server sends structured `log.entry` messages. Use temporary watches when a process-local condition must be retained without keeping a subscription open.

## Agent guardrails

- Call `/api/v1/status` before assuming source IDs.
- Start with `sessions summary`.
- Keep live subscriptions and context windows bounded.
- Remove temporary watches after investigation.
- Use UART TX when relevant to a live investigation, but do not invent firmware commands; do not delete session data, export sensitive logs, or edit project configuration without explicit approval.

The canonical investigation skill is available from the repository and through `embed-log skill`; use this document as the extended capability reference.
