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

## Important runtime assumptions

- Tabs and panes come from backend `config.tabs`.
- Pane ids remain unique technical keys; visible pane names may now come from backend `config.pane_labels`.
- Live runtime appends every received line into the pane DOM; the on-screen log must match the full live/exported history.
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
