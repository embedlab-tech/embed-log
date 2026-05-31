# FRONTEND_BACKLOG

Prioritized frontend/UI backlog distilled from current UI notes and E2E backlog.

## High priority

1. ~~Simplify selection/share UX around explicit scope~~ **Done**
   - scope toggle (Exact / Context) with shared state across panes
   - Copy and Download raw as primary actions
   - Export HTML and Add to clipboard demoted to secondary menu (More ···)
   - keyboard shortcut Ctrl/Cmd+C always copies exact (predictable)
   - source labels (`[SENSOR_A]`) shown consistently in all output formats

2. ~~Cross-tab sync persistence~~ **Works — click syncs, tab switch follows.**
   - click in one pane/tab,
   - switch tabs,
   - verify sync lands near same tick in the other tab.

3. ~~Invalid regex resilience~~ **Done**
   - invalid filter input does not break the UI, shows input error state
   - previous valid filter is preserved while user fixes the regex
   - clearing invalid input restores normal state

4. ~~Current HTML freshness~~ **Done**
   - repeated Export captures more log lines on each invocation
   - E2E test verifies second export has more `[SENSOR_A]...TEST` lines than the first

5. ~~UNWRAP virtual single-pane presentation mode~~ **Done**
   - toolbar toggle unwraps grouped tabs into one tab per pane
   - preserves log content across toggle
   - no "+" button in unwrap mode
   - config (TABS/PANES) is never mutated

6. Stronger session-rotation stale-line guard
   - verify multiple old lines are gone after rotation,
   - verify fresh lines arrive on all panes.

## Medium priority

7. Sessions list ordering and metadata coverage
   - current marker,
   - newest-first ordering,
   - valid manifest/html links.

8. Export during active traffic
   - ensure consistency while stream is still live.

9. Clipboard normalization and overlapping-range behavior
   - deterministic copy output,
   - no broken boundary handling.

10. Clear-cache UX verification
    - layout/filter/session cache should reset cleanly.

## Lower priority / future work

11. Pane swap persistence across reload
12. Malformed import/snapshot handling
13. Additional frontend helper/unit tests if a dedicated harness is introduced

## Already covered by E2E suite

- live connection and deterministic log arrival
- shift-click selection
- drag selection basics
- raw/html snippet flows
- full export replay
- filter by deterministic marker
- escape clears selection
- clipboard workflow basics
- scope-aware selection (Exact/Context copy, download, HTML export, clipboard add)
- per-pane wrap toggle
- UNWRAP toggle (per-pane tabs, log preservation, reversible)
- backend session HTML / current HTML path
- clean session rotation basics
- sessions popup basics
- shared page error guard
- CBOR decoder rendering
- layout sync / time synchronization across panes
- timestamp toggle (absolute / relative)
- filter keyboard interaction
- relative time in replay
- clipboard content matching
- export replay consistency
