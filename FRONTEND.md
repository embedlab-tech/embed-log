# log-viewer — browser UI

Live two-pane serial log viewer served by the log-viewer server over HTTP and WebSocket.

---

## Overview

Open `http://<host>:<ws-port>/` in a browser after starting the server with `--ws-port`.
The page connects automatically to `ws://<host>:<ws-port>/ws` and streams all serial RX/TX events in real time. No build step or bundler — plain HTML, CSS, and vanilla JS.

---

## File structure

```
frontend/index.html      HTML structure — toolbar, tab bar, two panes, script tags
frontend/viewer.css      All styles — themes, layout, ANSI color classes
frontend/state.js        Shared mutable state, PANES and TABS constants
frontend/ansi.js         ANSI escape sequence parser, timestamp utility
frontend/lines.js        Line rendering, pane sync, highlight, clear, jump-to-bottom
frontend/tabs.js         Tab bar rendering and tab switching
frontend/ui.js           Toolbar controls, filter inputs, TX input, splitter drag
frontend/ws.js           WebSocket connection, auto-reconnect, message dispatch
frontend/export.js       HTML export — builds a self-contained snapshot file
```

---

## Block diagram

```
 Browser
 ┌─────────────────────────────────────────────────────────────┐
 │  index.html                                                 │
 │  ┌───────────────────────────────────────────────────────┐  │
 │  │  Toolbar  (Wrap · Time · Sync · A- · A+ · Clear ·     │  │
 │  │            Export · Theme select · WS status)         │  │
 │  └───────────────────────────────────────────────────────┘  │
 │  ┌─────────────────────┐  ┌─────────────────────────────┐   │
 │  │  Pane: outside      │  │  Pane: inside               │   │
 │  │  ┌───────────────┐  │  │  ┌───────────────────────┐  │   │
 │  │  │  Filter input │  │  │  │  Filter input         │  │   │
 │  │  └───────────────┘  │  │  └───────────────────────┘  │   │
 │  │  ┌───────────────┐  │  │  ┌───────────────────────┐  │   │
 │  │  │  log-outside  │  │  │  │  log-inside           │  │   │
 │  │  │  .log-line×N  │  │  │  │  .log-line×N          │  │   │
 │  │  └───────────────┘  │  │  └───────────────────────┘  │   │
 │  │  [ Serial TX input ]│  │  [ Serial TX input        ]  │   │
 │  └─────────────────────┘  └─────────────────────────────┘   │
 │                                                              │
 │  Script load order (plain <script> tags, global scope):      │
 │    state.js → ansi.js → lines.js → ui.js → ws.js →          │
 │    export.js                                                 │
 └─────────────────────────────────────────────────────────────┘
          │ WebSocket ws://<host>:<port>/ws
          ▼
     server.py  WebSocketBroadcaster
```

---

## Script responsibilities

### `state.js`
Defines the single shared `state` object and `PANES` constant. All other scripts read and write `state` directly — no module system, intentionally simple.

```js
const PANES = ["outside", "inside"];

const state = {
    wrap, showTs, syncEnabled, fontSize,
    filters,     // { outside: RegExp|null, inside: RegExp|null }
    rawLines,    // { outside: Line[], inside: Line[] }
    atBottom,    // { outside: bool, inside: bool }
    highlighted, // { outside: div|null, inside: div|null }
};
```

### `ansi.js`
- `parseAnsi(raw)` — converts raw text with ANSI escape sequences to HTML. SGR color/bold codes become `<span class="ansi-N">`. All other sequences (cursor movement `ESC[nD`, erase `ESC[J`, OSC, bare ESC) are silently stripped.
- `tsToNum(ts)` — converts `"MM-DD HH:MM:SS.mmm"` to a sortable integer by stripping non-digits. Used for binary search in pane sync.

### `lines.js`
Core display and interaction logic:

- **`appendLine(paneId, ts, rawText, isTx)`** — parses ANSI, appends a `.log-line` div, attaches the click handler, auto-scrolls if at bottom.
- **`rerenderPane(paneId)`** — rebuilds innerHTML for all existing divs after a filter / timestamp toggle / font change.
- **`onLineClick(paneId, numTs, div)`** — click handler:
  - If filter active: clears filter, re-renders, scrolls source pane to the line.
  - If no filter: source pane does not move.
  - Always: highlights the clicked line, calls `syncPanes`.
- **`syncPanes(fromId, numTs, clickedDiv)`** — binary-searches `rawLines[toId]` for the closest timestamp, scrolls the target pane so the matched line lands at the same Y offset as the clicked line, highlights the matched line.
- **`highlightLine(paneId, div)`** — persistent highlight (removes previous, adds `.sync-highlight`).
- Jump-to-bottom button logic and scroll listeners.
- Clear (single pane and all panes).

### `ui.js`
Wires up all toolbar buttons and interactive controls to `state` and DOM:

- Wrap toggle → `.log-area.wrap`
- Timestamp toggle → `rerenderPane`
- Font size → CSS variable `--font-size`
- Sync toggle → `state.syncEnabled`
- Theme select → `data-theme` attribute on `<html>`
- Filter inputs → compile RegExp into `state.filters`, call `rerenderPane`
- Serial TX input → Enter / Send button → `wsSend({ cmd:"send_raw", ... })`
- Splitter drag → pointer capture, width update on both panes

### `ws.js`
- Connects to `ws://<window.location.host>/ws`.
- On `message`: routes `{type, data, timestamp, source_id}` to `appendLine`.
- On `close`: exponential backoff reconnect (1 s → 2 s → … → 16 s).
- `wsSend(obj)` — used by `ui.js` (TX input) and `export.js` is read-only.

### `export.js`
On **Export** click:
1. Reads all current CSS variable values from the live document via `getComputedStyle` — the exported file always matches the active theme.
2. Renders both panes (`state.rawLines`) into a self-contained HTML string with inline `<style>`.
3. Triggers a browser download as `log-viewer-<ISO-timestamp>.html`.

No server round-trip; the export is entirely client-side.

---

## WebSocket message format

### Server → browser (per log entry)

```json
{
  "type":      "rx",
  "data":      "free: 62832, used: 93976",
  "timestamp": "03-25 11:50:00.011",
  "source_id": "outside"
}
```

| Field | Values | Notes |
|---|---|---|
| `type` | `"rx"` / `"tx"` | `tx` = serial TX sent to device |
| `data` | string | Raw text, may contain ANSI escape codes |
| `timestamp` | `"MM-DD HH:MM:SS.mmm"` | Local time on the server |
| `source_id` | `"outside"` / `"inside"` | Must match a pane ID in the HTML |

### Browser → server (serial TX from the UI input)

```json
{ "cmd": "send_raw", "id": "outside", "data": "reboot\n" }
```

---

## Themes

Defined as CSS custom property blocks in `viewer.css`. Switched via `data-theme` attribute on `<html>`.

| Name | `data-theme` | Description |
|---|---|---|
| Mocha | `""` (default) | Catppuccin Mocha — dark |
| Whitesand | `"whitesand"` | Warm cream light theme |
| Legacy | `"legacy"` | Plain white, Bootstrap blue — matches the original legacy UI |

---

## Pane sync — how it works

1. Each `.log-line` div has its timestamp stored as a numeric value at append time (`tsToNum`).
2. Clicking a line calls `syncPanes(fromId, numTs, clickedDiv)`.
3. Binary search finds the line in the other pane with the closest numeric timestamp.
4. The target pane is scrolled so `targetDiv.offsetTop - logEl.scrollTop` equals `clickedDiv.offsetTop - fromLogEl.scrollTop` — the matched line lands at the exact same vertical position as the clicked line.
5. Both lines receive a persistent `.sync-highlight` outline that stays until the next click.

The sync toggle button disables step 2–5 without removing existing highlights.

---

## Adding a new pane / device

1. Add a `<div class="pane" id="pane-<id>">` block to `index.html` with matching filter, log area, jump button, and TX input (follow the `outside` / `inside` pattern).
2. Add `"<id>"` to the `PANES` array in `state.js`.
3. Configure the device with `source_id: "<id>"` in `config.yaml` or via `--device NAME PORT SOCKETPORT <id>`.
