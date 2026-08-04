---
description: Safely operate Embed-log through its CLI: discover capabilities, target daemons, rotate experiments, inspect bounded logs, send UART commands, and wait for retained matches. Use instead of grepping logs or opening owned UARTs.
---

# Embed-log agent skill

Embed-log is a persistent UART/UDP/file logger. Use its CLI as the canonical automation interface.

## Rules

- Do not open configured UARTs directly; use `embed-log tx`.
- Do not start, stop, rotate, or transmit without user intent.
- Mutations require explicit `--instance`, `EMBED_LOG_INSTANCE`, or supported `--url`; never infer a target.
- Do not parse `--help`, scrape the browser, grep session directories, read all `combined.jsonl`, or load `session.html`.
- Use bounded commands and return only relevant evidence.
- Prefer literal matches; use regex or exact raw bytes only when necessary.

## Discover

For an unfamiliar binary, call once and cache by `schema_version` plus `embed_log_version`:

```bash
embed-log schema
```

Request details only when needed:

```bash
embed-log schema sessions.read
embed-log schema tx
embed-log schema errors
```

Static schema does not contain runtime state. Discover the selected daemon, session, exact source IDs, and writable UARTs with:

```bash
embed-log status --instance bench-a --json
```

Only read-only `status` may infer the sole daemon.

## Experiment lifecycle

```bash
embed-log run --daemon --instance bench-a \
  --config embed-log.yml --port 18080 --json

embed-log sessions new --instance bench-a \
  --title "reconnect attempt 3" --json

embed-log stop --instance bench-a --json
```

Daemon startup requires explicit config, instance, and port. Rotation keeps UART ownership and daemon PID, resets global `sequence` to 1 and source-local `line_idx` to 0, and notifies browser/TUI clients.

## Bounded investigation

Use:

```text
summary → bounded read/search → exact context → conclusion
```

```bash
embed-log sessions list --limit 10
embed-log sessions summary latest --json

embed-log sessions read latest \
  --after 0 --limit 100 --time none --json

embed-log sessions around latest \
  --sequence 719 --before 10 --after 20 \
  --time relative --json
```

- `sequence` is the session-global order/cursor.
- `source_id + line_idx` is source-local identity.
- `--after N` is exclusive and applied before source filtering.
- Continue from `next_cursor` while `truncated` is true.
- Responses and around-context are capped at 1000 records; capture is not capped.
- Use `--time none` for minimum tokens, relative for firmware timing, absolute for external correlation.
- Use `--format full-json` only for exact metadata.

For legacy sessions or targeted matching:

```bash
embed-log sessions search --session latest --source DUT_UART \
  --regex 'panic|fatal|watchdog' --format compact --context 20
```

Compact identity:

```text
T+00:12.453 719 DUT_UART#428 boot complete
```

## UART TX

When authorized:

```bash
embed-log tx --instance bench-a --source DUT_UART \
  --line reset --expect "boot complete" \
  --timeout 30s --context 20 --json
```

The expectation is armed before writing and TX records cannot satisfy it. Prefer `--expect`; use `--expect-regex` only when needed. On `EXPECT_TIMEOUT`, inspect `error.details.context` and `next_cursor`, not the whole session.

## Retained watch

For behavior triggered outside TX:

```bash
watch_id=$(embed-log watch add --instance bench-a \
  --source DUT_UART --contains "session established" \
  --ttl 30s --json | jq -r '.watch.id')
embed-log watch wait "$watch_id" --instance bench-a --timeout 30s --json
embed-log watch remove "$watch_id" --instance bench-a --json
```

Watches are one-shot and process-local. A match is retained if it arrives before `wait`. Waiting does not stream ordinary logs.

## Errors

JSON failures use one stdout document and a nonzero exit status:

```json
{"ok":false,"error":{"code":"EXPECT_TIMEOUT","message":"...","details":{}}}
```

Branch on `error.code`, never message text. `COMMAND_FAILED` is the fallback. Discover codes with `embed-log schema errors`.

## Textual CoAP

For newline-delimited hexadecimal CoAP, attach the backend parser:

```yaml
parser:
  type: hex-coap
```

It keeps the line prefix, replaces bytes from the first valid CoAP header with a readable decode before persistence/streaming, and passes non-CoAP lines unchanged.

## Report

Conclude with the selected instance/session, source IDs, relevant sequence range or event/watch ID, concise evidence, final cursor/truncation state, and any timeout, stream-gap, parser, or source-health uncertainty.
