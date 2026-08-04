---
description: Operate Embed-log safely through its discoverable CLI: inspect bounded session evidence, target persistent daemons, rotate experiments, send atomic UART commands, and wait on retained watches. Use instead of grepping log directories or opening UARTs directly.
---

# Embed-log agent skill

Embed-log is a persistent multi-source firmware logger. It owns configured UARTs, captures UART/UDP/file logs, stores cross-source sessions, and exposes the same records to the CLI, browser, and TUI.

Use the CLI as the canonical automation interface. Do not implement a parallel serial logger, scrape the browser, or parse human `--help` output.

## Safety rules

- Never send UART data, rotate/start/stop a daemon, or change configuration without user intent.
- Never open a configured UART directly; Embed-log owns and locks it. Use `embed-log tx`.
- Never infer a target for a mutation. Supply `--instance`, `EMBED_LOG_INSTANCE`, or an explicitly supported `--url`.
- Never `cat`, recursively grep, or load `session.html`/whole `combined.jsonl` into context. Use bounded session commands.
- Prefer literal matches over regex. Use exact bytes (`--raw`, `--file`, `--stdin`) only when required.
- Capture everything, but return only bounded evidence relevant to the task.

## 1. Discover before guessing

For an unfamiliar installed version, call the compact static capability index once:

```bash
embed-log schema
```

Cache it by `schema_version` and `embed_log_version`. Query only the command needed:

```bash
embed-log schema status
embed-log schema sessions.read
embed-log schema tx
embed-log schema watch.wait
embed-log schema errors
embed-log schema config
```

Do not parse `--help`. `schema` describes binary capabilities; it does not report runtime state.

Discover a running daemon, current session, exact source IDs, and UART write capability separately:

```bash
embed-log status --instance bench-a --json
```

Read-only `status` may infer the sole daemon. Mutations never may.

## 2. Persistent experiment lifecycle

Start a named daemon only when requested. Daemon startup requires all three explicit values and never scans for another port:

```bash
embed-log run --daemon \
  --instance bench-a \
  --config embed-log.yml \
  --port 18080 \
  --json
```

Reuse is safe only when the registered live process matches the instance, endpoint, config, and logs directory. Otherwise startup fails visibly.

Rotate to a titled logical experiment without releasing UARTs or changing daemon PID:

```bash
embed-log sessions new \
  --instance bench-a \
  --title "reconnect attempt 3" \
  --json
```

Rotation resets session-global `sequence` to 1 and each source-local `line_idx` to 0. Browser/TUI clients follow the externally initiated rotation automatically.

Stop explicitly:

```bash
embed-log stop --instance bench-a --json
```

## 3. Inspect sessions with bounded evidence

Resolve logs using explicit `--dir`, then `--config`, then the configured/default logs directory. Run `embed-log doctor` if resolution is unclear.

Recommended investigation:

```text
summary → bounded read/search → exact sequence context → conclusion
```

Start small:

```bash
embed-log sessions list --limit 10
embed-log sessions summary latest --json
```

For new sessions, prefer the global cursor:

```bash
embed-log sessions read latest \
  --after 0 --limit 100 --time none --json
```

Compact JSON declares tuple fields once. Use `next_cursor` for the next page while `truncated` is true. A response is capped at 1000 records; capture is not capped.

- `sequence` is the authoritative session-global order/cursor.
- `source_id + line_idx` is the source-local identity.
- `--after N` is exclusive and applied globally before source filtering.
- `--time none` minimizes tokens.
- `--time relative` (default) preserves firmware timing.
- `--time absolute` supports external correlation.
- Use `--format full-json` only for exact stored metadata.

Get deterministic cross-source context around evidence:

```bash
embed-log sessions around latest \
  --sequence 719 --before 10 --after 20 \
  --time relative --json
```

A unique persisted event can be targeted with `--event ID`. The target plus before/after context is capped at 1000 records.

For legacy sessions or targeted text discovery:

```bash
embed-log sessions search \
  --session latest --source DUT_UART \
  --regex 'panic|fatal|watchdog' \
  --format compact --context 20
```

Use `sessions events` when configured event rules already detected relevant signatures.

Compact text has this identity shape:

```text
T+00:12.453 719 DUT_UART#428 boot complete
```

## 4. Atomic UART interaction

When TX is authorized, arm the expectation and write atomically:

```bash
embed-log tx --instance bench-a \
  --source DUT_UART \
  --line reset \
  --expect "boot complete" \
  --timeout 30s \
  --context 20 \
  --json
```

The subscription is armed before the write, TX records cannot satisfy the expectation, and returned context is bounded. On `EXPECT_TIMEOUT`, inspect `error.details.context` and `next_cursor`; do not dump the whole session. Prefer `--expect`; use `--expect-regex` only when necessary.

## 5. Retained watches for external actions

Use a temporary watch when the triggering action occurs outside `tx`:

```bash
watch_id=$(embed-log watch add --instance bench-a \
  --source DUT_UART \
  --contains "session established" \
  --ttl 30s --json | jq -r '.watch.id')

embed-log watch wait "$watch_id" \
  --instance bench-a --timeout 30s --json

embed-log watch remove "$watch_id" \
  --instance bench-a --json
```

Watches are one-shot, process-local, and retain a match that arrives before `watch wait`. Waiting does not stream ordinary logs. Keep TTLs short and remove matched/expired watches.

## 6. Machine errors

Every invocation requesting JSON emits one failure document on stdout and exits nonzero:

```json
{"ok":false,"error":{"code":"EXPECT_TIMEOUT","message":"...","details":{}}}
```

Branch on `error.code`, never message text. `COMMAND_FAILED` is the stable fallback. Use `embed-log schema errors` for the installed catalog.

## 7. Textual CoAP sources

When a source emits newline-delimited hexadecimal CoAP, configure the backend parser rather than decoding in the browser:

```yaml
sources:
  RADIO:
    type: uart
    path: /dev/ttyUSB1
    parser:
      type: hex-coap
```

The parser keeps the log prefix and replaces bytes from the first valid CoAP header with readable type, code, message ID, token, options, and payload length before persistence, watches, and streaming. Non-CoAP lines pass through unchanged.

## Completion discipline

Conclude with:

- selected instance and session;
- exact source IDs used;
- relevant sequence range or event/watch ID;
- concise evidence;
- whether output was truncated and the final cursor;
- any timeout, stream gap, parser, or source-health uncertainty.
