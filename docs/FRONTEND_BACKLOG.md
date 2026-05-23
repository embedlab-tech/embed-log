# FRONTEND_BACKLOG

Prioritized frontend/UI backlog distilled from current UI notes and E2E backlog.

## High priority

1. Simplify selection/share UX around explicit scope
   - revisit overlapping selection actions:
     - `Clipboard add`
     - `Copy`
     - `Copy range`
     - `Raw file`
     - `HTML snippet`
   - redesign around user intent instead of transport/mechanism
   - make export scope explicit with two modes:
     - `Exact` / selected only
     - `With context` / selected + synchronized context
   - default to exact selection so the UI never sends more lines than the user explicitly selected unless they opt in
   - reduce primary actions to likely common workflows:
     - `Copy`
     - `Download raw`
   - demote advanced/specialized actions to secondary UI:
     - clipboard buffer / `Clipboard add`
     - HTML snippet export
   - remove or rename ambiguous actions such as bare `Copy` vs `Copy range`
   - define clear behavior for:
     - copying exact selected lines,
     - copying selected + synchronized context,
     - downloading exact raw text,
     - downloading raw text with context,
     - exporting exact HTML,
     - exporting HTML with context
   - validate against real user goals:
     - share exact selected logs with another human,
     - send exact selected logs to an agent app,
     - explicitly opt into cross-pane debugging context when desired

2. Cross-tab sync persistence
   - click in one pane/tab,
   - switch tabs,
   - verify sync lands near same tick in the other tab.

3. Invalid regex resilience
   - invalid filter input must not break rendering or interaction.

4. Current HTML freshness
   - repeated save/export should reflect newer log content.

5. Stronger session-rotation stale-line guard
   - verify multiple old lines are gone after rotation,
   - verify fresh lines arrive on all panes.

## Medium priority

6. Sessions list ordering and metadata coverage
   - current marker,
   - newest-first ordering,
   - valid manifest/html links.

7. Export during active traffic
   - ensure consistency while stream is still live.

8. Clipboard normalization and overlapping-range behavior
   - deterministic copy output,
   - no broken boundary handling.

9. Clear-cache UX verification
   - layout/filter/session cache should reset cleanly.

10. UNWRAP virtual single-pane presentation mode
   - frontend-only alternate presentation for small displays,
   - virtually expand grouped config tabs into one pane per page/tab,
   - preserve backend config as the canonical layout source,
   - do not mutate persisted backend config to achieve the mode.

## Lower priority / future work

10. Pane swap persistence across reload
11. Malformed import/snapshot handling
12. Additional frontend helper/unit tests if a dedicated harness is introduced

## Already covered by E2E suite

- live connection and deterministic log arrival
- shift-click selection
- drag selection basics
- raw/html snippet flows
- full export replay
- filter by deterministic marker
- escape clears selection
- clipboard workflow basics
- backend session HTML / current HTML path
- clean session rotation basics
- sessions popup basics
- shared page error guard
