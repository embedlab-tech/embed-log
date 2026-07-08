# Rust TUI Frontend — Implementation Plan

A new terminal UI (TUI) frontend for `embed-log`, built with **ratatui + crossterm**,
that inherits the full browser/Tauri feature surface (tabs, panes, selection, markers,
events timeline, plugins, TX, sessions, export, onboarding) and reuses the same
`embed-log-core` runtime. The demo app gets a `--tui` mode.

## Execution order (MVP-demo-first)

The phases below are written in dependency order. **Execution order is reordered to
get a runnable one-command demo ASAP**, then layer the full feature surface:

1. **Phase 0** — scaffold ✅ (done)
2. **Phase 1** — WS client + protocol model + state
3. **Phase 2** — app shell, event loop, layout
4. **Phase 3** — log rendering & virtualization → *visible TUI against a running server*
5. **Phase 11** — CLI integration (`embed-log run --tui` / `demo --tui` call `run_in_process`)
6. **Phase 12** — demo for TUI (`just demo-tui` works end-to-end) → **one-command MVP demo milestone**
7. **Phase 4** — interaction: tabs, panes, focus, sync, scroll
8. **Phase 5** — selection, copy, markers
9. **Phase 6** — events tab + timeline
10. **Phase 7** — TX input & command suggestions
11. **Phase 8** — plugins (Rust hex-coap builtin)
12. **Phase 9** — themes, settings, sessions, export, rotation, stats
13. **Phase 10** — onboarding (native TUI)
14. **Phase 13** — tests
15. **Phase 14** — docs, config samples, release

Phases 11–12 only need the `run_in_process` entry point + Phases 1–3's client, so the
reorder is clean — they don't depend on selection/markers/events.

## Architecture overview

```
embed-log.yml (or embedded demo config)
  │
  ▼
embed-log CLI  ──┬── `run` / `demo` / default  ──▶ LogServer (in-process) ──▶ /ws + /api/*
                 │                                  ▲
                 ├── `--ui`           ──▶ Tauri webview shell (existing) ──┘ (loopback WS)
                 │
                 └── `--tui` (NEW)    ──▶ embed-log-tui shell ──┘ (loopback WS)
                                          │ ratatui + crossterm
                                          │ tokio-tungstenite WS client
                                          ▼
                                     same /ws + /api/v1/control + REST contract
                                     as the browser frontend
```

**Key decision: the TUI is a WS client, not a fork of the server.** It consumes the exact
same `/ws` config + replay + live-broadcast protocol and `/api/*` REST endpoints as
`frontend/*.js`. No new server-side code is required. This mirrors how the Tauri shell
works (it also just points a webview at the in-process Axum server).

Two invocation modes for the `embed-log-tui` crate:
- **In-process** (`embed-log run --tui`, `embed-log demo --tui`, `just demo-tui`):
  CLI starts `LogServer` headless in-process, then launches the TUI client connected to
  `ws://127.0.0.1:<port>/ws`. On TUI exit: shut down server + export current session
  (mirrors the Tauri close handler in `embed-log-tauri/src/lib.rs`).
- **Client-only** (`embed-log-tui --url ws://host:port` or `--config path`-derived URL):
  connects to an already-running server. Useful for remote/CI viewing.

## Workspace change

New crate `crates/embed-log-tui` (4th workspace member), binary `embed-log-tui`.
The crate exposes both a `bin` target and a library `run()`/`run_client()` entry point
so the CLI can call into it in-process without spawning a subprocess.

```toml
# crates/embed-log-tui/Cargo.toml
[package]
name = "embed-log-tui"
version.workspace = true
# ...

[dependencies]
embed-log-core.workspace = true      # config types, demo, onboarding reuse
tokio.workspace = true
futures.workspace = true
serde.workspace = true
serde_json.workspace = true
anyhow.workspace = true
chrono.workspace = true
regex.workspace = true
clap.workspace = true
ratatui = "0.28"                     # TUI rendering
crossterm = "0.28"                   # terminal I/O / event source
tokio-tungstenite = "0.24"           # WS client
arboard = "3"                        # clipboard (copy/yank) — optional, feature-gated
url = "2"
```

Add `"crates/embed-log-tui"` to the workspace `members`. `embed-log-cli` gains an
`embed-log-tui` workspace dependency (optional, feature-gated) so `--tui` can call the
lib entry point in-process. Release impact: `embed-log-tui` is pure-Rust (no system
webview), so it can either ship as a second binary in the existing release artifact or
be compiled into the `embed-log` CLI behind a feature. **Recommendation: ship as a
second binary `embed-log-tui` in the same release tarball** (one extra binary, no
workflow change beyond `package-cli` copying it).

## justfile / CLI integration

```just
# Run the TUI demo with generated demo traffic.
demo-tui:
    {{cargo}} build --package embed-log-tui --bin embed-log-tui
    {{cargo}} run --package embed-log-cli --bin embed-log -- demo --tui --no-open-browser

# Run the TUI against a config.
run-tui cfg=config:
    {{cargo}} run --package embed-log-cli --bin embed-log -- run --tui --config {{cfg}} --no-open-browser
```

CLI changes in `crates/embed-log-cli/src/main.rs`:
- New top-level flag `--tui` (sibling to existing `--ui`).
- `Command::Run` and `Command::Demo` gain a `--tui` flag (or the top-level `--tui`
  propagates). `cmd_run` / `cmd_demo`: when `--tui`, set `open_browser=false`, start
  `LogServer`, then call `embed_log_tui::run_in_process(port)` instead of scheduling a
  browser open. On return, stop server + export session.
- `cmd_tui(config_path)` mirrors `cmd_ui` (which spawns Tauri) but spawns/calls the TUI.

## WebSocket protocol contract (consumed unchanged from `embed-log-core`)

The TUI client must handle exactly what `frontend/ws.js` handles. On `/ws` connect the
server sends, in order:
1. **config** message
2. **replay** buffer (log entries)
3. **events replay** buffer
4. live broadcast messages

### Config message (`build_ws_config_message`, server.rs:944)

```jsonc
{
  "type": "config",
  "app_name": "embed-log demo",
  "theme_defaults": { "light": "...", "dark": "..." },
  "session": { /* SessionManager::build_session_info */ },
  "pane_labels": { "DUT": "DUT Device", ... },
  "pane_kinds":  { "DUT": "udp", "UART_DUT": "uart", ... },
  "pane_commands": { "UART_DUT": ["help\r\n", "version\r\n", ...] },
  "tabs": [ { "label": "Device", "panes": ["DUT","HOST"], "pane_labels": {...} }, ... ],
  "frontend_plugins": { "hex-coap": { "builtin": "hex-coap" } },
  "pane_plugins": { "COAP_RAW": [ {"name":"hex-coap"} ] },
  "plugin_scripts": { "hex-coap": "<js source>" },
  "event_rules": { "DUT": [ {"name":"fatal_error","severity":"error"}, ... ] },
  "markers": [ { "paneId":"DUT", "lineIdx":42, "endIdx":42, "numTs":..., "description":"...", "kind":"user"|"event", "severity":"..." } ]
}
```

### Live log payload (server.rs:1235)

```jsonc
{
  "type": "rx" | "tx",
  "data": "<ANSI-wrapped message>", "message": "<raw message>",
  "timestamp": "06-14 09:30:45.123", "timestamp_iso": "...", "timestamp_num": 1718347845123.0,
  "source_id": "DUT", "line_idx": 42, "origin": "SERIAL" | "TX::ui" | "...",
  "color": "cyan" | null,
  "absTs": "...", "absNum": ..., "relTs": "00:00:45.123", "relNum": 45123.0
}
```

### Event payload (server.rs:1259)

```jsonc
{ "type":"event", "event_id":"fatal_error", "source_id":"DUT", "severity":"error",
  "timestamp":..., "timestamp_iso":..., "timestamp_num":..., "rel_num":...,
  "line_idx":42, "message":"...", "origin":"SERIAL", "captures":[...] }
```

### Other broadcasts to handle
- `session_info` (first_log_at set / session rotation) — update `first_log_at`, timestamp context.
- `markers_update` — replace `state.markers`; re-render marker gutters + nav.
- `session_html_status` — update status bar export indicator.
- `session_rotated` — tear down layout, rebuild from new config message (server sends a fresh config).
- `clear_logs` — clear pane (or all panes) in state.

### Client → server
- `/ws` commands (same `handle_client_command`): `export_session_html`, `save_markers`, `clear_logs` (`{pane}`), `set_filter` (`{id, filter}`), `send_raw` (`{id, data}`).
- REST: `GET /api/health`, `/api/session/current`, `/api/sessions`, `/api/stats`, `POST /api/session/export`, `/api/session/rotate`, `/api/session/snippet`, `GET /sessions/{id}/{file}`.
- `/api/v1/control` (structured): `hello`, `subscribe`, `unsubscribe`, `log.inject`, `tx.write`, `marker.create` — used if the TUI needs structured inject/marker flows; primary path is `/ws` for parity with the browser frontend.

---

## Feature surface mapping (browser → TUI)

Every browser feature in `frontend/` has a TUI counterpart. Nothing is dropped.

| Browser (`frontend/*.js`) | TUI module | Notes |
| --- | --- | --- |
| `ws.js` connect/config/replay/reconnect | `tui::client` | tokio-tungstenite, backoff mirror ws.js (1s→16s). |
| `state.js` (TABS/PANES/labels/kinds/ts context) | `tui::state` | Same shape; per-pane line store. |
| `tabs.js` / `tabcreate.js` (tab bar, create, switch) | `tui::tabs` | ratatui `Tabs` widget; rebuild on session change. |
| `lines.js` (virtual windowed render, ANSI, tags, stats) | `tui::lines` | Windowed render w/ overscan; `ratatui::text::Line` + ANSI→`Style`. |
| `selection.js` (Exact/Context scope, copy, raw dl, snippets, markers, drag) | `tui::selection` | Keyboard range select; `arboard` clipboard; file write for download; `/api/session/snippet` for snippets. |
| `events.js` (SVG swimlane timeline) | `tui::events` | `ratatui::canvas::Canvas` braille scatter (x=time, y=lane), severity colors; zoom/pan/filter keys; Enter=sync. |
| `ui.js` (toolbar: unwrap, ts toggle, sessions, theme, swap, filter, TX, splitter) | `tui::ui` / keybindings | Keys drive all toolbar actions; popups for sessions/settings/theme. |
| `export.js` (HTML snapshot, raw download, hydrate) | `tui::export` | `export_session_html` cmd + REST; raw download = file write. |
| `persist.js` (localStorage session restore) | `tui::persist` | TOML/JSON cache under `~/.config/embed-log/tui/`. |
| `settings.js` / `themes.js` / `fontsize.js` | `tui::theme` | Palettes → `ratatui::Style` sets; light/dark quick toggle. Font size has no terminal equivalent → mapped to line-wrap toggle (documented gap). |
| `pluginRuntime.js` / `plugin-hex-coap.js` | `tui::plugin` + Rust `hex-coap` | JS plugins can't run in TUI; `TuiPlugin` trait + Rust builtin reimpls. Custom JS plugins show a "browser-only" indicator. |
| `tsparse.js` / `import.js` (static log import) | `tui::import` | Open `.log` files into panes for offline replay. |
| `onboarding.js` (first-run config builder) | `tui::onboarding` | Native TUI wizard; reuses `embed_log_core::onboarding` (serial port listing, `save_quick_config`). |

---

## Phases

### Phase 0 — Crate scaffold & workspace wiring
**Files:** `Cargo.toml`, `crates/embed-log-tui/{Cargo.toml,src/main.rs,src/lib.rs}`, `justfile`.
**Change:**
1. Add `crates/embed-log-tui` to workspace `members`; add `embed-log-tui = { path = "crates/embed-log-tui" }` to `[workspace.dependencies]`.
2. New crate: `lib.rs` exposes `run_in_process(port, app_name) -> Result<()>` and `run_client(url) -> Result<()>`; `main.rs` is a clap CLI (`--url`, `--config`) calling the library.
3. `justfile`: `demo-tui`, `run-tui`, `build-tui`, `tui-unit` recipes.
4. `embed-log-cli` gains `--tui` flag wiring (Phase 11 completes it).

**Acceptance:** `cargo build -p embed-log-tui` succeeds; `embed-log-tui --help` prints.

### Phase 1 — WS client + protocol model + state
**Files:** `crates/embed-log-tui/src/{client.rs, protocol.rs, state.rs}`.
**Change:**
1. `protocol.rs`: typed structs (serde) for `ConfigMessage`, `LogPayload` (rx/tx), `EventPayload`, `SessionInfo`, `Marker`, `MarkersUpdate`, `SessionHtmlStatus`, `SessionRotated`, `ClearLogs`. Tagged enum on `type`.
2. `client.rs`: connect `/ws`, read config → replay → events replay → live; reconnect with exponential backoff (1s→16s, mirror ws.js). Expose `mpsc<ServerEvent>` to the app and `mpsc<ClientCmd>` outbound.
3. `state.rs`: mirror `frontend/state.js` — `tabs: Vec<Tab>`, `panes: Vec<String>`, `pane_labels`, `pane_kinds`, `pane_commands`, `timestamp_mode`, `first_log_at`, `sync_ts`, `sync_tab_switch`, `filters: HashMap<pane, Regex>`, `raw_lines: HashMap<pane, Vec<StoredLine>>`, `markers`, `events`, `event_rules`, `events_enabled`. Per-pane line cap (virtualization backing store).

**Tests (unit):** parse each payload shape from JSON fixtures (reuse shapes from `ws_server.rs`/`server.rs` tests); state transitions: apply config → tabs/panes built; apply log → line appended; apply markers_update → markers replaced.

### Phase 2 — App shell, event loop, layout
**Files:** `crates/embed-log-tui/src/{app.rs, draw.rs, input.rs}`.
**Change:**
1. `app.rs`: owns `State`, two mpsc channels (server events, key events), draw-schedule debounce. `run()` enters alternate screen, loops until quit.
2. `draw.rs`: layout — top `Tabs` (tab labels + `Events` appended when enabled), center `TabContent` (1–2 panes via `Layout::horizontal` with resizable ratio), bottom `StatusBar` (ws status, session id, active pane stats, timestamp mode, key hints).
3. `input.rs`: crossterm event poll → key events into channel. Support kitty/keyboard enhancements where available.
4. Unwrap mode (`u`): flatten panes into single column; tab bar lists pane labels (mirror `tabs.js` unwrap branch).

**Tests (unit, `TestBackend`):** drive `app` with scripted keys + injected server events; assert rendered buffer (tab bar labels, status bar text, pane titles).

### Phase 3 — Log rendering & virtualization
**Files:** `crates/embed-log-tui/src/lines.rs`.
**Change:**
1. Windowed render: compute row height (1 line, or 2 if wrap), scroll offset, visible count + overscan (mirror `lines.js` OVERSCAN=60). Only format visible lines.
2. Line formatting: `ts` (abs or rel per `timestamp_mode`) + message. ANSI parse → `ratatui::Style`/`Color` (reuse `embed_log_core::models::Ansi` code map). Line tag classes (`<wrn>`, `[ERR]`, `[error]`) → color (mirror `_lineTagClass`). TX lines → yellow. Marker gutter: `▎` left border colored by marker kind/severity; marker description on the line or via popup.
3. Filter: per-pane regex; `matches_filter` skips non-matching lines from the visible window (but keeps raw indices stable). Filter input popup (`f`).
4. Stats: per-pane line count + UTF-8 byte count; status bar total (mirror `lines.js` `_updateToolbarStats`).

**Tests (unit):** ANSI→Style mapping; tag class detection; virtualization: 10k lines, scroll to middle → only N visible formatted; filter reduces visible count without shifting raw indices; byte count accrual.

### Phase 4 — Interaction: tabs, panes, focus, sync, scroll
**Files:** `crates/embed-log-tui/src/{tabs.rs, keys.rs}`.
**Change:** keybindings (documented in a help popup `?`):
- `Tab`/`Shift+Tab` cycle tabs; `1`-`9` jump; `u` unwrap; `t` timestamp mode.
- `h`/`l` or `Tab` move pane focus (2-pane tabs); `H`/`L` resize splitter.
- `j`/`k`/`↓`/`↑` scroll active pane; `g`/`G` top/bottom; `J` jump-to-bottom toggle.
- `Enter`/`<` on a line: set `sync_ts = line.absNum`, `sync_tab_switch=true`, sync other panes in active tab (`scroll_pane_to_ts` equivalent — binary search nearest line by `absNum`/`relNum`, center it). Highlight synced line. (Mirror `lines.js::onLineClick`/`syncPanes`.)
- `>` (middle-click equiv): clear active pane filter + sync "zoom out to this moment" (mirror `onMiddleClick`).

**Tests (unit, `TestBackend`):** tab switch renders correct panes; sync moves both panes; splitter resize changes column widths; unwrap flattens.

### Phase 5 — Selection, copy, markers
**Files:** `crates/embed-log-tui/src/{selection.rs, markers.rs}`.
**Change:**
1. Selection: `Space` toggle line; `v` visual range mode (move to extend); `Esc` clear. Scope toggle `c`: Exact (active pane) vs Context (selected + synchronized lines from all panes in tab, mirror `selection.js` `_rangeTargetPanes`/`_collectRangeEntries`).
2. Copy: `y` yanks scope to clipboard (`arboard`, feature-gated; fallback: write to `/tmp/embed-log-sel-<ts>.txt` and print path). Scope-aware text format (mirror `_formatSelectionBlock`/`_formatRangeRaw`).
3. Download raw: `d` writes scope raw to a file (prompt path via popup; default `./embed-log-raw-<pane>.txt`).
4. Snippets: `s` opens label popup → `POST /api/session/snippet` with text + panes + scope.
5. Markers: `m` toggles marker on active line (popup for description); persist via `save_markers` WS command (send full marker set for pane). Marker nav `[`/`]` prev/next (skip `kind:"event"` by default; `M` toggles include-event-markers, mirror `__embedLogToggleEventMarkers`). Marker tooltip: popup on `K` (hover equivalent). Event markers render with severity color.

**Tests (unit):** selection set/range/clear; scope toggle changes collected lines; marker toggle updates state + emits `save_markers` payload; marker nav skips event markers until toggled.

### Phase 6 — Events tab + timeline (TUI translation)
**Files:** `crates/embed-log-tui/src/events.rs`.
**Change:**
1. Events "tab" appended to tab bar when `events_enabled` (mirror `events.js` `initEventsTab`). Activated like a regular tab but renders the timeline view instead of panes.
2. Timeline: `ratatui::canvas::Canvas` with braille points — x = `timestamp_num` (epoch ms) auto-ranged `[first_log_at, latest]`, y = one lane per unique `event_id` (mirror `_computeLanes`). Points colored by severity (info=blue, warn=yellow, error=red, fatal=dark red). Lane labels rendered in the left margin.
3. Zoom `+`/`-` adjust `_viewRange`; `<`/`>` pan (mirror `_zoom`). Reset with `0`.
4. Filter popup (`F` in events tab): checkboxes for sources + severities (mirror `_renderFilters`).
5. Enter on nearest event → `sync_ts = event.timestamp_num`, switch to the tab containing `event.source_id` pane, `scroll_pane_to_ts` (mirror `_onEventClick`).
6. Tooltip popup on `K`: event message + captures (mirror `_showTooltip`).
7. Receive `type:"event"` live + `events_replay` on connect → push to `state.events`.

**Tests (unit):** lane assignment; range computation; filter reduces visible points; click→sync sets `sync_ts` and switches tab; `TestBackend` renders points within range.

### Phase 7 — TX input & command suggestions
**Files:** `crates/embed-log-tui/src/tx.rs`.
**Change:**
1. For writable panes (`pane_kinds[pane] == "uart"`), an input bar at the pane bottom. `:` or `i` (when pane focused) opens TX input. `Enter` sends `{cmd:"send_raw", id:<pane>, data:<text>+"\n"}` via `/ws`. Show `send_raw_result` in status bar.
2. Command suggestions: `pane_commands[pane]` — autocomplete popup while typing (or `Ctrl+Space`/`Tab` to cycle, mirror the browser Tab-cycling suggestions from companion `.commands.yml`).
3. TX lines render yellow with `origin: "ui"` (server already produces these).

**Tests (unit):** TX input sends correct `send_raw` payload; suggestion popup filters/cycles; non-writable pane shows no input bar.

### Phase 8 — Plugins (TUI translation)
**Files:** `crates/embed-log-tui/src/plugin.rs`, `crates/embed-log-tui/src/plugins/hex_coap.rs`.
**Change:**
1. `TuiPlugin` trait: `fn analyze(&self, raw: &str, opts: &Value) -> PluginAnnotation` and `fn detail(&self, raw: &str) -> String` (for the info popup).
2. Rust `hex-coap` builtin mirroring `frontend/plugin-hex-coap.js`: decode CoAP hex → method/code/options summary. Annotation shown as a suffix or a right-aligned column on the line.
3. Plugin info popup (`p` on a line in a pane with plugins): full decoded detail (mirror `_renderPluginInfo`).
4. `pane_plugins` from config maps panes → plugin names; only builtins with a Rust implementation render annotations. Unknown/JS-only plugins: show a `◆` indicator + "(browser-only plugin)" note in the info popup.
5. Plugin settings: persisted in `~/.config/embed-log/tui/plugins.toml` (mirror `setPanePluginSetting`).

**Tests (unit):** hex-coap decodes known payloads (reuse `COAP_HEX_LIST` from `tests-ui/rust-demo-server.mjs` as fixtures); annotation attaches to matching lines; info popup content.

### Phase 9 — Themes, settings, sessions, export, rotation, stats
**Files:** `crates/embed-log-tui/src/{theme.rs, settings.rs, sessions.rs, export.rs}`.
**Change:**
1. `theme.rs`: map `themes.js` palettes → `ratatui::Style` sets (bg/fg/accent/marker/severity). `T` quick light/dark toggle; palette picker in settings. Apply `theme_defaults` from config on connect.
2. Settings popup (`S`): clear cached session; timestamp mode; theme palette; open current session HTML in browser (`open`/`xdg-open`); reveal session dir in file manager (mirror `reveal_in_file_manager`); quit.
3. Sessions popup (`o`): `GET /api/sessions` → list with current marker + `html_status`; Enter opens `session.html` in browser or reveals dir.
4. Export: `e` → `export_session_html` WS cmd; status bar shows `html_status`. Scope-aware export (`E` exports selection as a trimmed HTML? — browser does snapshot; TUI delegates to server full export + offers raw selection download instead).
5. Rotation: `R` → `POST /api/session/rotate`; handle `session_rotated` (teardown + rebuild).
6. Stats popup (`/`): `GET /api/stats` → ws_clients, replay_depth, per-source counters, totals.
7. Clear logs: `C` → `clear_logs` (active pane; `Shift+C` all).

**Tests (unit):** theme toggle swaps styles; sessions popup parses `/api/sessions` shape; export cmd emits correct payload; rotation triggers layout rebuild.

### Phase 10 — Onboarding (native TUI)
**Files:** `crates/embed-log-tui/src/onboarding.rs`.
**Change:**
1. Native TUI wizard (mirror `onboarding.js`): source picker (list serial ports via `embed_log_core::onboarding::list_serial_ports` + UDP/file templates), tab/pane builder, logs dir, app name → save config.
2. Reuse `embed_log_core::onboarding` helpers where pure (port listing, quick-config serialization). The browser `OnboardingServer` stays for CLI/Tauri; the TUI adds a native path.
3. Triggered when `--tui` resolves no config (mirror CLI/Tauri first-run behavior), or via `embed-log onboard --tui`.

**Acceptance:** with no config, `embed-log run --tui` opens the wizard, saves `embed-log.yml`, then starts the server + TUI.

### Phase 11 — CLI integration
**Files:** `crates/embed-log-cli/src/main.rs`, `crates/embed-log-cli/Cargo.toml`.
**Change:**
1. Top-level `Cli` gains `--tui` (sibling to `--ui`).
2. `Command::Run` and `Command::Demo` honor `--tui`: start `LogServer` headless, call `embed_log_tui::run_in_process(port, app_name)`, then stop server + export session on return.
3. `cmd_tui(config_path)` mirrors `cmd_ui` (which spawns the Tauri binary) — spawns `embed-log-tui` binary or calls the lib in-process.
4. `embed-log-cli` depends on `embed-log-tui` (optional feature `tui` to keep the CLI lean when TUI isn't needed).

**Acceptance:** `embed-log demo --tui` and `embed-log run --tui --config x.yml` work end-to-end; `just demo-tui` renders the demo in the terminal.

### Phase 12 — Demo for TUI
**Files:** `justfile`, `crates/embed-log-cli/src/main.rs` (cmd_demo), `crates/embed-log-tui` (demo entry).
**Change:**
1. `embed-log demo --tui`: reuse `cmd_demo`'s embedded `DEMO_CONFIG` + `prepare_demo_file_sources` + `spawn_demo_traffic` + `LogServer::run` headless, then launch the TUI client on loopback. All demo sources (DUT/HOST UDP, UART UDP, CoAP hex, SENSORS CBOR, NET_CAPTURE mock, FILE_WATCH) render in the TUI.
2. `just demo-tui` recipe (Phase 0 stub → functional here).
3. The same demo traffic generator feeds both browser and TUI demos unchanged.

**Acceptance:** `just demo-tui` shows 6 tabs, live flowing logs, CoAP plugin annotations, events (if `demo.events.yml` present), and TX works on UART panes.

### Phase 13 — Tests
**Files:** `crates/embed-log-tui/src/**` `#[cfg(test)]` modules, `crates/embed-log-tui/tests/`.
**Change:**
1. Unit tests per module (listed in each phase above).
2. Integration test: start a `LogServer` on a temp config (mirror `ws_server.rs`/`server.rs` test helpers), connect the TUI client, push log/event/marker broadcasts, assert `State` updates.
3. `TestBackend` rendering tests: scripted key sequence + injected WS messages → assert `ratatui` buffer (tab labels, pane titles, status bar, marker gutters, events points).
4. Optional pty smoke test (later): drive `embed-log-tui` binary via a pty against the demo server, assert expected lines appear. Not blocking.

**Acceptance:** `cargo test -p embed-log-tui` green; `just tui-unit` recipe.

### Phase 14 — Docs, config samples, release
**Files:** `README.md`, `docs/architecture.md`, `docs/tui.md` (new), `docs/cli.md`, `justfile`, release scripts.
**Change:**
1. `docs/tui.md`: keybindings, modes, plugin support matrix, demo/remote usage.
2. `docs/architecture.md`: add TUI shell to the high-level diagram + crate table.
3. `README.md`: mention `--tui` / `just demo-tui` in quick start.
4. `docs/cli.md`: document `--tui`, `embed-log-tui --url/--config`.
5. Release: add `embed-log-tui` binary to `package-cli` / tarball steps in `scripts/` (one extra binary, no new runner). Update install scripts to place it.
6. Changelog entry.

**Acceptance:** `just verify` (fmt-check, check, clippy, test, ui-unit) green with the new crate; docs accurate.

---

## Keybindings reference (proposed)

| Key | Action |
| --- | --- |
| `?` | Help overlay |
| `Tab` / `Shift+Tab` | Cycle tabs |
| `1`-`9` | Jump to tab N |
| `u` | Toggle unwrap (flatten panes) |
| `t` | Toggle absolute/relative timestamps |
| `T` | Toggle light/dark theme |
| `h`/`l` | Pane focus left/right |
| `H`/`L` | Resize splitter |
| `j`/`k`, `↓`/`↑` | Scroll active pane |
| `g` / `G` | Top / bottom of active pane |
| `J` | Toggle jump-to-bottom |
| `Enter` | Sync panes to this line's timestamp |
| `>` | Clear filter + sync (zoom-out gesture) |
| `f` | Pane regex filter input |
| `v` / `Space` | Visual select / toggle line |
| `c` | Toggle Exact/Context selection scope |
| `y` | Yank selection (clipboard) |
| `d` | Download raw selection (file) |
| `s` | Save snippet |
| `m` | Toggle marker on line |
| `[` / `]` | Marker prev/next |
| `M` | Toggle include event markers in nav |
| `K` | Marker / event / plugin tooltip popup |
| `p` | Plugin info popup |
| `:` / `i` | TX input (writable pane) |
| `e` | Export session HTML |
| `R` | Rotate session |
| `C` / `Shift+C` | Clear active pane / all panes |
| `o` | Sessions list popup |
| `S` | Settings popup |
| `/` | Stats popup |
| `F` | Events filter popup (events tab) |
| `+`/`-`/`<`/`>`/`0` | Events timeline zoom / pan / reset |
| `q` / `Ctrl+C` | Quit (export + shutdown in-process mode) |

## Gaps / explicit deviations from browser UI
- **Font size** has no terminal equivalent → mapped to a line-wrap toggle; documented in `docs/tui.md`.
- **Drag-to-select / splitter drag / pointer hover** → keyboard equivalents (visual mode, resize keys, tooltip popups).
- **Custom JS plugins** cannot run in the TUI → only Rust-reimplemented builtins (hex-coap) annotate; others show a browser-only indicator.
- **SVG timeline** → braille-point `Canvas` (same data, terminal-renderable).
- **Browser open / file-manager reveal** → `open`/`xdg-open` (cross-platform best-effort, mirrors Tauri `reveal_in_file_manager`).

## Verification plan
- `cargo fmt-check --all`, `cargo check --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`.
- `cargo test -p embed-log-tui` (unit + integration + `TestBackend`).
- `just demo-tui` manual smoke: 6 tabs, live logs, CoAP annotations, markers, events, TX, export, rotate.
- `just verify` green end-to-end.
