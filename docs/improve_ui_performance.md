# UI performance: JSON export + lazy hydration

## Problem

After ~1 minute of `--fast` demo traffic (9 sources, 50ms ticks, ~180 lines/second), the live UI becomes laggy. Exported session HTML files (17MB, ~100k lines) fail to render within a minute.

**Root cause**: every log line is a `<div>` in the DOM. With 100k+ lines, the browser's layout/recalc cost grows with every append. The exported HTML embeds all lines as pre-rendered DOM elements — the parser must construct the entire tree before first paint.

---

## Solution architecture

Embed log data as compact JSON in the exported HTML. On load, hydrate `state.rawLines[]` from JSON (fast). Render only the visible window of lines into the DOM (~500 elements). As the user scrolls, destroy elements outside the window and create elements inside it.

---

## Files changed (4)

```
utils/merge_logs.py          ← embed line arrays as JSON, not DOM divs
frontend/lines.js            ← add renderPaneWindow() + scroll handler
frontend/export.js           ← hydrate JSON on load, kick off windowed render
tests-ui/tests/              ← add perf test for large export
```

---

## What stays identical (zero regression surface)

- `state.rawLines[]` — always fully populated (selection, filtering, markers, sync all index into it)
- `state.filters`, `state.selected`, `state.markers`, `state.atBottom` — no change
- `buildStoredLine`, `buildLineHtml`, `applyLineDom`, `matchesFilter` — no change
- `syncPanes`, `scrollPaneToTs`, `highlightLine`, `onLineClick`, `onMiddleClick` — no change (they already look up DOM elements by `data-idx`, which windowed rendering preserves)
- Plugin analysis (`analyzeLinePlugins`) — runs on lines when they enter the visible window
- Download raw `.log` — unchanged (reads from `.log` files on disk, not from the DOM)
- Live mode (`ws.js`, `appendLineBatch`) — completely untouched
- Unwrap, font size, timestamp toggle, theme — DOM manipulation patterns unchanged

---

## Step 1: `merge_logs.py` — embed data as JSON, not HTML

**Current**: bootstrap script does `appendLine()` for every entry → 100k DOM elements created at load.

**New**: embed each pane's lines as a compact JSON array in a `<script>` tag. The bootstrap script stores them in `state.rawLines[]` via `buildStoredLine` (no DOM creation). Then calls `renderPaneWindow()` to render just the visible portion.

The JSON format per line is the same object `parse_log_file` already produces: `{ts, text, isTx, absTs, absNum, relTs, relNum, ...}`. No new parsing. Just a different embedding target.

```html
<!-- Instead of _logData in bootstrap, one script per pane: -->
<script type="application/json" id="lines-SENSOR_A">
[["T+00:00:00.000","TEST src=SENSOR_A tick=001...",false,{"timestampIso":"...","numTs":...}],
 ...]
</script>
```

Use compact tuples `[ts, text, isTx, meta]` to minimize file size. `buildStoredLine` already accepts these exact arguments.

---

## Step 2: `frontend/lines.js` — windowed rendering

Add three functions:

### `renderPaneWindow(paneId, { targetIdx })`

Calculates which line indices should be in the DOM based on the target index. Ensures those indices have DOM elements, removes elements outside the window, preserves scrollTop.

```
Window = [targetIdx - 250, targetIdx + 250]  clamped to [0, rawLines.length)
```

Initial load: `targetIdx = rawLines.length - 1` (show latest lines).

### `_ensureRange(paneId, start, end)`

For each index in [start, end] that has no DOM element, create one via `buildStoredLine` + `applyLineDom` and insert at the correct position. Uses a `Map<index, Element>` to track which indices are currently rendered.

### `_pruneOutside(paneId, start, end)`

Remove DOM elements whose `data-idx` is outside the window. The `Map` tracks rendered indices.

---

## Step 3: `frontend/export.js` — hydrate on load

The bootstrap (currently inline in `merge_logs.py`, moved to `export.js` for clarity) does:

```js
// Read compact JSON arrays from <script> tags
document.querySelectorAll('script[data-pane]').forEach(script => {
  const paneId = script.dataset.pane;
  const tuples = JSON.parse(script.textContent);
  state.rawLines[paneId] = tuples.map(([ts, text, isTx, meta]) =>
    buildStoredLine(paneId, ts, text, isTx, meta)
  );
});
// Render visible window for each pane
PANES.forEach(id => renderPaneWindow(id, { targetIdx: state.rawLines[id].length - 1 }));
```

---

## Step 4: Scroll handler

Add a scroll listener on each `.log-area` that calls `renderPaneWindow` when the user scrolls near the window boundary (within 100px of the top or bottom of the rendered range). Debounced via `requestAnimationFrame`.

When the user clicks a line or sync scrolls to a timestamp, `scrollPaneToTs` already finds the target by `data-idx`. If the target index is not in the DOM window, `renderPaneWindow` is called with that `targetIdx` before scrolling.

---

## Step 5: Filter interaction with windowed rendering

When a filter is applied (`state.filters[paneId]` becomes a RegExp):

1. `rerenderPane` already re-renders all visible lines
2. With windowing, `renderPaneWindow` is called which only re-renders the visible window
3. Filtered-out lines still exist in `state.rawLines[]` but their DOM elements have `display: none` (existing behavior via `applyLineDom`)
4. When scrolling, newly-rendered lines also get filtered via `applyLineDom`

This is already correct — `applyLineDom` sets `display: none/block` based on the filter. Windowed rendering just calls `applyLineDom` on the newly created elements.

---

## Step 6: Selection across window boundaries

`state.selected[paneId]` is a `Set<index>`. When the user shift-clicks to select a range, the selection spans indices that may not all be in the DOM. The CSS class `.log-line.selected` is applied during `applyLineDom` when the line is rendered. Lines outside the window do not have DOM elements, so they do not show the visual selection — but the `Set` has the data. When the user scrolls and those indices enter the window, they render with the `.selected` class.

Copy/export operations that read selected lines use `state.selected[paneId]` and `state.rawLines[paneId][idx].rawText` — they do not walk the DOM. So they work regardless of what is rendered.

---

## Step 7: Sync scrolling between panes

`syncPanes(fromId, numTs, clickedDiv)` scrolls sibling panes to the line closest to `numTs`. It does a binary search on DOM elements' `data-idx` → `rawLines[idx].numTs`. With windowing, the target line might not be in the DOM. Fix: before the binary search, call `renderPaneWindow(paneId, { targetIdx })` with the estimated target index to ensure it is rendered.

`scrollPaneToTs` already does a linear scan of DOM elements. Same fix — ensure the target is in the window first.

---

## Edge cases

| Scenario | How it works |
|----------|-------------|
| User scrolls to top | `renderPaneWindow` with `targetIdx = 0` |
| User jumps to bottom | `renderPaneWindow` with `targetIdx = rawLines.length - 1` |
| User clicks a specific line | Line's `data-idx` is found, `renderPaneWindow` ensures it is rendered, then scrolls to it |
| Filter is applied | `rerenderPane` triggers `renderPaneWindow`, only visible window re-renders |
| Unwrap toggle | `rebuildLayout` calls `repopulatePaneLogs` → calls `renderPaneWindow` on new panes |
| Font size change | All rendered lines need re-measurement, then `renderPaneWindow` recalculates |
| Plugin inline text changes line height | Window bounds are pixel-based (scrollTop), not line-count-based — self-correcting |
| 100k+ line export | JSON parsing: <1s. Initial render: ~500 DOM elements. Scroll: smooth |

---

## Migration path — no breaking change

A `--lazy` flag on `merge_logs.py` toggles between old (embedded DOM) and new (JSON + hydration) formats. Default is new. The `--legacy-embed` flag produces the old format for comparison testing.

Existing exported HTML files continue to work — they embed the old format. New exports use the new format. The frontend code handles both: if `<script data-pane>` tags exist, hydrate from JSON; otherwise, lines are already in the DOM (legacy path).

---

## Test plan

1. **Unit**: `test_merge_logs.py` — verify new JSON embedding format matches existing data
2. **E2E**: Export a session with 30k lines, open the HTML, verify first paint < 2 seconds, verify scrolling is smooth, verify sync/filter/selection/export/download all work
3. **Regression**: Existing Playwright tests must pass unchanged
4. **Perf guard**: `expect(firstLogLine).toBeVisible({ timeout: 2000 })` on a 30k-line export
