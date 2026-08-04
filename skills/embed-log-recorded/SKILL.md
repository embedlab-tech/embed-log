---
description: Investigate firmware, hardware, UART, CoAP, pytest, and integration-test failures in saved Embed-log sessions. Use when a failure already occurred and the user asks to inspect logs, reconstruct a timeline, find errors, or compare recorded runs.
---

# Embed-log recorded-session analysis

Use bounded `embed-log sessions` commands. A running daemon is not required.

## Rules

- Do not start or modify a daemon for retrospective analysis.
- Prefer explicit `--dir` or `--config`; report the resolved session rather than only `latest`.
- Do not recursively grep session directories or load whole `combined.jsonl` or `session.html` files.
- Start with summaries or counts and retrieve only relevant evidence.
- Prefer literal matching; use regex only when necessary.
- Discover unfamiliar commands through `embed-log schema`, not `--help`.
- Treat configured merges as virtual source filters: selecting one expands to original member records without changing `source_id` or `sequence`. Legacy materialized merge records are excluded by default; do not enable compatibility output unless explicitly needed.

## Investigate

Use:

```text
list → summary → count/search → exact context → conclusion
```

Resolve and summarize the concrete session:

```bash
embed-log sessions list --dir "$LOGS_DIR" --limit 10 --json
embed-log sessions summary "$SESSION_ID" --dir "$LOGS_DIR" --json
```

Check a supplied error literally before using regex:

```bash
embed-log sessions search --dir "$LOGS_DIR" \
  --session "$SESSION_ID" --contains "$ERROR_TEXT" --count
```

Retrieve bounded matches:

```bash
embed-log sessions search --dir "$LOGS_DIR" \
  --session "$SESSION_ID" --contains "$ERROR_TEXT" \
  --limit 20 --format compact --context 10
```

For sessions with global sequences, use bounded cursor reads:

```bash
embed-log sessions read "$SESSION_ID" --dir "$LOGS_DIR" \
  --after "$CURSOR" --limit 100 --time none --json
```

Continue from `next_cursor` only while `truncated` is true. `sequence` is session-global; `source_id + line_idx` is source-local. Retrieve deterministic cross-source context around relevant evidence:

```bash
embed-log sessions around "$SESSION_ID" --dir "$LOGS_DIR" \
  --sequence "$SEQUENCE" --before 10 --after 20 --json
```

When comparing runs, summarize both first and retrieve context around the first meaningful divergence rather than reading both sessions completely. Use `sessions search` for legacy sessions without global sequences.

If only a standalone export is available, filter it in bounded chunks and preserve session, source, sequence, and timestamp identity; do not pretend it is a session directory.

Branch on structured `error.code`, not message text.

Report the logs directory, concrete session and source IDs, searched signature, relevant sequence range, concise evidence, final cursor/truncation state, and any invalid-record, missing-source, timing, or legacy-session uncertainty.
