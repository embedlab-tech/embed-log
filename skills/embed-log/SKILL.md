---
description: Investigate firmware, hardware, UART, pytest, CoAP, and integration-test behavior with Embed-log. Use for both active capture and saved-session analysis.
---

# Embed-log investigation

Use Embed-log CLI only; never open configured UARTs or read session files directly.

## Evidence format

All log evidence must use this concise format:

```text
+time seq=N src=SOURCE#INDEX | message
```

Use only `sessions summary`, `sessions read`, `sessions search`, and `sessions around` for evidence. Do not request JSON for log-bearing commands.

## Workflow

```text
identify session/source → establish cursor → act or wait → bounded read → around relevant sequence → conclude
```

Discover unfamiliar commands with `embed-log schema`, never `--help`.

List and summarize before reading:

```bash
embed-log sessions list --dir "$LOGS_DIR" --limit 10
embed-log sessions summary "$SESSION_ID" --dir "$LOGS_DIR"
```

Read incrementally after the last observed global sequence:

```bash
embed-log sessions read "$SESSION_ID" --dir "$LOGS_DIR" \
  --after "$CURSOR" --limit 100
```

Set `CURSOR` to the final printed `seq=N`. If 100 records are returned, immediately read again before waiting. For one source, add `--source SOURCE`; omit it for cross-source ordering.

Search known evidence without scanning or dumping files:

```bash
embed-log sessions search --dir "$LOGS_DIR" --session "$SESSION_ID" \
  --source SOURCE --contains "$TEXT" --limit 20
```

Get bounded cross-source context for evidence:

```bash
embed-log sessions around "$SESSION_ID" --dir "$LOGS_DIR" \
  --sequence "$SEQUENCE" --before 10 --after 20
```

## Actions

For active capture, start/reuse the project daemon, then identify its session and sources:

```bash
embed-log run --daemon --instance NAME --config embed-log.yml
embed-log status --instance NAME --brief
```

Send UART commands only through the daemon. Treat TX output as action acknowledgement; retrieve evidence afterward with `sessions read`:

```bash
embed-log tx --instance NAME --source SOURCE --line "$COMMAND"
sleep 1
embed-log sessions read "$SESSION_ID" --dir "$LOGS_DIR" --after "$CURSOR" --limit 100
```

Use `embed-log mark --instance NAME --action reset` for an external experiment boundary. Use `embed-log export --instance NAME` only when a durable HTML report is requested.

Use JSON only for a separate orchestration step that must extract an ID, endpoint, or stable error code; never present its payload as log evidence.

Report session/source IDs, action, sequence range, concise evidence, and uncertainty.
