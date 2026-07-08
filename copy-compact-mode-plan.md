# Copy Compact / Agent-Friendly Selection Plan

This document describes a human-in-the-loop workflow for copying selected log evidence from the `embed-log` UI into an AI agent context without wasting tokens.

The user often visually inspects logs in the web UI, selects suspicious lines or a time range, and then wants to paste a concise representation into an AI agent for analysis. The goal is to support that workflow with low-token copy formats.

---

## Core recommendation

Do **not** add a new selection mode such as `All-Compact`.

Instead, keep these two concepts separate:

```text
What lines are included?
  Exact / Context / Selected panes / All

How are they copied?
  Raw text / Compact text / Mini JSONL / Full JSONL
```

Adding modes like this would create mode explosion:

```text
Exact
All
Selected
All-Compact
Selected-Compact
Exact-Compact
```

A cleaner model is:

```text
scope = what evidence
copy format = how to serialize it
```

So the UI should add output actions such as:

```text
Copy
Copy for Agent
Copy Mini JSONL
```

or a dropdown:

```text
Copy ▾
  Text
  Compact for Agent
  Mini JSONL
  Full JSONL
```

---

## Current useful selection scopes

The existing selection concepts are valuable and should remain focused on choosing evidence.

### Exact

Only explicitly selected lines in the current pane.

Good for:

```text
one suspicious line or a short selected range
```

### Context

Selected time range plus sibling panes around the same timestamps.

Good for:

```text
what happened around this issue across DUT/HOST/network
```

### Selected panes / All panes

Include the selected range/context but only for enabled panes, or for all panes.

Good for:

```text
exclude noisy pane, include DUT + HOST only
```

All copy formats should respect the currently selected scope.

Example:

```text
Scope: Context
Action: Copy for Agent
```

Should output compact cross-pane evidence.

---

## Recommended UI

### Minimal UI

Add one new button near existing selection controls:

```text
[Copy] [Copy for Agent]
```

Tooltip:

```text
Copy for Agent: compact timestamp/source/line format for pasting into AI tools.
```

### Slightly richer UI

```text
[Copy Text] [Copy Compact] [Copy JSONL]
```

### Best long-term UI

```text
Copy ▾
  Text
  Compact for Agent
  Mini JSONL
  Full JSONL
```

Or:

```text
[Copy] [Copy for Agent] [More ▾]
```

Where `More` contains:

```text
Copy Compact
Copy Mini JSONL
Copy Full JSONL
Download Compact
Download Mini JSONL
```

For initial implementation, prefer:

```text
Copy for Agent
```

because it is simple and user-friendly.

---

## Output formats

### 1. Current text copy

Keep the existing behavior for human-readable copying.

Example:

```text
[DUT]
[12:00:01.123] boot complete
[12:00:02.345] panic: watchdog reset

[HOST]
[12:00:01.000] sent command status
```

This is good for humans and should remain available.

---

### 2. Compact agent text

This should be the default format for `Copy for Agent`.

Example:

```text
# embed-log evidence
# session=2026-07-03_12-00-00 scope=context panes=DUT,HOST,COAP_NET lines=6

12:00:01.000 HOST#88 sent command firmware_update
12:00:01.050 COAP_NET#642 udp dst=5683 confirmable PUT /fw
12:00:03.050 COAP_NET#643 retransmit timeout msg_id=1234
12:00:04.600 DUT#7421 flash erase still busy
12:00:05.000 DUT#7422 watchdog not fed for 4000 ms
12:00:05.010 DUT#7423 fatal: watchdog reset
```

Advantages:

- readable by humans
- very token-efficient
- preserves timestamp/source/line index
- easy for an agent to cite evidence
- causal order is clear when sorted by timestamp

---

### 3. Mini JSONL

Useful when the agent/tooling wants structure.

Example:

```jsonl
{"t":"12:00:01.000","s":"HOST","i":88,"m":"sent command firmware_update"}
{"t":"12:00:01.050","s":"COAP_NET","i":642,"m":"udp dst=5683 confirmable PUT /fw"}
{"t":"12:00:03.050","s":"COAP_NET","i":643,"m":"retransmit timeout msg_id=1234"}
{"t":"12:00:04.600","s":"DUT","i":7421,"m":"flash erase still busy"}
{"t":"12:00:05.000","s":"DUT","i":7422,"m":"watchdog not fed for 4000 ms"}
{"t":"12:00:05.010","s":"DUT","i":7423,"m":"fatal: watchdog reset"}
```

Mini JSONL is more structured, but slightly less readable than compact text.

---

### 4. Full JSONL

Useful for files/tools, but probably not the default for copying into an AI chat.

Example:

```jsonl
{"timestamp_iso":"...","source_id":"DUT","source_label":"Device","source_kind":"uart","line_idx":7423,"message":"fatal: watchdog reset"}
```

This is too verbose for normal paste-to-agent usage, but should be available later under advanced options.

---

## Sorting behavior

For `Copy for Agent`, output should be sorted by absolute timestamp ascending.

Why:

- cross-source causal order matters more than pane grouping
- agents reason better from a chronological sequence
- this is better for DUT/HOST/network correlation

Preferred:

```text
12:00:01.000 HOST#88 ...
12:00:01.050 COAP_NET#642 ...
12:00:04.600 DUT#7421 ...
```

Less ideal for agent analysis:

```text
[HOST]
...
[DUT]
...
[COAP]
...
```

The normal `Copy` action can keep the existing grouping if that is better for human reading.

---

## Metadata header

`Copy for Agent` should include a small header with minimal metadata.

Example:

```text
# embed-log evidence
# session=2026-07-03_12-00-00
# scope=context
# panes=DUT,HOST,COAP_NET
# lines=6
```

Do **not** include large manifest/config data.

The header gives the agent enough context without wasting tokens.

---

## Token estimate in UI

This is a high-value feature and should be treated as part of the core agent-copy UX, not just a nice-to-have.

The UI should show an approximate token count for the evidence that would be copied. This gives users immediate feedback before they paste into an agent and helps them learn how to keep context small.

### Where to show it

Show token estimates directly in the selection/action popup.

Example:

```text
Selected: 42 lines
Compact: ~1.8k tokens
Raw text: ~4.7k tokens
Mini JSONL: ~2.4k tokens
```

If space is limited:

```text
42 lines · ~1.8k tokens for Agent Copy
```

After copying:

```text
Copied for agent: 42 lines, ~1.8k tokens
```

### Why this matters

This helps users avoid accidental context clobbering. Instead of pasting a huge block and only realizing later that the agent context is polluted, the UI makes the cost visible at selection time.

It also encourages better human-in-the-loop behavior:

- select a smaller range
- use Context only when needed
- disable noisy panes
- prefer compact agent copy over raw text

### Approximation

A simple approximation is enough:

```js
tokens = Math.ceil(text.length / 4)
```

This is not exact across all models/tokenizers, but it is good enough for UI guidance.

### Optional token budget colors

Use simple visual thresholds:

```text
< 2k tokens      green
2k–8k tokens     yellow
8k–16k tokens    orange
> 16k tokens     red
```

Example UI copy:

```text
~1.8k tokens · safe to paste
~6.4k tokens · medium
~13k tokens · large selection
~24k tokens · likely too large
```

### Optional per-format comparison

When multiple copy formats are available, show the estimated size for each:

```text
Copy Text       ~4.7k tokens
Copy for Agent  ~1.8k tokens
Mini JSONL      ~2.4k tokens
Full JSONL      ~8.9k tokens
```

This makes the benefit of `Copy for Agent` obvious.

### Implementation note

Compute the estimate by generating the candidate output string for each format and applying `Math.ceil(text.length / 4)`. This keeps the estimate honest and avoids maintaining a separate estimator that can drift from the real clipboard output.

---

## Truncation / safety guard

For agent copy, avoid accidental giant clipboard payloads.

If estimated compact output exceeds a threshold, show a confirmation.

Example:

```text
This selection is ~18k tokens. Copy anyway?
[Copy anyway] [Copy first 200 lines] [Cancel]
```

Suggested default threshold:

```text
8k to 12k estimated tokens
```

This directly addresses context clobbering.

---

## Copy for Agent behavior

Define `Copy for Agent` as:

```text
Copy the currently scoped evidence in compact chronological text format.
```

Where currently scoped means:

- Exact mode: only explicitly selected lines
- Context mode: lines from sibling panes within selected time window
- Selected panes mode: only checked panes
- All mode: all panes in range

Output:

- compact text
- timestamp sorted
- line format: `<time> <source>#<line_idx> <message>`
- small metadata header
- token estimate in confirmation/status

---

## Optional: Copy Agent Prompt

A future enhancement could add:

```text
Copy Agent Prompt
```

This would include the compact evidence plus a short instruction.

Example:

```text
Analyze this embed-log evidence. Identify the likely root cause, symptom, and cite source#line evidence.

# embed-log evidence
# session=2026-07-03_12-00-00
# scope=context panes=HOST,COAP_NET,DUT lines=6

12:00:01.000 HOST#88 sent command firmware_update
12:00:01.050 COAP_NET#642 udp dst=5683 confirmable PUT /fw
12:00:03.050 COAP_NET#643 retransmit timeout msg_id=1234
12:00:04.600 DUT#7421 flash erase still busy
12:00:05.000 DUT#7422 watchdog not fed for 4000 ms
12:00:05.010 DUT#7423 fatal: watchdog reset
```

This is very user-friendly, but should probably come after the simpler `Copy for Agent` action.

Later this could connect to prompt templates.

---

## Minimal implementation proposal

### UI

Add one button:

```text
Copy for Agent
```

near existing copy/download/export selection controls.

### Format

Use compact timestamp/source/line text:

```text
# embed-log evidence
# session=<id> scope=<scope> panes=<ids> lines=<n>
<time> <source>#<line_idx> <message>
...
```

### Sorting

Sort by absolute timestamp ascending.

### Feedback

After copy:

```text
Copied for agent: 37 lines, ~1.4k tokens
```

If over threshold:

```text
Selection is ~13k tokens. Copy anyway?
```

### Later additions

Add dropdown options:

```text
Copy Mini JSONL
Copy Full JSONL
Download Compact
Download Mini JSONL
```

---

## Recommendation

Do **not** add `All-Compact` as a new mode.

Do add:

```text
Copy for Agent
```

as a new action that respects the current selection scope.

Later add advanced formats under a dropdown:

```text
Copy ▾
  Text
  Compact for Agent
  Mini JSONL
  Full JSONL
```

This keeps the mental model clean:

```text
scope = what evidence
copy format = how to serialize it
```

This is the cleanest UX and the best match for AI-agent usage.
