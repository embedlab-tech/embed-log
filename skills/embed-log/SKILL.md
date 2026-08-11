---
description: Investigate live firmware/test behavior and saved Embed-log sessions.
---

# Embed-log investigation

Use Embed-log CLI only. Never open configured UARTs or read session files directly.

## Evidence

Use only `sessions summary`, `sessions read`, `sessions search`, and `sessions around` for log evidence. Keep evidence in this format:

```text
+time seq=N src=SOURCE#INDEX | message
```

Do not use JSON for log-bearing commands. Do not dump complete logs. For an unfamiliar command, use `embed-log schema`.

## Workflow

```text
doctor → identify session/source → cursor → act/wait → read → around → conclude
```

Start with configuration and physical-source discovery:

```bash
embed-log doctor
```

Use bounded readers:

```bash
embed-log sessions summary latest --dir "$LOGS_DIR"
embed-log sessions read "$SESSION_ID" --dir "$LOGS_DIR" \
  --after "$CURSOR" --limit 100
# add --source "$SOURCE" for one physical source
embed-log sessions search --dir "$LOGS_DIR" --session "$SESSION_ID" \
  --source SOURCE --contains "$TEXT" --limit 20
embed-log sessions around "$SESSION_ID" --dir "$LOGS_DIR" \
  --sequence "$SEQUENCE" --before 10 --after 20
```

Set `CURSOR` to the final printed `seq=N`. If a read returns 100 records, read again immediately.

## Actions

For active capture, reuse the daemon or start it with the project config:

```bash
embed-log run --daemon --instance NAME --config "$CONFIG"
```

Send UART commands only through Embed-log, then read session evidence:

```bash
embed-log tx --instance NAME --source SOURCE --line "$COMMAND"
sleep 1
embed-log sessions read "$SESSION_ID" --dir "$LOGS_DIR" --after "$CURSOR" --limit 100
```

Use JSON only to extract an ID, endpoint, or stable error code; never present it as log evidence.
