# Fixing plan: scalable log rendering for 5MB+ exports/live logs

## Goal

Replace the current DOM-culling “windowed rendering” with real log virtualization that gives stable scrolling, faster exported HTML load, and maintainable behavior across filtering, markers, selection, sync, plugins, live logs, and imports.

## Current problems to fix

1. **Scroll geometry is wrong**
   - Current code removes off-window rows without preserving total virtual height.
   - Result: exported HTML feels like infinite/rubber-band scrolling.

2. **Hydration still does too much work**
   - Export load eagerly converts every tuple into a full stored line.
   - This still runs ANSI parsing and plugin analysis for every line.

3. **Live/import paths still build full DOM**
   - `appendLineBatch()` and `import.js` still append a DOM node per line.

4. **DOM index assumptions break**
   - Marker and selection code often treats `logEl.children[i]` as raw line index `i`.
   - Under virtualization, child index no longer equals line index.

5. **Filtering is incomplete**
   - Current filtering hides rendered rows only.
   - Matches outside the rendered window are invisible/unreachable.

6. **Plugin catch-up is incomplete**
   - Lines rendered before plugins load are rerendered, but not necessarily re-analyzed.

7. **Export safety regression**
   - `merge_logs.py` lazy JSON embedding must escape `</script>` sequences.

## Proposed architecture

### 1. Introduce a `VirtualLogPane` model

Each pane gets a small rendering controller:

```js
{
  paneId,
  rawEntries,       // compact tuples or raw stored lines
  lineCache,        // index -> built StoredLine
  visibleIndices,   // null means all lines; array when filter active
  rowHeight,
  viewportEl,
  spacerEl,
  contentEl,
  firstRendered,
  lastRendered,
}
```

DOM shape:

```html
<div class="log-area" id="log-SENSOR_A">
  <div class="log-spacer">
    <div class="log-window"></div>
  </div>
</div>
```

Behavior:

- `.log-spacer` height represents the full virtual height.
- `.log-window` is translated vertically to the rendered range offset.
- Only visible rows plus overscan are in DOM.
- Scrollbar position maps to actual log position.

For nowrap mode:

```js
virtualHeight = visibleLineCount * rowHeight
startIndex = Math.floor(scrollTop / rowHeight)
```

For wrap mode / variable height:

- Start with a conservative fallback:
  - keep virtualization disabled below a threshold;
  - above threshold, use chunked/paginated rendering until measured-height virtualization is added.
- Later enhancement: measured-height cache + prefix sums.

## Implementation phases

## Phase 1: Make exported lazy rendering correct

### Changes

- Replace `_renderedIndices`, `_windowTarget`, and `renderPaneWindow()` with a true virtual renderer.
- Add spacer/content layer to `renderPaneShell()`.
- Initial exported HTML should:
  - parse compact JSON tuples;
  - store them as raw tuples;
  - render only visible rows;
  - build full `StoredLine` objects lazily for rendered indices.

### Acceptance criteria

- Exported 100k-line HTML opens quickly.
- Scrollbar has stable finite height.
- Scrolling to top/middle/bottom shows expected real `data-idx`.
- Jump to bottom lands on last line.
- No duplicate `.log-line[data-idx]` elements.

## Phase 2: Remove DOM-index assumptions

### Changes

Update all code that assumes child index equals line index:

- `selection.js`
  - `_applySelection()`
  - marker application
  - marker navigation
  - hash marker jump
- Any use of:
  - `Array.from(logEl.children).forEach((div, i) => …)`
  - `logEl.children[idx]`

Use `Number(div.dataset.idx)` instead.

Expose helpers from the virtual renderer:

```js
ensureLineVisible(paneId, rawIndex, { align = "center" })
getRenderedLineElement(paneId, rawIndex)
rerenderRenderedLines(paneId)
```

### Acceptance criteria

- Selection survives scrolling.
- Shift-select works after scrolling.
- Marker rendering uses correct raw line indices.
- Marker navigation can jump to a marker outside current viewport.

## Phase 3: Fix filtering for virtualized logs

### Changes

Filtering should build an index projection:

```js
visibleIndices = rawEntries
  .map((_, rawIndex) => rawIndex)
  .filter(rawIndex => matchesFilter(getLine(rawIndex), rx))
```

Then virtual scrolling operates over `visibleIndices`, not raw line count.

Rendered row `n` maps to:

```js
rawIndex = visibleIndices ? visibleIndices[n] : n
```

### Acceptance criteria

- Filtering finds rare matches outside the initial viewport.
- Clearing filter restores full scroll range and original indices.
- Clicking a filtered line clears filter and scrolls to the real raw line context.
- Empty filter result shows stable empty pane, not broken scroll.

## Phase 4: Make live logs use the same renderer

### Changes

Refactor `appendLineBatch()`:

- Always append to pane data model.
- If pane is virtualized:
  - do not append DOM nodes directly;
  - update virtual height;
  - rerender only if at bottom or new lines intersect viewport.
- If below threshold, full DOM mode is allowed initially, but switching to virtual mode must clear/adopt existing DOM safely.

Recommended threshold:

```js
const VIRTUALIZE_AFTER_LINES = 2000;
```

### Acceptance criteria

- Long live sessions do not keep growing DOM unbounded.
- Tail-at-bottom remains smooth.
- Jump-to-bottom works.
- Switching tabs/syncing does not duplicate rows.

## Phase 5: Fix import path

### Changes

Refactor `import.js` to append parsed lines into the pane model, not directly into DOM.

### Acceptance criteria

- Importing a 5MB log does not create one DOM node per line.
- Imported logs support filtering, selection, markers, sync, download.

## Phase 6: Plugin and timestamp handling

### Changes

- `getLine(rawIndex)` lazily builds/caches `StoredLine`.
- When plugins load or plugin settings change:
  - invalidate plugin-derived fields for affected pane;
  - rerender visible rows;
  - optionally re-analyze lazily on next access.
- Timestamp mode change:
  - update cached lines;
  - uncached lines apply timestamp mode when first built.

### Acceptance criteria

- Plugin summaries/tooltips appear for lines rendered before and after plugin load.
- Changing plugin setting updates visible rows.
- Timestamp toggle works without eagerly rebuilding every line where avoidable.

## Phase 7: Harden exported JSON embedding

### Changes

In `merge_logs.py`, escape embedded JSON script content:

```py
json_text = json.dumps(data, ensure_ascii=False).replace("</", "<\\/")
```

Apply anywhere log text is embedded inside `<script>`.

### Acceptance criteria

- Log line containing `</script>` does not break exported HTML.
- Exported file still hydrates correctly.

## Test plan

### Playwright

Add targeted tests:

1. **Large export opens**
   - Generate/open 100k-line exported HTML.
   - Assert first visible line renders.
   - Assert DOM line count stays bounded.

2. **Finite scrolling**
   - Scroll to middle.
   - Assert rendered `data-idx` is near middle.
   - Scroll to bottom.
   - Assert last rendered line has final index.

3. **Filter outside viewport**
   - Put rare match near end.
   - Filter for it.
   - Assert it becomes visible.

4. **Markers outside viewport**
   - Marker on line near end.
   - Navigate marker.
   - Assert correct line visible/highlighted.

5. **Selection after scrolling**
   - Scroll to middle.
   - Select range.
   - Export/copy selected lines.
   - Assert correct raw indices/content.

6. **Live tailing**
   - Send large UDP burst.
   - Assert DOM count remains bounded.
   - Assert last line visible when at bottom.

### Unit tests

- JSON escaping in `merge_logs.py`.
- Filter projection maps visible rows to correct raw indices.
- Virtual index math for top/middle/bottom.

## Dependency recommendation

A frontend framework migration is not the fix. The core work is independent of template syntax:

- stable scroll geometry,
- index projection under filtering,
- lazy parsing/plugin analysis,
- variable row heights,
- exported static HTML compatibility.

Recommended path:

1. First implement a plain-JS virtual log pane.
2. Keep frontend modules and static export model intact.
3. If dependencies become acceptable later, consider a small framework-agnostic virtualizer such as `@tanstack/virtual-core`, not a full frontend rewrite.
