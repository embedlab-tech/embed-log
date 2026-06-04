# FRONTEND

## What the frontend is

The frontend is a plain-browser UI for live and replayed logs.

It is intentionally simple:
- vanilla JS modules,
- no build step,
- backend-driven tabs/panes,
- same UI reused for exported/replayed HTML.

## Main features

- tabbed log viewer with one or two panes per tab
- live WebSocket updates
- line click sync and cross-pane highlighting
- regex filtering per pane
- range selection:
  - click,
  - shift-click,
  - drag selection
- clipboard flows:
  - copy current selection,
  - clipboard buffer,
  - raw snippet download,
  - HTML snippet download
- full snapshot export
- settings panel:
  - theme/font/cache controls
  - timestamp mode toggle (`absolute` / `relative`) when both views are available
  - visible hint when imported/offline data lacks the session origin needed for conversion
- sessions popup
- clean-session rotation trigger
- cache-backed layout/state persistence
- pane swap UI and splitter dragging
## Key files

- `frontend/state.js` — shared state, `TABS`, `PANES`, active tab, filters, sync state
- `frontend/ws.js` — WS connect/reconnect and incoming message handling
- `frontend/lines.js` — rendering, scroll behavior, sync, highlights
- `frontend/tabs.js` — tab bar and tab switching
- `frontend/tabcreate.js` — pane/tab DOM creation
- `frontend/ui.js` — toolbar, settings panel, sessions, clean-session flow
- `frontend/selection.js` — range selection, clipboard, snippet export
- `frontend/export.js` — full export generation
- `frontend/persist.js` — session-aware cache persistence

## Scalable log rendering

The UI keeps large live sessions and exported HTML smooth by separating the log data model from the DOM.

Each pane stores the complete log history in `state.rawLines[paneId]`, but the browser only renders the rows that intersect the current viewport plus a small overscan buffer. The DOM shape is:

```html
<div class="log-area" id="log-...">
  <div class="log-spacer">
    <div class="log-window"></div>
  </div>
</div>
```

- `.log-spacer` has the full virtual height (`visible row count × measured row height`), so the browser scrollbar has a stable, finite range even for multi-megabyte logs.
- `.log-window` contains only the currently rendered `.log-line` elements.
- Every rendered row carries `data-idx`, the raw line index. Code must use that index, not DOM child position.
- Live logs, imports, and exported replay all append to the same pane model and rerender the visible window instead of appending one DOM node per line.
- Exported/replayed logs and live appends may store compact tuples (`[ts, rawText, isTx, meta]`). Full line objects, ANSI parsing, timestamp view selection, and plugin analysis are built lazily by `getLine(paneId, idx)` only when a row is rendered or otherwise needed.
- Filtering builds a projection of matching raw indices. Virtual scrolling then operates over that projection, so rare matches far outside the current viewport are reachable without rendering the whole log.
- Sync highlights are persisted by raw line index and reapplied when virtualized rows are recreated during scrolling, tab switching, font-size changes, or live updates.

This keeps DOM size bounded while preserving full-history operations such as export, download, selection, markers, filtering, and timestamp synchronization.
## Important runtime assumptions

- Tabs and panes come from backend `config.tabs`.
- Pane ids remain unique technical keys; visible pane names may now come from backend `config.pane_labels`.
- Live runtime stores every received line in the pane model; the rendered DOM is a bounded virtual window over the full live/exported history.
- Live UI and exported HTML reuse the same general UI model.
- Timestamp mode switching is a pure view switch when both timestamp representations are available; live/export paths preserve enough metadata to render both offline.
- Not all panes are visible at once; current demo has multiple tabs.
- Exported/static HTML may use slugged pane ids; tests should prefer pane labels when DOM ids are unstable across live/export paths.
## Demo layout used by tests

- `DevA`:
  - `SENSOR_A` rendered as `READER`
  - `SENSOR_B` rendered as `CONTROLLER`
- `DevB`:
  - `SENSOR_C` rendered as `READER`

This matters for assertions: a test that expects all three panes to be visible at once is wrong.

## Fragile areas

- session export/open flow (`Save HTML`, `Current HTML`)
- session rotation clearing stale state
- timestamp-mode conversion when session origin metadata is missing or delayed
- selection/clipboard/download path consistency
- cache restore after layout mutations
- cross-tab sync behavior when switching tabs after explicit sync gesture
- frontend behavior under invalid regex input or malformed imported HTML
## UI testing guidance

- Prefer deterministic waits using `tick=...` and `kind=...` markers.
- Use `tests-ui/tests/helpers.js` helpers instead of ad hoc sleeps.
- Capture page errors with `collectPageErrors(page)` in E2E tests.
- When testing exported HTML, assert tab/pane behavior, not assumptions copied from the live DOM.
