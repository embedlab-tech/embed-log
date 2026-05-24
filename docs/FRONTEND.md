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
- settings panel
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
- Live UI and exported HTML reuse the same general UI model.
- Not all panes are visible at once; current demo has multiple tabs.
- Exported/static HTML may use slugged pane ids; tests should prefer pane labels when DOM ids are unstable across live/export paths.

## Demo layout used by tests

- `Simulated Devices`:
  - `SENSOR_A`
  - `SENSOR_B`
- `Other Sensor`:
  - `SENSOR_C`

This matters for assertions: a test that expects all three panes to be visible at once is wrong.

## Fragile areas

- session export/open flow (`Save HTML`, `Current HTML`)
- session rotation clearing stale state
- selection/clipboard/download path consistency
- cache restore after layout mutations
- cross-tab sync behavior when switching tabs after explicit sync gesture
- frontend behavior under invalid regex input or malformed imported HTML

## UI testing guidance

- Prefer deterministic waits using `tick=...` and `kind=...` markers.
- Use `tests-ui/tests/helpers.js` helpers instead of ad hoc sleeps.
- Capture page errors with `collectPageErrors(page)` in E2E tests.
- When testing exported HTML, assert tab/pane behavior, not assumptions copied from the live DOM.
