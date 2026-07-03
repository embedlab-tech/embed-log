# embed-log AI Agent Improvement Plan

This document captures ideas for making `embed-log` more AI-agent friendly while keeping token usage low. The initial foundation is the session-wide `combined.jsonl` log stream, but agents should normally interact with bounded/query-oriented commands instead of reading whole log files.

## Goals

- Preserve complete durable logs for evidence and replay.
- Avoid clobbering an AI agent's context with large raw logs.
- Provide easy, fast, efficient log searching.
- Provide real-time inspection/listening capabilities.
- Let users watch the normal web UI while an agent observes the same live data.
- Favor small, bounded, structured output formats.
- Make common agent workflows deterministic and scriptable.

## Design principle

Use three layers:

1. **Raw durable truth**
   - `combined.jsonl`
   - `events.jsonl`
   - per-source `.log` files

2. **Efficient query/read tools**
   - search/filter/context commands returning small result sets
   - compact output formats
   - latest-session shortcuts

3. **Real-time low-token watch mode**
   - wait for important signals
   - watch event hits instead of raw logs where possible
   - bounded timeout/max-match behavior

The agent should usually not stream all logs into context. It should ask targeted questions and receive bounded answers.

---

## Current useful foundations

Already available or recently added:

- `combined.jsonl`: one structured append-only stream across all sources.
- `events.jsonl`: append-only event-detection hits.
- `embed-log sessions combined <SESSION_ID>`.
- `embed-log sessions tail-combined <SESSION_ID> --follow`.
- `embed-log sessions search` with structured filters.
- `embed-log sessions events <SESSION_ID>`.
- `/api/v1/control` WebSocket API:
  - `hello`
  - `subscribe`
  - `unsubscribe`
  - `log.inject`
  - `tx.write`
  - `marker.create`
- Python SDK wrapping the control API.

---

## Recommended agent-facing namespace

Consider adding a dedicated namespace:

```bash
embed-log agent ...
```

Possible commands:

```bash
embed-log agent query
embed-log agent watch
embed-log agent wait
embed-log agent watch-events
embed-log agent summary
embed-log agent context
```

A dedicated `agent` namespace allows the UX to be opinionated, compact, bounded, and token-efficient without complicating normal human-oriented CLI commands.

Alternative: keep everything under `sessions`:

```bash
embed-log sessions query
embed-log sessions watch
embed-log sessions summary
```

Preference: use `embed-log agent ...` for live/agent-specific workflows, and enhance `embed-log sessions ...` for offline/session inspection.

---

## Output formats

Current JSONL is useful but can be verbose. Add output controls to search/combined/events commands.

Recommended formats:

```text
jsonl       full existing JSONL records
compact     concise human/agent-readable text
mini-jsonl  compact structured JSONL with short keys
```

### Full JSONL example

```json
{"timestamp_iso":"2026-07-03T12:00:01.123Z","source_id":"DUT","source_label":"Device","source_kind":"uart","tab_labels":["Device"],"session_id":"2026-07-03_12-00-00","app_name":"embed-log","job_id":"nightly-42","message":"panic: watchdog reset","origin":"SERIAL","line_idx":1234,"color":null}
```

### Compact text example

```text
12:00:01.123 DUT#1234 panic: watchdog reset
```

### Mini JSONL example

```json
{"t":"12:00:01.123","s":"DUT","i":1234,"m":"panic: watchdog reset"}
```

For packet/network entries:

```json
{"t":"12:00:01.123","s":"COAP","i":42,"m":"udp ...","src":"192.168.1.2:49152","dst":"224.0.1.187:5683","len":32}
```

For events:

```json
{"t":"12:00:01.123","s":"DUT","i":42,"sev":"fatal","ev":"panic","m":"panic: watchdog reset"}
```

---

## Offline/session inspection improvements

### 1. Latest-session alias

Agents should not need to manually discover session IDs for common cases.

Support `latest` wherever a session ID is accepted:

```bash
embed-log sessions combined latest
embed-log sessions combined latest --lines 50
embed-log sessions events latest
embed-log sessions export latest --format html
embed-log sessions search --session latest --contains panic
```

Potential behavior:

- `latest` resolves to the newest session under `--dir`.
- Existing unique-prefix behavior should continue to work.

### 2. Compact/mini output for combined logs

Enhance:

```bash
embed-log sessions combined latest --lines 50 --format compact
embed-log sessions combined latest --lines 50 --format mini-jsonl
embed-log sessions tail-combined latest --follow --format compact
```

Compact output example:

```text
12:01:00.001 DUT#120 boot step 1
12:01:00.112 DUT#121 boot step 2
12:01:00.315 HOST#88 sent command status
```

### 3. Context search

Agents often need surrounding lines, not only matching lines.

Add grep-like context options:

```bash
embed-log sessions search --contains panic --context 30
embed-log sessions search --contains panic -C 30
embed-log sessions search --contains panic --before-context 20 --after-context 40
embed-log sessions search --contains panic -B 20 -A 40
```

Example output:

```text
# match 1 session=2026-07-03_12-00 source=DUT line=1234
12:00:00.991 DUT#1232 feeding watchdog
12:00:01.001 DUT#1233 task sensor blocked
12:00:01.123 DUT#1234 panic: watchdog reset   << MATCH
12:00:01.124 DUT#1235 backtrace frame 0 ...
12:00:01.125 DUT#1236 backtrace frame 1 ...
```

This is one of the highest-value improvements for debugging without dumping large logs.

### 4. Relative time windows

Current `--from` / `--to` are good, but agents benefit from relative shortcuts.

Add:

```bash
--since 10m
--since 1h
--last 500
```

Examples:

```bash
embed-log sessions search --session latest --since 10m --contains ERROR
embed-log sessions search --session latest --source DUT --last 200
```

Also consider aliases:

```bash
--after
--before
--until
```

### 5. Case-insensitive search

Add:

```bash
embed-log sessions search --contains panic --ignore-case
embed-log sessions search --regex 'fatal|panic' --ignore-case
```

### 6. Session context command

Add direct context lookup around a known line:

```bash
embed-log sessions context latest --source DUT --line 1234 --before 30 --after 30
```

This complements search and lets an agent follow up after a match.

### 7. Session summary command

Add:

```bash
embed-log sessions summary latest
```

Token-efficient output example:

```text
session: 2026-07-03_12-00-00 job=nightly-42
duration: 00:14:22
sources:
  DUT uart lines=12340 first=12:00:00 last=12:14:22
  HOST udp lines=840 first=12:00:01 last=12:14:20
events:
  fatal=1 error=3 warn=12 info=44
recent:
  12:14:19 DUT#12338 feeding watchdog
  12:14:20 HOST#839 test finished
  12:14:22 DUT#12339 shell ready
```

This should be a recommended first call for agents.

---

## Real-time agent support

Real-time support should not stream all logs by default. It should wait for specific signals or events with bounded output.

### 1. Agent watch command

Add:

```bash
embed-log agent watch
```

It should connect to `/api/v1/control`, subscribe to sources, apply filters, and print only matching entries.

Examples:

```bash
embed-log agent watch --config embed-log.yml --source DUT --contains panic --timeout 60
embed-log agent watch --url ws://127.0.0.1:8080/api/v1/control --regex 'panic|assert|fatal' --timeout 120
embed-log agent watch --source DUT --regex 'panic|fatal' --max 10 --format mini-jsonl
```

Important options:

```text
--source DUT           source filter, repeatable if needed
--contains panic       substring filter
--regex 'panic|fatal'  regex filter
--timeout 60           exit after N seconds
--max 10               exit after N matches
--quiet-until-match    print nothing unless matched
--format compact       compact output
--format mini-jsonl    compact structured output
```

### 2. Agent wait command

This is a very high-value automation primitive.

Add:

```bash
embed-log agent wait --source DUT --contains "boot complete" --timeout 30
embed-log agent wait --source DUT --regex "panic|fatal|assert" --timeout 120
```

Behavior:

- connect to control API
- subscribe to selected sources
- watch only future log lines by default
- exit `0` if matched
- exit nonzero on timeout
- print only the match, preferably compact/mini-json

Example output:

```text
MATCH 12:00:01.123 DUT#42 boot complete
```

This is ideal for agents and CI.

### 3. Event watch commands

Events are better than raw log streams for agents because they are low-noise and semantically meaningful.

Add:

```bash
embed-log agent watch-events --severity error --timeout 60 --max 5
embed-log agent wait-event --name boot-complete --timeout 30
embed-log agent wait-event --severity fatal --timeout 120
```

These should use existing event detection infrastructure and/or the control WebSocket.

---

## Event-first workflows

Events should become first-class for agent workflows.

Recommended event rule examples:

```yaml
sources:
  DUT:
    - name: panic
      pattern: "panic|assert|fatal"
      severity: fatal
    - name: watchdog
      pattern: "watchdog"
      severity: warn
    - name: boot-complete
      pattern: "boot complete|shell ready"
      severity: info
```

Recommended commands:

```bash
embed-log sessions events latest --severity fatal
embed-log agent watch-events --severity error --timeout 120 --max 10
embed-log agent wait-event --name boot-complete --timeout 60
```

Potential built-in presets:

- panic/assert/fatal
- watchdog
- reset/reboot
- boot complete / shell ready
- test pass/fail
- CoAP timeout/error

Need to be careful with defaults: presets should probably be opt-in to avoid surprising users.

---

## Control API improvements

Current active control API commands:

- `hello`
- `subscribe`
- `unsubscribe`
- `log.inject`
- `tx.write`
- `marker.create`

Potential additions:

### `event.subscribe`

Dedicated event subscription command:

```json
{
  "id": "1",
  "type": "event.subscribe",
  "severity": ["error", "fatal"]
}
```

If current `subscribe` can already subscribe to events, document and expose that clearly instead of adding a redundant command.

### `query.recent`

Allow clients to fetch a bounded recent replay buffer slice without filesystem access:

```json
{
  "id": "2",
  "type": "query.recent",
  "sources": ["DUT"],
  "limit": 50,
  "format": "mini"
}
```

This is useful for agents connected remotely to the running server.

### `query.search`

Potentially useful, but lower priority. File-based CLI search over `combined.jsonl` may be sufficient initially.

---

## Agent discovery metadata

Consider adding an agent-oriented section to the session manifest or a small `agent.json` file.

Example:

```json
{
  "agent": {
    "combined": "combined.jsonl",
    "events": "events.jsonl",
    "control_ws": "ws://127.0.0.1:8080/api/v1/control",
    "recommended_commands": [
      "embed-log sessions summary latest",
      "embed-log sessions search --session latest --contains panic --context 20",
      "embed-log agent watch-events --severity fatal --timeout 120"
    ]
  }
}
```

This is not urgent, but it can help automated tools discover the intended workflow.

---

## Recommended initial implementation order

### Phase 1: Token-efficient offline inspection

Enhance existing `sessions` commands first:

1. `latest` session alias everywhere.
2. `--format compact|jsonl|mini-jsonl` for:
   - `sessions combined`
   - `sessions search`
   - `sessions events`
3. Search context:
   - `--context/-C`
   - `--before-context/-B`
   - `--after-context/-A`
4. Relative time filters:
   - `--since 10m`
   - `--last 500`
5. `sessions summary latest`.

This provides immediate value without needing long-running WebSocket clients.

### Phase 2: Real-time agent watch/wait

Add:

```bash
embed-log agent watch
embed-log agent wait
embed-log agent watch-events
embed-log agent wait-event
```

Internally these should use `/api/v1/control`.

Important behavior:

- bounded by `--timeout`
- bounded by `--max`
- quiet until match by default for `wait`
- compact/mini-json output
- exit codes suitable for automation

### Phase 3: Event-first agent workflows

Improve event usability:

- starter `.events.yml` examples
- optional presets
- event summary counts in `sessions summary`
- documentation around event-first agent workflows

---

## Suggested recommended agent workflow

Do not recommend raw full-log reads by default.

Recommended workflow:

```bash
# 1. Get a tiny overview
embed-log sessions summary latest

# 2. Query only what matters
embed-log sessions search --session latest --contains panic --context 20 --format compact

# 3. Wait for a future signal
embed-log agent wait --source DUT --regex 'boot complete|panic|fatal' --timeout 60 --format mini-jsonl

# 4. Use events when configured
embed-log agent watch-events --severity error --timeout 120 --max 10
```

This gives:

- low token usage
- real-time support
- deterministic commands
- no context clobbering
- complete raw evidence still available in `combined.jsonl`

---

## Strong recommendation

Keep full `combined.jsonl` as the authoritative audit trail, but build agent workflows around compact, bounded commands.

The most valuable immediate features are probably:

1. `latest` alias.
2. compact/mini-json formats.
3. search context.
4. `sessions summary`.
5. `agent wait`.
6. event-first watch/wait commands.
