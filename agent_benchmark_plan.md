# embed-log Agent Benchmark Plan

This document describes how to benchmark whether `embed-log` agent-oriented functionality reduces token usage and improves diagnostic efficiency compared with pointing an AI agent directly at raw log files.

The benchmark should compare:

1. raw per-source files
2. raw `combined.jsonl`
3. `embed-log` CLI / future agent commands

The key question:

> Given the same deterministic log problem, how many tokens and tool calls does an agent need to identify the issue using each access method?

---

## Goals

- Measure token usage for agent log inspection.
- Compare raw files vs `combined.jsonl` vs `embed-log` CLI/agent tools.
- Use deterministic logs with known injected issues.
- Keep correctness as the primary metric.
- Avoid optimizing for low tokens if the agent gets the wrong answer.
- Identify which CLI/agent features provide the biggest token savings.

---

## Benchmark modes

### Mode A: raw file pointing

The agent is pointed at session files directly.

Prompt style:

```text
Here are the logs under logs/<session-id>/.
Find the root cause.
```

Allowed tools:

```bash
grep
rg
tail
head
sed
awk
jq
cat, if needed but discouraged
```

This simulates the common baseline of pointing an agent at log files.

---

### Mode B: raw `combined.jsonl`

The agent is told to use only the aggregated session stream.

Prompt style:

```text
Use logs/<session-id>/combined.jsonl to diagnose the issue.
```

Allowed tools:

```bash
rg
jq
tail
head
sed
awk
```

This isolates the value of having a single aggregated JSONL file.

---

### Mode C: `embed-log` CLI / agent commands

The agent uses only `embed-log` commands for inspection.

Prompt style:

```text
Use embed-log CLI commands only. Prefer compact bounded output.
Find the root cause.
```

Current and future useful commands:

```bash
embed-log sessions summary latest
embed-log sessions combined latest --lines 50 --format compact
embed-log sessions search --session latest --contains panic --context 20 --format compact
embed-log sessions search --session latest --regex 'panic|fatal|assert|watchdog' --context 20 --format compact
embed-log sessions events latest --severity fatal --format compact
embed-log agent wait --source DUT --regex 'boot complete|panic|fatal' --timeout 60 --format mini-jsonl
```

This mode measures the value of agent-oriented affordances.

---

## What to measure

### 1. Token usage

Track:

```text
total_input_tokens
total_output_tokens
total_tokens
```

Include:

- user prompt
- tool outputs
- file contents returned to the model
- assistant reasoning/final answer, if available

For early experiments, approximate tokens with:

```text
approx_tokens = chars / 4
```

Later, use a tokenizer such as `tiktoken`.

---

### 2. Tool output size

Track:

```text
tool_output_chars
tool_output_bytes
tool_output_lines
max_single_tool_output_chars
```

The `max_single_tool_output_chars` metric is useful because one overly large command output can clobber context.

---

### 3. Tool call count

Track:

```text
tool_call_count
```

Good agent-facing CLI should reduce exploratory calls.

---

### 4. Time to answer

Optional but useful:

```text
wall_time_seconds
```

---

### 5. Correctness

Correctness should be primary.

Each case should have a gold expected answer.

Example:

```yaml
expected:
  root_cause: watchdog reset after firmware update flash erase blocked main loop
  symptom: fatal watchdog reset
  sources: [HOST, COAP_NET, DUT]
  evidence:
    - source: HOST
      message_contains: firmware_update
    - source: COAP_NET
      message_contains: retransmit timeout
    - source: DUT
      message_contains: fatal: watchdog reset
```

Scoring options:

```text
0 = wrong
1 = partial
2 = correct
```

Or structured booleans:

```text
found_source: true/false
found_symptom: true/false
found_root_cause: true/false
found_correlation: true/false
```

---

## Benchmark cases

Start with two deterministic cases.

---

## Case 1: single-source obvious panic

### Purpose

- Baseline search efficiency.
- Tests whether the agent can find a clear failure quickly.
- Useful for measuring overhead/token differences between modes.

### Scenario

One source, many normal lines, one obvious issue.

```text
DUT boots normally.
DUT emits many normal status lines.
DUT logs a clear panic/assert/fatal line.
DUT emits a short backtrace.
```

Example issue:

```text
panic: assertion failed: sensor_init returned EBUSY
```

### Log shape

```text
sources: DUT
line count: 10,000
issue line: around 7,423
```

### Gold answer

```text
Root cause: sensor_init failed with EBUSY and triggered assertion panic.
Source: DUT
Approx line: 7423
Symptom: panic/assertion failure
```

### Ideal CLI query

```bash
embed-log sessions search \
  --session latest \
  --regex 'panic|assert|fatal|ERROR' \
  --context 20 \
  --format compact
```

### Expected result hypothesis

All modes should succeed, but token efficiency should rank roughly:

```text
CLI compact search > combined.jsonl grep > raw per-source files
```

---

## Case 2: multi-source causal watchdog/correlation

### Purpose

- Tests cross-source correlation.
- More representative of embedded debugging.
- Shows the value of `combined.jsonl`, events, summaries, and context search.

### Scenario

A host/network action causes DUT to block and eventually reset.

```text
HOST sends a firmware/update command.
COAP/network source shows request and retransmit timeout.
DUT logs flash erase / busy loop.
DUT misses watchdog feed.
DUT logs fatal watchdog reset.
```

### Sources

```text
HOST
DUT
COAP_NET
```

### Log shape

```text
combined line count: ~30,000
issue span: 3 sources over ~4 seconds
```

### Example causal sequence

```text
12:00:01.000 HOST      tx command firmware_update
12:00:01.050 COAP_NET  udp dst=5683 confirmable PUT /fw
12:00:03.050 COAP_NET  retransmit timeout msg_id=1234
12:00:04.600 DUT       flash erase still busy
12:00:05.000 DUT       watchdog not fed for 4000 ms
12:00:05.010 DUT       fatal: watchdog reset
```

### Gold answer

```text
Root cause: firmware update / flash erase blocked DUT long enough to miss watchdog feed.
Symptom: fatal watchdog reset.
Correlated sources: HOST command, COAP retransmit timeout, DUT watchdog logs.
```

### Ideal CLI queries

```bash
embed-log sessions summary latest

embed-log sessions events latest \
  --severity fatal \
  --format compact

embed-log sessions search \
  --session latest \
  --contains watchdog \
  --context 30 \
  --format compact

embed-log sessions search \
  --session latest \
  --from <around fatal - 5s> \
  --to <fatal + 1s> \
  --format compact
```

Future better query:

```bash
embed-log sessions search \
  --session latest \
  --event-context fatal \
  --format compact
```

### Expected result hypothesis

Correctness:

```text
CLI events/context >= combined.jsonl > raw per-source files
```

Token efficiency:

```text
CLI summary/events/context >> combined.jsonl manual jq/rg >> raw files
```

---

## Event variants

For each benchmark case, create two variants:

### Variant 1: no events

Agent must use search/context/manual inspection.

### Variant 2: events configured

Event rules detect important signals:

```yaml
DUT:
  - name: panic
    pattern: "panic|assert|fatal"
    severity: fatal
  - name: watchdog
    pattern: "watchdog"
    severity: warn
COAP_NET:
  - name: coap-timeout
    pattern: "timeout|retransmit"
    severity: warn
HOST:
  - name: firmware-update
    pattern: "firmware_update|fw update"
    severity: info
```

Then compare:

```bash
embed-log sessions events latest --severity fatal
embed-log sessions events latest --severity warn
```

This measures how much event detection reduces token usage.

---

## Benchmark matrix

Initial matrix:

| Case | Raw files | combined.jsonl | embed-log CLI |
| --- | --- | --- | --- |
| single-source panic | yes | yes | yes |
| multi-source watchdog | yes | yes | yes |

Optional event matrix:

| Case | CLI no events | CLI with events |
| --- | --- | --- |
| single-source panic | yes | yes |
| multi-source watchdog | yes | yes |

---

## Deterministic dataset generation

Create a deterministic generator.

Possible script:

```bash
scripts/generate-agent-bench-logs.py
```

Output structure:

```text
bench-data/
  single_panic/
    logs/
      2026-07-03_12-00-00/
        manifest.json
        combined.jsonl
        events.jsonl optional
        Device__DUT__session.log
    expected.yml

  multi_source_watchdog/
    logs/
      2026-07-03_12-10-00/
        manifest.json
        combined.jsonl
        events.jsonl optional
        Device__DUT__session.log
        Host__HOST__session.log
        Network__COAP_NET__session.log
    expected.yml
```

Use deterministic:

- timestamps
- line numbers
- session IDs
- source names
- issue location
- event IDs

Avoid randomness unless seeded and recorded.

---

## Expected answer format

Require the agent to answer in a strict schema.

Example:

```json
{
  "root_cause": "firmware update flash erase blocked DUT long enough to miss watchdog feed",
  "symptom": "fatal watchdog reset",
  "sources": ["HOST", "COAP_NET", "DUT"],
  "evidence": [
    {
      "source": "HOST",
      "line_idx": 1820,
      "message_contains": "firmware_update"
    },
    {
      "source": "COAP_NET",
      "line_idx": 642,
      "message_contains": "retransmit timeout"
    },
    {
      "source": "DUT",
      "line_idx": 7423,
      "message_contains": "fatal: watchdog reset"
    }
  ],
  "confidence": "high"
}
```

This makes correctness scoring easier.

---

## Token measurement

### Simple first approach

Use character counts:

```text
approx_tokens = chars / 4
```

Track per run:

```json
{
  "mode": "raw-files",
  "case": "single_panic",
  "prompt_chars": 1200,
  "tool_output_chars": 45000,
  "answer_chars": 900,
  "approx_tokens": 11775,
  "tool_calls": 8,
  "correct": true
}
```

### Better later approach

Use a tokenizer library.

For OpenAI-style tokenization:

```bash
pip install tiktoken
```

Count tokens for:

- prompt
- tool outputs
- final answer

But for early comparisons, chars/4 is good enough.

---

## Fixed prompts

Use fixed prompts for each mode to reduce benchmark variance.

### Prompt A: raw files

```text
You are diagnosing embed-log session logs.

Use the files under:
bench-data/single_panic/logs/2026-07-03_12-00-00/

Find the root cause. You may inspect files directly. Return JSON with root_cause, symptom, sources, evidence, confidence.
```

### Prompt B: combined JSONL

```text
You are diagnosing an embed-log session.

Use only:
bench-data/single_panic/logs/2026-07-03_12-00-00/combined.jsonl

Find the root cause. Return JSON with root_cause, symptom, sources, evidence, confidence.
```

### Prompt C: CLI/agent

```text
You are diagnosing an embed-log session.

Use embed-log CLI commands only. Prefer compact bounded output. Start with summary/search/events commands. Return JSON with root_cause, symptom, sources, evidence, confidence.
```

---

## Determining which mode is better

A mode is better if it gets the same or better correctness with:

1. fewer input tokens
2. fewer total tokens
3. fewer tool calls
4. smaller largest tool output
5. less time
6. less manual grep/jq reasoning

Priority order:

```text
primary: correctness
secondary: token usage
tertiary: tool calls / latency
```

Do not count a low-token wrong answer as better.

---

## Likely outcomes

### Single obvious panic

Expected token efficiency:

```text
CLI compact search > combined.jsonl grep > raw per-source files
```

Expected correctness:

```text
all modes should usually succeed
```

### Multi-source causal watchdog

Expected correctness:

```text
CLI events/context >= combined.jsonl > raw per-source files
```

Expected token efficiency:

```text
CLI summary/events/context >> combined.jsonl manual jq/rg >> raw files
```

This is where embed-log-specific agent tooling should shine.

---

## Recommended features before benchmarking CLI mode

For a fair CLI-vs-file benchmark, implement at least:

1. `latest` session alias.
2. `--format compact|jsonl|mini-jsonl`.
3. Search context:
   - `--context/-C`
   - `--before-context/-B`
   - `--after-context/-A`
4. `sessions summary latest`.

Minimal target command set:

```bash
embed-log sessions summary latest
embed-log sessions search --session latest --regex 'panic|fatal|assert|watchdog' --context 20 --format compact
embed-log sessions events latest --severity fatal --format compact
```

Without these, CLI mode may not fully represent the intended agent UX.

---

## Recommended initial benchmark implementation

1. Add deterministic log generator.
2. Generate two benchmark datasets:
   - `single_panic`
   - `multi_source_watchdog`
3. Add gold `expected.yml` for each.
4. Run each benchmark in three modes:
   - raw files
   - combined JSONL
   - embed-log CLI/agent
5. Capture:
   - correctness
   - prompt chars/tokens
   - tool output chars/tokens
   - answer chars/tokens
   - tool call count
   - wall time
6. Compare results in a table.

---

## Example result table

| Case | Mode | Correct | Approx tokens | Tool calls | Max output chars | Notes |
| --- | --- | --- | ---: | ---: | ---: | --- |
| single_panic | raw files | yes | 12,000 | 8 | 5,000 | several greps/tails |
| single_panic | combined.jsonl | yes | 7,000 | 5 | 3,000 | easier single stream |
| single_panic | CLI compact | yes | 1,800 | 2 | 900 | direct match/context |
| multi_watchdog | raw files | partial | 20,000 | 12 | 8,000 | missed COAP correlation |
| multi_watchdog | combined.jsonl | yes | 12,000 | 7 | 5,000 | manual timestamp correlation |
| multi_watchdog | CLI compact/events | yes | 3,000 | 3 | 1,200 | summary + events + context |

Numbers above are illustrative, not expected exact values.

---

## Recommendation

Yes: create two deterministic benchmark cases.

Start with:

1. **single-source panic**
2. **multi-source watchdog/correlation**

Run each in:

1. raw files mode
2. combined JSONL mode
3. embed-log CLI/agent mode

Track:

- correctness
- total tool-output chars/tokens
- final answer correctness
- tool calls
- wall time

This gives a concrete way to evaluate whether agent-oriented `embed-log` features actually reduce token cost instead of merely feeling nicer.
