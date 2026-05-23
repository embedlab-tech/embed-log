# UI/E2E tests backlog

Current deterministic Playwright suite covers the main smoke/regression path and currently passes locally (`17 passed`).

This file is intentionally kept as a working UI test notebook for future test additions.

## Implemented

- Live UI connects and receives deterministic logs.
- Live tab/pane layout is verified.
- Time synchronization highlights sibling pane lines.
- Shift+Click selects a range.
- Drag-select range basics are covered.
- Escape clears selection.
- Filter by deterministic `kind=filter-alpha` works.
- Shared page-error guard is used in Playwright tests.
- Raw snippet download cleans duplicated prefixes/timestamps.
- `Copy range` clipboard content matches downloaded raw content.
- Platform shortcut copy works.
- Clipboard buffer add / peek / copy-all / clear flow is covered.
- HTML snippet uses the regular exported embed-log UI.
- Downloaded HTML snippet reopens as static replay.
- Full toolbar `Export` downloads and reopens as static replay.
- Backend `Current HTML` flow is covered.
- `Clean session` rotation basics are covered.
- Sessions popup basics are covered.
- Scope-aware selection (Exact/Context copy, download, HTML export, clipboard add) covered.
- Per-pane wrap toggle covered.
- UNWRAP single-pane mode (per-pane tabs, log preservation, toggle reversible) covered.
- Cross-tab sync works (click syncs timestamp, tab switch follows).
- Invalid regex resilience (enter `(` — UI stays responsive, shows error state, preserves previous filter).
- Current HTML freshness (repeated Export captures more data on each invocation).

## Remaining backlog

### 1. ~~Cross-tab synchronization~~ **Works — click syncs, tab switch follows.**

- Click a line in `SENSOR_A`.
- Switch to `Other Sensor`.
- Assert `SENSOR_C` jumps/highlights near the same tick.

### 2. ~~Regex filter resilience~~ **Done**

### 3. Sessions metadata depth

- Rotate session multiple times.
- Assert newest session appears first.
- Assert `current` tag moves correctly.
- Assert manifest/open-html links are valid for each row.

### 4. ~~Current HTML freshness~~ **Done**

### 5. Export during active traffic

- Export while deterministic stream is active.
- Assert exported HTML opens and contains consistent logs.

### 6. Clipboard normalization edge cases

- Add overlapping ranges from the same pane.
- Assert `Copy all` output remains deterministic.
- Verify newline normalization and no broken range boundaries.

### 7. Pane swap persistence

- Swap panes via hover swap UI.
- Reload page.
- Assert swapped pane order is restored from cache.

### 8. Clear cache UX

- Create non-default layout/filter state.
- Click `Clear cache`.
- Reload and assert default layout/state is restored.

### 9. Import malformed snapshot handling

- Import invalid or partial HTML snapshot.
- Assert user-visible error.
- Assert no corrupted UI state remains.

### 10. Stronger stale-line guard on session rotation

- Capture multiple unique old lines.
- Rotate session.
- Assert all captured old lines are absent.
- Assert new lines arrive on all panes.

### 11. Optional helper/unit tests

- Timestamp parser helper tests.
- Snippet cleanup helper tests.
- Range merge/sort helper tests.

### 12. ~~UNWRAP virtual single-tab mode~~ **Done**

- Add a virtual UI mode that bypasses the current grouped tab view.
- Render one page/tab per configured pane while preserving backend config as the source of truth.
- Keep the underlying config unchanged; this is a frontend-only alternate presentation.
- Useful for small displays and narrow laptop screens.
