# UI / E2E regression test plan

This plan lists the core frontend behaviors that should be protected with Playwright tests. The focus is on real browser behavior: layout, WebSocket-driven logs, pane synchronization, selections, snippets, and exported HTML.

## Test levels

### Level 1 — Live UI against demo backend

Run the real backend/demo and test the browser UI over WebSocket.

Purpose:

- catch regressions in live WebSocket flow
- catch DOM/event/selection regressions
- verify downloads generated from live in-memory logs

Default target:

```text
http://127.0.0.1:8080/
```

### Level 2 — Exported HTML replay

Generate/download an exported HTML snapshot, open it as a static file in Playwright, and repeat key UI tests.

Purpose:

- ensure exported HTML uses the same frontend behavior
- ensure no WebSocket dependency leaks into static exports
- catch broken embedded assets/bootstrap/log data

### Level 3 — Focused frontend logic helpers, optional later

If some logic becomes complex enough, extract pure helpers and test them with Node test runner/Vitest.

Candidates:

- timestamp parsing/normalization
- snippet range filtering
- raw snippet cleanup
- merge/sort order

Current recommendation: start with Playwright E2E first, because most regressions are browser/UI integration issues.

---

## Core behaviors to test

## 1. Live startup and WebSocket connection

### Test: UI connects to backend

Steps:

1. Open `/`.
2. Wait for `#ws-status` to show connected.
3. Assert configured panes exist.
4. Wait until logs appear.

Assertions:

- `#ws-status` contains `connected`
- `#pane-SENSOR_A`, `#pane-SENSOR_B`, `#pane-SENSOR_C` exist
- total `.log-line` count becomes `> 0`

Why:

- protects module loading, WebSocket setup, config message handling, dynamic pane creation

---

## 2. Pane/tab layout

### Test: initial demo layout is correct

Expected demo config:

- Tab `Simulated Devices`: `SENSOR_A` + `SENSOR_B` side-by-side
- Tab `Other Sensor`: `SENSOR_C`

Steps:

1. Open live UI.
2. Check tab buttons.
3. Check first tab shows two panes.
4. Switch to second tab.
5. Check second tab shows `SENSOR_C`.

Assertions:

- tab labels are visible
- first tab has two visible panes
- second tab has one visible pane
- splitters exist only where expected

Why:

- protects backend-configured layout and tab switching

### Test: pane order is stable

Steps:

1. Open first tab.
2. Read visible pane names left-to-right.

Assertions:

- order is `SENSOR_A`, `SENSOR_B`

Why:

- protects layout ordering and avoids accidental pane swaps/regressions

---

## 3. Log rendering

### Test: live log lines render with timestamps and raw content

Steps:

1. Wait for lines in a pane.
2. Inspect first visible line.
3. Toggle timestamp visibility if setting exists.

Assertions:

- line contains `.ts`
- line text is non-empty
- ANSI/severity tags like `<err>` are safely rendered as text, not HTML elements

Why:

- protects rendering, ANSI parsing, HTML escaping

### Test: filtering hides/shows matching lines

Steps:

1. Wait for logs.
2. Type a known regex/string into filter input.
3. Assert visible line count changes.
4. Clear filter.

Assertions:

- matching lines remain visible
- non-matching lines are hidden
- clearing filter restores lines

Why:

- protects filter/rerender path

---

## 4. Time synchronization between panes

This is one of the most important features.

### Test: clicking a line sync-scrolls sibling pane

Steps:

1. Ensure first tab has `SENSOR_A` and `SENSOR_B` with enough logs.
2. Pick a line in `SENSOR_A` with timestamp `t`.
3. Click it.
4. Observe `SENSOR_B` scroll position / highlighted nearest timestamp.

Assertions:

- clicked line gets sync highlight or selection behavior expected by the app
- sibling pane scrolls to the closest timestamp
- highlighted line in sibling pane has timestamp close to `t`

Tolerance:

- exact timestamp match may not exist; compare numeric timestamp distance against nearest available line

Why:

- protects `onLineClick`, `syncPanes`, timestamp numeric ordering, cross-pane alignment

### Test: middle click / explicit sync behavior, if supported

Steps:

1. Middle-click a line or trigger the supported equivalent.
2. Switch tabs.

Assertions:

- app keeps sync timestamp when switching tabs if that is the intended behavior

Why:

- protects persistent sync mode/tab-switch sync behavior

---

## 5. Drag selection

### Test: drag-select range in one pane

Steps:

1. Wait for at least several lines in `SENSOR_A`.
2. Drag from line `n` to line `n+3`.

Assertions:

- at least 4 lines get `.selected`
- `#copy-actions-SENSOR_A` becomes visible
- buttons are visible: `Copy`, `Clipboard add`, `Copy range`, `Raw file`, `HTML snippet`

Why:

- protects pointer selection and overlay actions

---

## 6. Shift+Click range selection

### Test: Shift+Click selects a contiguous range

Steps:

1. Click line `n`.
2. Shift+Click line `n+4` in the same pane.

Assertions:

- exactly or at least 5 contiguous lines are selected
- overlay is visible
- selection does not affect other panes

Why:

- protects the faster range-selection UX

### Test: Shift+Click does not break normal click sync

Steps:

1. Click a line normally.
2. Assert no range selection appears.
3. Assert normal sync behavior still happens.

Why:

- prevents selection UX from breaking time sync

---

## 7. Clipboard/copy actions

### Test: Copy selected lines

Steps:

1. Select several lines.
2. Click `Copy`.
3. Read browser clipboard.

Assertions:

- clipboard contains selected pane lines only
- line count matches selection
- no HTML tags are copied

Notes:

- requires clipboard permissions in Playwright context

Why:

- protects direct copy flow

### Test: Clipboard add buffer

Steps:

1. Select lines in pane A.
2. Click `Clipboard add`.
3. Select lines in pane B.
4. Click `Clipboard add`.
5. Open clipboard buffer peek.

Assertions:

- indicator shows accumulated line count
- peek contains both selections separated by blank space
- `Copy all` works
- `Clear` empties buffer

Why:

- protects internal clipboard buffer UX

---

## 8. Raw range snippet download

### Test: Raw file downloads merged synchronized snippet

Steps:

1. Select range in one pane.
2. Click `Raw file`.
3. Read downloaded `.log`.

Assertions:

- filename starts with `embed-log-snippet-`
- file has merged lines from all panes in selected time range
- each line has normalized prefix:

```text
[MM-DD HH:MM:SS.mmm] [SOURCE] message
```

- no duplicate source prefix:

```text
[SENSOR_A] [SENSOR_A]
```

- no duplicate ISO timestamp after normalized prefix:

```text
[SENSOR_A] [2026-...]
```

- lines are sorted by timestamp

Why:

- protects the LLM/agent-friendly raw snippet feature

### Test: Copy range matches raw file content

Steps:

1. Select a range.
2. Click `Copy range`.
3. Click `Raw file`.
4. Compare clipboard text to downloaded file content, ignoring final newline.

Assertions:

- contents match

Why:

- protects consistency between clipboard and download paths

---

## 9. HTML snippet download

### Test: HTML snippet uses regular embed-log export UI

Steps:

1. Select a range.
2. Click `HTML snippet`.
3. Read downloaded `.html`.

Assertions:

- filename starts with `embed-log-snippet-`
- contains normal exported UI markers:
  - `#toolbar`
  - `#tab-bar`
  - `var _logData =`
  - pane HTML
- does not contain custom snippet-only UI like `<h1>embed-log snippet</h1>`

Why:

- protects decision to reuse normal export UI, not maintain a second view

### Test: Open HTML snippet and verify layout/logs

Steps:

1. Download HTML snippet.
2. Open downloaded file with Playwright.
3. Wait for bootstrap to load logs.

Assertions:

- toolbar exists
- tab bar exists
- panes exist side-by-side according to original layout
- log lines exist only in selected time range
- no WebSocket connection is required/visible

Why:

- protects exported snippet usability

---

## 10. Full session HTML export

### Test: Export current live session

Steps:

1. Wait for logs.
2. Click toolbar `Export`.
3. Download full HTML.
4. Open it with Playwright.

Assertions:

- full exported HTML opens without JS errors
- panes/tabs match live layout
- logs are present
- basic UI actions work in static file:
  - tab switch
  - filter
  - wrap toggle
  - selection
  - snippet raw/html from exported file if supported

Why:

- protects primary shareable session artifact

### Test: Save HTML / Current HTML backend export

Steps:

1. Click `Save HTML`.
2. Wait for status ready.
3. Click `Current HTML`.

Assertions:

- backend-generated session HTML opens
- contains expected panes/logs

Why:

- protects backend export integration

---

## 11. Session and cache UX

### Test: Clear UI cache

Steps:

1. Wait for logs.
2. Click settings gear.
3. Click `Clear cache`.
4. Reload page.

Assertions:

- page reconnects cleanly
- no duplicated restored lines

Why:

- protects localStorage refresh cache behavior

### Test: Clean session rotation

Steps:

1. Wait for logs.
2. Click `Clean session` and confirm.
3. Wait for UI reset/session rotated event.

Assertions:

- panes are cleared
- new logs appear under new session
- session id changes
- previous session appears in Sessions popup

Why:

- protects session rotation, UI reset, cache scoping

---

## 12. Sessions popup and raw log links

Current UI does not yet expose per-source raw log download prominently. When added, test:

### Test: Sessions popup exposes session artifacts

Steps:

1. Open settings.
2. Open `Sessions` popup.

Assertions:

- current session is marked
- `open html` link exists when ready
- `manifest` link exists
- future: raw per-source log links exist
- future: merged raw session link exists

Why:

- protects saved session browsing UX

---

## 13. Keyboard behavior

### Test: Escape clears selection / closes popups

Steps:

1. Select range.
2. Press Escape.

Assertions:

- selection clears
- overlay hides

### Test: Cmd/Ctrl+C copies current selection

Steps:

1. Select range.
2. Press platform copy shortcut.
3. Read clipboard.

Assertions:

- selected lines copied

Why:

- protects keyboard workflow

---

## 14. No-JS-error baseline

For every major flow, collect console errors.

Implementation idea:

```js
const errors = [];
page.on('console', msg => {
  if (msg.type() === 'error') errors.push(msg.text());
});
page.on('pageerror', err => errors.push(String(err)));
```

Assertions:

- no uncaught JS exceptions
- no failed module import errors

Why:

- catches cache/module/export regressions early

---

## Suggested implementation order

### Phase 1 — Must-have smoke/regression

- [x] Live UI connects and receives logs
- [x] Shift+Click selects range
- [x] Raw file downloads cleaned merged snippet
- [x] HTML snippet uses regular export UI
- [x] Open downloaded HTML snippet and verify layout/logs
- [x] Live pane layout/tab layout test
- [x] Time synchronization between panes

### Phase 2 — Export confidence

- [x] Full toolbar `Export` download
- [x] Open full exported HTML and repeat layout/filter/selection tests
- [ ] Backend `Save HTML` / `Current HTML` flow

### Phase 3 — UX details

- [ ] Drag selection
- [ ] Clipboard direct copy
- [ ] Clipboard add/peek/copy-all/clear
- [x] Escape clears selection
- [ ] Cmd/Ctrl+C copies selection

### Phase 4 — Session workflows

- [ ] Clean session rotation
- [ ] Sessions popup artifacts
- [ ] Future raw log links

### Phase 5 — Optional helper/unit tests

- [ ] timestamp parser helper tests
- [ ] snippet cleanup helper tests
- [ ] range merge/sort helper tests

---

## Test data strategy

Prefer deterministic test data where possible.

Current demo generates random logs, which is useful for smoke testing but not ideal for exact assertions. For stronger tests, add a test-only log injector/profiles:

- fixed sequence of UDP messages with known timestamps/content
- fixed inject markers
- short runtime
- high frequency

Potential future command:

```bash
./run_demo.sh --no-browser --test-profile deterministic
```

or env var:

```bash
DEMO_PROFILE=test
```

This would make time sync and exact snippet assertions much easier.
