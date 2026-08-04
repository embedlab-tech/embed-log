---
description: Debug firmware, hardware, UART, CoAP, pytest, and integration-test behavior with live Embed-log capture. Use when reproducing a device issue and examining logs in real time, even if the user does not mention Embed-log.
---

# Embed-log live investigation

Use the CLI as the canonical interface. Never open configured UARTs directly.

## Rules

- Discover an unfamiliar CLI once with `embed-log schema`; request individual command schemas only when needed.
- Mutations require an explicit instance or supported URL. Never infer their target.
- A request to reproduce an issue with live logs authorizes starting or reusing the established project daemon.
- UART TX is a normal investigation tool; no separate confirmation is needed.
- Do not invent firmware commands. Keep reads, expectations, and context bounded.
- Leave a persistent daemon running unless asked to stop it.
- Do not parse `--help`, scrape the browser, or load whole session files.

## Ensure capture

Start or idempotently reuse the project daemon:

```bash
embed-log run --daemon --instance bench-a \
  --config embed-log.yml --json
```

The endpoint comes from `server.listen`, which defaults to `127.0.0.1:18080`; `--host` and `--port` are explicit overrides. Config and instance must come from the project or user. Ask if either is unknown.

Discover the exact session, logs directory, source IDs, health, and writable UARTs:

```bash
embed-log status --instance bench-a --json
```

Use those returned values rather than assuming `latest` or a logs directory.

## Investigate

Use:

```text
ensure daemon → status → establish cursor → reproduce → bounded read → exact context → conclusion
```

Rotate when a clean reproduction boundary is useful:

```bash
embed-log sessions new --instance bench-a \
  --title "pytest reconnect failure" --json
```

Read only records produced after the established cursor:

```bash
embed-log sessions read "$SESSION_ID" --dir "$LOGS_DIR" \
  --after "$CURSOR" --limit 100 --time relative --json
```

Continue from `next_cursor` only while `truncated` is true. Retrieve exact cross-source context around evidence:

```bash
embed-log sessions around "$SESSION_ID" --dir "$LOGS_DIR" \
  --sequence "$SEQUENCE" --before 10 --after 20 --json
```

## UART interaction

Confirm through `status` that the source is writable, then use firmware-specific values:

```bash
embed-log tx --instance bench-a --source DUT_UART \
  --line "$DEVICE_COMMAND" --expect "$EXPECTED_REPLY" \
  --timeout 30s --context 20 --json
```

The expectation is armed before TX and cannot match the TX record. For actions performed outside UART TX, use a short-lived `watch add` → `watch wait` → `watch remove` flow.

On failure, branch on structured `error.code`; inspect bounded returned context instead of dumping the session.

Report the instance, session, source IDs, reproduction action, relevant sequences, evidence, final cursor, and any timeout, stream gap, or source-health uncertainty.
