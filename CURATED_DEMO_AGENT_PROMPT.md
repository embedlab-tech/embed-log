# Ready-to-paste agent prompt for curated website demo

Recommended agent type: `task`

Use this as the `context` and `assignment` for a single delegated task.

## Context

```text
# Goal
Produce one polished embed-log demo session and exported session.html for the website, plus a short regeneration note.

# Constraints
- Read AGENTS.md, docs/FRONTEND.md, embed-log.demo.yml, and the relevant UI tests before editing or generating artifacts.
- Treat static/exported HTML and live runtime as different demo surfaces.
- The website artifact must be a curated exported session.html, not a live-only demo.
- Runtime-only features must be documented separately, not assumed to exist in static export.
- Prefer reusing the current demo layout unless there is a strong reason to change it:
  - DevA -> READER + CONTROLLER
  - DevB -> READER
  - cbor-tab -> CBOR
- Keep the demo grounded in existing embed-log behavior and existing frontend capabilities.
- Do not redesign the website in this task.
- Do not run project-wide build/test/lint/formatting commands.
- Do not run broad repo verification. If you add helper files or scripts, verify only the directly affected behavior needed for this demo artifact.
- Skip formatters.

# Contract
The exported demo must make these existing frontend capabilities easy to show:
- synchronized panes via line click
- range selection with useful Exact and context scope behavior
- saved markers visible in exported replay
- tabbed investigation flow with DevA, DevB, and cbor-tab each serving a purpose
- structured CBOR-decoded content that adds visible value
- timestamp toggle if the session carries the required origin metadata

The runtime-only capabilities that must be called out separately are:
- WS connected/disconnected status
- Save HTML / Open HTML flow
- New session rotation
- Sessions popup / saved session browsing
- TX input / send command path, if it can be shown cleanly
```

## Assignment

```text
# Target
Work inside the embed-log repository only.
Primary outputs:
- one curated exported session.html suitable for embedding on the marketing site
- any minimal helper inputs/scripts needed to regenerate it deterministically
- one short implementation note with exact regeneration steps and output paths

Non-goals:
- no website edits
- no broad product refactor
- no speculative feature work

# Change
1. Inspect the existing demo/runtime/frontend behavior first:
   - AGENTS.md
   - docs/FRONTEND.md
   - embed-log.demo.yml
   - tests-ui/tests/layout-sync.spec.js
   - tests-ui/tests/scope-selection.spec.js
   - tests-ui/tests/session-workflows.spec.js
   - tests-ui/tests/cbor-decoder.spec.js
   - tests-ui/tests/export-replay.spec.js
   - frontend/profile.js
2. Decide the simplest honest path to a polished artifact:
   - preferred: generate a live session and export it
   - acceptable fallback: produce a curated artifact from prepared logs with merge, but only if live-session generation is materially worse for markers/session realism
3. Keep or intentionally justify the tab layout. Default to:
   - DevA: READER + CONTROLLER
   - DevB: READER
   - cbor-tab: CBOR
4. Author a single clear investigation story in the logs:
   - startup / ready state
   - nominal behavior
   - one visible incident unfolding across panes at aligned times
   - a short aftermath / diagnosis window
   - structured CBOR lines that support the same story rather than duplicate raw text
5. Make DevA the main investigation surface.
   Required content shape:
   - READER shows first symptoms
   - CONTROLLER shows response/fallback/retry logic near the same timestamps
   - DevB contributes one corroborating stream for the same window
   - cbor-tab shows readable key=value structured diagnostics with meaningful fields like kind=..., src=..., counters/states/measurements
6. Ensure the exported session is good for the website iframe:
   - not too long or noisy
   - clear sync-worthy moment near the interesting section
   - at least one 4-10 line selection window in DevA that is meaningful in Exact mode and in context mode
   - enough nearby lines in sibling panes for All / Sel… to look valuable
7. Save meaningful markers before export.
   Use real investigation-note phrasing. Recommended set:
   - Boot complete
   - First warning on reader
   - Controller fallback engaged
   - Publish/auth failure
   - Recovered after retry window or Failure persists
   Adjust names only if the final scenario needs different wording.
8. Export the curated session to session.html.
9. Verify the exported artifact directly for the demo goals:
   - tabs render correctly
   - sync is demonstrable
   - selection opportunities are present
   - markers survive export and are visible/navigable
   - CBOR tab adds visible value
   - timestamp toggle works if metadata exists; if not, document that clearly
10. Write a short implementation note in the repo with:
   - what was generated
   - exact commands used
   - session id / artifact path
   - any helper files/scripts added
   - whether the artifact came from live session export or merge fallback
11. In that implementation note, add a short runtime showcase recommendation separate from the static export. It must recommend a short live clip showing:
   - live logs arriving
   - pane synchronization
   - markers
   - Save HTML
   - Open HTML
   - optionally New session / Sessions popup / TX if demonstrated cleanly

# Acceptance
The task is complete only when all of the following are true:
- there is one clearly identified exported session.html intended for website embedding
- the export is more intentional and polished than the current generic sample
- DevA contains an obvious sync-worthy incident window
- DevA contains a useful short range for demonstrating Exact and context selection
- several meaningful markers exist in the exported artifact
- cbor-tab contributes readable structured diagnostic value
- regeneration steps are documented precisely enough for another engineer to reproduce the artifact
- runtime-only capabilities are documented separately from static export instead of being conflated with it
- no project-wide build/test/lint/format passes were run
```

## Recommended task payload

```json
{
  "agent": "task",
  "context": "# Goal\nProduce one polished embed-log demo session and exported session.html for the website, plus a short regeneration note.\n\n# Constraints\n- Read AGENTS.md, docs/FRONTEND.md, embed-log.demo.yml, and the relevant UI tests before editing or generating artifacts.\n- Treat static/exported HTML and live runtime as different demo surfaces.\n- The website artifact must be a curated exported session.html, not a live-only demo.\n- Runtime-only features must be documented separately, not assumed to exist in static export.\n- Prefer reusing the current demo layout unless there is a strong reason to change it:\n  - DevA -> READER + CONTROLLER\n  - DevB -> READER\n  - cbor-tab -> CBOR\n- Keep the demo grounded in existing embed-log behavior and existing frontend capabilities.\n- Do not redesign the website in this task.\n- Do not run project-wide build/test/lint/formatting commands.\n- Do not run broad repo verification. If you add helper files or scripts, verify only the directly affected behavior needed for this demo artifact.\n- Skip formatters.\n\n# Contract\nThe exported demo must make these existing frontend capabilities easy to show:\n- synchronized panes via line click\n- range selection with useful Exact and context scope behavior\n- saved markers visible in exported replay\n- tabbed investigation flow with DevA, DevB, and cbor-tab each serving a purpose\n- structured CBOR-decoded content that adds visible value\n- timestamp toggle if the session carries the required origin metadata\n\nThe runtime-only capabilities that must be called out separately are:\n- WS connected/disconnected status\n- Save HTML / Open HTML flow\n- New session rotation\n- Sessions popup / saved session browsing\n- TX input / send command path, if it can be shown cleanly",
  "tasks": [
    {
      "id": "CuratedDemo",
      "description": "Build curated website demo export",
      "assignment": "# Target\nWork inside the embed-log repository only.\nPrimary outputs:\n- one curated exported session.html suitable for embedding on the marketing site\n- any minimal helper inputs/scripts needed to regenerate it deterministically\n- one short implementation note with exact regeneration steps and output paths\n\nNon-goals:\n- no website edits\n- no broad product refactor\n- no speculative feature work\n\n# Change\n1. Inspect the existing demo/runtime/frontend behavior first:\n   - AGENTS.md\n   - docs/FRONTEND.md\n   - embed-log.demo.yml\n   - tests-ui/tests/layout-sync.spec.js\n   - tests-ui/tests/scope-selection.spec.js\n   - tests-ui/tests/session-workflows.spec.js\n   - tests-ui/tests/cbor-decoder.spec.js\n   - tests-ui/tests/export-replay.spec.js\n   - frontend/profile.js\n2. Decide the simplest honest path to a polished artifact:\n   - preferred: generate a live session and export it\n   - acceptable fallback: produce a curated artifact from prepared logs with merge, but only if live-session generation is materially worse for markers/session realism\n3. Keep or intentionally justify the tab layout. Default to:\n   - DevA: READER + CONTROLLER\n   - DevB: READER\n   - cbor-tab: CBOR\n4. Author a single clear investigation story in the logs:\n   - startup / ready state\n   - nominal behavior\n   - one visible incident unfolding across panes at aligned times\n   - a short aftermath / diagnosis window\n   - structured CBOR lines that support the same story rather than duplicate raw text\n5. Make DevA the main investigation surface.\n6. Ensure the exported session is good for the website iframe:\n   - not too long or noisy\n   - clear sync-worthy moment near the interesting section\n   - at least one 4-10 line selection window in DevA that is meaningful in Exact mode and in context mode\n   - enough nearby lines in sibling panes for All / Sel… to look valuable\n7. Save meaningful markers before export using real investigation-note phrasing.\n8. Export the curated session to session.html.\n9. Verify the exported artifact directly for the demo goals.\n10. Write a short implementation note in the repo with commands, paths, and runtime showcase recommendation.\n\n# Acceptance\nThe task is complete only when all of the following are true:\n- there is one clearly identified exported session.html intended for website embedding\n- the export is more intentional and polished than the current generic sample\n- DevA contains an obvious sync-worthy incident window\n- DevA contains a useful short range for demonstrating Exact and context selection\n- several meaningful markers exist in the exported artifact\n- cbor-tab contributes readable structured diagnostic value\n- regeneration steps are documented precisely enough for another engineer to reproduce the artifact\n- runtime-only capabilities are documented separately from static export instead of being conflated with it\n- no project-wide build/test/lint/format passes were run"
    }
  ]
}
```
