# Curated demo plan for website export

## Goal
Prepare one polished `embed-log` demo session and its exported `session.html` for the marketing website.

The demo must make `embed-log` look like a serious embedded debugging tool, not a generic log tail. The export will be embedded on the website, so the session must be intentionally authored for first-time interaction.

## Important product constraint
Do **not** treat the exported HTML and the live runtime as the same demo surface.

From the current frontend capability split:
- static/exported HTML disables live-only actions like WS status, `Export HTML`, session APIs, and TX (`frontend/profile.js`)
- exported replay is expected to hide `#btn-clear`, `#btn-export`, and `#ws-status` (`tests-ui/tests/export-replay.spec.js`)
- live mode keeps session workflows, TX, and WS state (`frontend/profile.js`, `tests-ui/tests/session-workflows.spec.js`)

Therefore:
1. the **website iframe** should be a curated exported session
2. **runtime-only features** must be demonstrated separately and intentionally

## Deliverables
1. A curated session artifact with:
   - meaningful multi-pane logs
   - pre-saved markers that survive export
   - a clean exported `session.html`
2. A short implementation note containing:
   - how the session was generated
   - the session id / output path
   - any helper scripts or input files used
3. Optional but preferred: a small set of reusable demo inputs/scripts so the session can be regenerated deterministically

## Non-goals
- Do not redesign the website here
- Do not change core frontend behavior unless strictly required to enable/demo existing features
- Do not broaden this into a large product polish effort

## Use the existing layout unless there is a strong reason not to
Keep the current demo structure unless you can justify a better one:
- `DevA` tab → `READER` + `CONTROLLER`
- `DevB` tab → `READER`
- `cbor-tab` → `CBOR`

This layout already matches tests and existing expectations in the repo.

## The curated demo must tell one clear story
The session should feel like a guided investigation with a beginning, middle, and end.

Recommended storyline:
1. normal startup / handshake
2. steady-state communication across multiple panes
3. one visible incident that unfolds across panes at nearly the same timestamps
4. one structured/decoded view in the CBOR tab that shows the same system from another angle
5. enough aftermath to show diagnosis, not just the crash point

Recommended incident shape:
- `READER` shows upstream symptoms first
- `CONTROLLER` shows the control path or command reaction
- `DevB` shows a supporting or downstream signal at the same moment
- `CBOR` shows structured fields (`kind=...`, `src=...`, key=value data) that look useful rather than noisy

Good examples:
- telemetry lag → retry → publish/auth failure
- sensor drift → threshold warning → controller fallback
- command timeout → retry storm → recovery marker

## What the exported demo must make easy to show
The website visitor should be able to discover these quickly, without instructions longer than a few bullets.

### 1) Cross-pane synchronization
The export must contain at least one obvious incident window where clicking a line in one pane makes the neighboring pane jump to the same moment.

Requirements:
- shared timestamps across `READER` and `CONTROLLER`
- at least one pair of near-simultaneous lines that are semantically related
- do not bury the best sync moment too deep in the log

### 2) Range selection
The export must contain at least one short contiguous block that is worth selecting.

Requirements:
- include a compact 4–10 line incident window in `DevA`
- the selected range should produce meaning both in `Exact` mode and in context mode
- surrounding panes must contain useful nearby lines in the same time window so `All` / `Sel…` feels valuable

### 3) Markers
Markers should already exist before export.

Requirements:
- save several markers in the live session before generating `session.html`
- markers should correspond to real investigation checkpoints, not placeholders
- titles must read like notes an engineer would actually leave

Recommended marker set:
- `Boot complete`
- `First warning on reader`
- `Controller fallback engaged`
- `Publish/auth failure`
- `Recovered after retry window` or `Failure persists`

### 4) Tabs with purpose
Tabs should not feel redundant.

Requirements:
- `DevA` is the main investigation surface
- `DevB` should show a single-pane companion view that supports the same timeline
- `cbor-tab` should prove structured decoding, not just duplicate plain text

### 5) Portable replay feel
The export should feel polished when opened standalone.

Requirements:
- works cleanly as static HTML
- includes enough data to demonstrate timestamp toggling if origin metadata is available
- preserves markers and meaningful selection opportunities
- no clutter from irrelevant noise or extremely long filler runs

## How the log content should look
Aim for clarity over volume.

Preferred characteristics:
- realistic embedded phrasing
- mixed severity (`dbg`, `wrn`, `err`, normal info)
- repeated nominal lines for context, but not enough to swamp the key incident
- consistent terminology across panes so the relationship is obvious

Avoid:
- joke/demo text
- random unrelated failures
- giant walls of repetitive noise
- too many equally important incidents

Recommended scale:
- compact enough to be usable inside a website iframe
- long enough that sync, selection, and marker navigation feel real
- one primary incident beats many small ones

## Concrete content brief for the curated session
Use this as the target shape.

### Tab: DevA
#### Pane: READER
Should include:
- startup / ready lines
- nominal reads / packets / telemetry
- first warning
- one clear error or degraded condition
- a short recovery or post-failure explanation

#### Pane: CONTROLLER
Should include:
- controller startup / config apply
- command or state transitions that line up with `READER`
- fallback/retry behavior near the incident
- a clear response after the main failure moment

### Tab: DevB
#### Pane: READER
Should include:
- one corroborating stream for the same timeline
- fewer lines than `DevA`
- at least one line that clearly aligns with the `DevA` incident window

### Tab: cbor-tab
#### Pane: CBOR
Should include decoded key=value style lines that expose structure such as:
- `src=SENSOR_CBOR`
- `kind=sync` / `kind=warning` / similar fields
- counters, mode/state fields, or measurements

This tab should look like a structured diagnostic channel, not like raw binary or plain-text duplication.

## Runtime-only showcase plan
The website export cannot fully show live-runtime capabilities. Prepare a second, separate runtime narrative.

### Runtime features worth showing
These are strong live-only differentiators:
- WS connected/disconnected state
- `Save HTML` flow
- `Open HTML` / exported report handoff
- `New session` rotation
- `Sessions` popup / saved session browsing
- TX input / send command path (if the demo path can show it meaningfully)

### Best way to present runtime on the website
Do **not** try to cram all runtime behavior into the exported iframe.

Recommended presentation:
1. **Primary surface:** embedded exported `session.html`
2. **Secondary runtime proof:** one short screen recording or animated capture showing:
   - live viewer connected
   - selecting a moment across panes
   - saving markers
   - clicking `Save HTML`
   - opening exported HTML
   - optionally rotating to a new session

If only one runtime clip is made, show this sequence:
1. live session receiving logs
2. click line to sync panes
3. make or reveal markers
4. `Save HTML`
5. `Open HTML`
6. show the same investigation state in static replay

That sequence explains the product better than a generic “tailing logs” recording.

## Implementation guidance for the agent
Preferred order:
1. read `AGENTS.md`, `docs/FRONTEND.md`, and the relevant UI tests
2. inspect the existing demo config and deterministic demo behavior
3. decide whether to:
   - reuse deterministic demo traffic and curate it, or
   - generate a dedicated marketing-focused session with custom inputs
4. produce the session in live mode if possible so markers/session flows are real
5. save markers during the live session
6. export the session to HTML
7. verify the exported HTML still demonstrates sync, selection, markers, tabs, and timestamp toggle behavior where available

## Acceptance criteria
The work is complete when all of the following are true:
- one exported `session.html` is clearly better suited for the marketing site than the current generic sample
- `DevA` contains an obvious sync-worthy incident window
- selection in `DevA` is worth demonstrating in both exact and context scope
- saved markers are present and meaningful in the exported artifact
- `cbor-tab` adds visible value through structured decoding
- the agent documents how to regenerate the artifact
- runtime-only capabilities are called out separately instead of being incorrectly assumed to exist in static export

## Suggested commands and tools
Use the current CLI/documented paths in the repo. Depending on the chosen approach, likely commands are in this family:

```bash
embed-log demo --profile deterministic --no-browser
embed-log sessions list
embed-log sessions export <session-id>
embed-log sessions marker list <session-id>
embed-log sessions open <session-id> marker 1
```

If a fully curated static artifact is easier to author from prepared logs, `embed-log merge` is a valid fallback, but live-session generation is preferred because it exercises markers and session workflows more honestly.

## Final note for the agent
Optimize for a demo that helps a first-time visitor understand `embed-log` in under a minute:
- one real investigation story
- obvious cross-pane correlation
- obvious selection value
- obvious marker value
- clear handoff from live runtime to portable HTML replay
