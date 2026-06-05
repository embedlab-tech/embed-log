# Frontend plugins

Plugins inspect log lines and attach structured metadata — inline text, tooltips with detailed protocol breakdowns, and filter keywords. The built-in `hex-coap` plugin is the reference implementation.

## Quick start — the 30-second plugin

```js
// my-plugin.js
(function () {
  window.EmbedLogPlugins.register({
    apiVersion: 1,
    kind: 'line',
    name: 'my-plugin',
    displayName: 'My Plugin',
    analyzeLine(ctx) {
      if (/ERROR/.test(ctx.rawText)) {
        return {
          label: 'ERROR',
          summary: 'Error detected',
          inlineText: '[ERR]',
          filterText: 'error my-plugin',
          classNames: ['line-plugin-match', 'line-plugin-error'],
        };
      }
      return null; // nothing matched
    },
  });
})();
```

## Configuration

Plugins are declared in the YAML config under `frontend_plugins` and assigned to panes under `tabs[].panes[].plugins`.

```yaml
frontend_plugins:
  hex-coap:
    builtin: hex-coap           # built-in plugin
  my-plugin:
    path: plugins/my-plugin.js  # custom plugin from a file

tabs:
  - label: Net
    panes:
      - source: COAP_DEVICE
        plugins: [hex-coap, my-plugin]
      - source: TELEMETRY       # no plugins on this pane
```

### Built-in vs custom

| Type | Config field | Plugin loaded from |
|------|-------------|--------------------|
| Built-in | `builtin: hex-coap` | Embedded in `frontend/plugin-hex-coap.js` |
| Custom | `path: plugins/my-plugin.js` | Filesystem path (relative to config) |

Custom plugin scripts are hashed (SHA-256) and the hash is sent to the frontend so it can cache/reload them correctly. The server reads the file at startup; hot-reload is not supported.

### Per-pane plugin options

Plugins can accept options configured per-pane:

```yaml
tabs:
  - label: Net
    panes:
      - source: COAP_DEVICE
        plugins:
          - name: hex-coap
            options:
              allLogs: true     # show inline decode on every line
```

Options are merged with defaults: config values override defaults, UI settings override both.

---

## Plugin API (`analyzeLine`)

Every plugin must export a function via `window.EmbedLogPlugins.register(definition)`.

### `definition` object

| Field | Required | Description |
|-------|----------|-------------|
| `apiVersion` | Yes | Must be `1` |
| `kind` | Yes | Must be `'line'` |
| `name` | Yes | Unique plugin identifier (e.g. `'hex-coap'`) |
| `displayName` | No | Human-readable name shown in UI |
| `settings` | No | Array of setting definitions (see below) |
| `analyzeLine(ctx)` | Yes | Function called for every log line on configured panes |

### `analyzeLine(ctx)` → return value

Called once per log line on each pane the plugin is assigned to.

**`ctx` fields supplied to the plugin:**

```js
ctx = {
  paneId,      // string — source name this line belongs to
  options,     // object — merged config/UI settings for this pane+plugin
  rawText,     // string — full text of the log line
  html,        // string — HTML-rendered version of the line (tags stripped for matching)
  isTx,        // bool — true if this is a TX (transmitted) line
  timestamp,   // string — current display timestamp
  absTs,       // string|null — absolute timestamp
  absNum,      // number|null — absolute timestamp as epoch ms
  relTs,       // string|null — relative timestamp (T+…)
  relNum,      // number|null — relative timestamp as ms from first line
  utils: {
    escapeHtml(str)  // function — HTML-escape a string
  },
}
```

**Return value** — `null` if nothing matched, or an object:

| Field | Type | Description |
|-------|------|-------------|
| `label` | string | Short label (e.g. `'CoAP'`, `'ERROR'`). Added to filter index. |
| `summary` | string | One-line description shown in tooltip header (e.g. `'GET /sensors/temp'`). |
| `details` | string[] | Multi-line details shown in the tooltip body. |
| `inlineText` | string | Text injected into the line's inline display (visible in the log pane). |
| `filterText` | string | Additional text added to the filter index (not displayed in UI). |
| `classNames` | string[] | CSS class names added to the line div. Built-in: `line-plugin-match` (highlights the line). |
| `disableTooltip` | boolean | If `true`, suppresses the tooltip for this plugin match on this line. |

### Settings definitions

```js
settings: [
  {
    key: 'allLogs',              // option key name
    label: 'Show all logs',       // human-readable label in settings UI
    type: 'bool',                 // 'bool', 'string', or 'number'
    defaultValue: false,          // default when not configured
    description: 'Show inline CoAP decode on every line',
  },
]
```

Settings appear in the per-pane plugin configuration gear menu (`Configure pane plugins`).

---

## How plugins run (lifecycle)

1. **Config load** — server reads plugin scripts, computes SHA-256, sends metadata in the WS `config` message.
2. **Frontend init** — `ws.js` receives the config, calls `configurePanePlugins()` which loads and executes each plugin script (via `<script>` injection).
3. **Line rendering** — when a log line arrives or a pane re-renders, `analyzeLinePlugins(paneId, line)` iterates over configured plugins and calls `analyzeLine(ctx)` on each.
4. **Results stored** — plugin output is stored on the line object as `line.pluginData`, `line.pluginInlineText`, `line.pluginFilterText`, `line.pluginClassNames`.
5. **UI display** — inline text is shown in the log pane; tooltips appear on hover; filter text extends regex matching.

---

## Plugin tooltip

When a line has plugin data, hovering over it shows a tooltip. The tooltip header is `label — summary`. The body is the `details` array, one per line.

Set `disableTooltip: true` in the return value to suppress the tooltip (useful when `allLogs` mode shows inline text on every line and the tooltip would be noise).

---

## Built-in reference: hex-coap

`frontend/plugin-hex-coap.js` is the canonical example. It:

1. Scans `rawText` for hex strings that look like CoAP packets (8+ hex chars).
2. Parses the CoAP header: version, type, code, message ID, token.
3. Decodes CoAP options (Uri-Path, Content-Format, Block1/Block2, Observe, etc.).
4. Returns a rich result with:
   - `inlineText` — compact one-liner: `v:1 t:CON c:GET i:01a4 {ab12} [Uri-Path:sensors/temp] :: data len 0`
   - `summary` — `GET /sensors/temp`
   - `details` — full structured breakdown of every header and option
   - `filterText` — all CoAP metadata added to filter index

---

## Registering a plugin at runtime

Plugins are loaded by the server, but you can also register from the browser console for development:

```js
window.EmbedLogPlugins.register({ ... });
// Then trigger a re-render:
// state.filters = {}; PANES.forEach(id => rerenderPane(id));
```
