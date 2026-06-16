# Real-time Event Detection — Implementation Plan

Per-source regex event rules defined in a companion YAML file, matched in the Rust backend, visualized as an interactive timeline in the frontend, with marker reuse for log-line annotations and Python SDK integration.

## Architecture overview

```
embed-log.events.yml  (companion file, resolved like .commands.yml)
  │
  ▼
config::events::load_event_rules() → HashMap<String, Vec<EventRule>>
  │                                     source name → compiled rules
  ▼
runtime::server.rs → builds PatternMatcher per source
  │
  ▼
run_writer() loop — after building WS payload, before broadcast:
  │  for each rule in matcher.check(&message):
  │    1. build event payload
  │    2. append to events.jsonl
  │    3. push to events replay buffer
  │    4. broadcast "type":"event" on broadcast_tx
  │    5. create marker (kind:"event") → broadcast markers_update
  │
  ├─ /ws clients receive "type":"event" + "markers_update"
  │    └─ frontend events.js renders timeline + log-line marker
  │
  └─ /api/v1/control clients receive event via subscribe events:true
       └─ Python SDK client.events() yields Event objects
```

## Data schemas

### Event rule (companion YAML)

```yaml
# embed-log.events.yml
DUT:
  - name: fatal_error
    pattern: "FATAL ERROR"
    severity: error
  - name: boot_complete
    pattern: "boot complete"
    severity: info
HOST:
  - name: test_passed
    pattern: "PASSED"
    severity: info
```

### Rust structs

```rust
struct EventRule {
    name: String,           // unique within source
    pattern: String,        // raw regex string
    severity: String,       // "info" | "warn" | "error" | "fatal"
    regex: regex::Regex,    // compiled at load time
}

struct PatternMatcher {
    rules: Vec<EventRule>,
}

impl PatternMatcher {
    fn check(&self, message: &str) -> Vec<EventMatch>;
}

struct EventMatch {
    rule_name: String,
    severity: String,
    captures: Vec<String>,
}
```

### Event broadcast payload (JSON)

```json
{
  "type": "event",
  "event_id": "fatal_error",
  "source_id": "DUT",
  "severity": "error",
  "timestamp": "06-14 09:30:45.123",
  "timestamp_iso": "2026-06-14T09:30:45.123+02:00",
  "timestamp_num": 1718347845123.0,
  "rel_num": 45123.0,
  "line_idx": 42,
  "message": "ZEPHYR FATAL ERROR: stack overflow",
  "origin": "SERIAL",
  "captures": ["ZEPHYR FATAL ERROR"]
}
```

### Event marker (extends existing marker shape)

```json
{
  "paneId": "DUT",
  "lineIdx": 42,
  "endIdx": 42,
  "numTs": 5000.0,
  "description": "fatal_error: ZEPHYR FATAL ERROR: stack overflow",
  "kind": "event",
  "severity": "error",
  "createdAt": "2026-06-14T09:30:45.123+02:00"
}
```

Existing user markers get implicit `kind: "user"` on read (backward compatible — missing field defaults to `"user"`).

### events.jsonl

One JSON event payload per line in the session directory, appended as events fire.

---

## Phase 1 — Rust: event rules config loader

**Files:**
- `crates/embed-log-core/src/config/events.rs` (new)
- `crates/embed-log-core/src/config/mod.rs` (export)
- `crates/embed-log-core/src/config/models.rs` (EventRule struct)

**Change:**
1. Add `EventRule` and `Severity` structs to `models.rs`.
2. Create `events.rs` mirroring `commands.rs`:
   - `load_event_rules(config_path, configured_sources) -> HashMap<String, Vec<EventRule>>`
   - Same companion-file resolution order: `<stem>.events.yml`, then `embed-log.events.yml` in config dir, then CWD.
   - Compile each `pattern` with `regex::Regex::new()`. Fail on invalid regex (return error, log warning, skip rule).
   - Validate rule names unique within each source.
   - Only load rules for sources that exist in the config. Log warning for unknown source names, skip them.
3. Export from `config/mod.rs`.

**Tests (unit, in `events.rs`):**
- Valid file with multiple sources parses and compiles regexes.
- Invalid regex produces error, rule skipped.
- Duplicate rule name within source produces error.
- Unknown source names are skipped with warning.
- Companion-file resolution order: stem-specific beats embed-log beats CWD.
- Missing file returns empty map (no error).

---

## Phase 2 — Rust: pattern matching + event emission

**Files:**
- `crates/embed-log-core/src/runtime/server.rs`
- `crates/embed-log-core/src/config/mod.rs` (already exports loader)

**Change:**
1. In `LogServer::run()`, after loading command suggestions (~line 105), load event rules:
   ```rust
   let event_rules = crate::config::load_event_rules(
       self.config_path.as_deref(),
       &source_writability,  // actually just source names here
   );
   ```
2. Build `HashMap<String, PatternMatcher>` — one matcher per source that has rules.
3. Pass the matchers into `WriterRuntime` (add field `event_matchers: HashMap<String, PatternMatcher>`).
4. In `run_writer()`, after building the `payload` JSON but before `broadcast_tx.send()`:
   ```rust
   if let Some(matcher) = runtime.event_matchers.get(&source_name) {
       for m in matcher.check(&entry.message) {
           // build event payload
           // append to events.jsonl
           // push to events replay buffer
           // broadcast "type":"event"
           // create marker (kind:"event") via session manager
       }
   }
   ```
5. Event payload construction reuses the same `abs_num`, `rel_num`, `ts_iso`, `line_idx`, `origin` values already computed for the log payload — no duplicate work.
6. Marker creation calls session manager `save_markers()` (same path as `handle_marker_create` in control_ws.rs) and broadcasts `markers_update`.

**Tests (unit, in `server.rs` tests module):**
- Log line matching a rule produces event in broadcast output.
- Log line matching multiple rules produces multiple events.
- Non-matching log line produces no event.
- Event payload contains correct `timestamp_num`, `line_idx`, `source_id`, `captures`.
- Event marker created with `kind: "event"` and correct severity.
- Source without rules produces no events.

**Tests (integration):**
- Full `run_writer` with a matcher: send matching LogEntry, assert event broadcast + marker persisted.

---

## Phase 3 — Rust: events.jsonl persistence + replay buffer

**Files:**
- `crates/embed-log-core/src/session/manager.rs`
- `crates/embed-log-core/src/runtime/server.rs`
- `crates/embed-log-core/src/net/ws_server.rs` (ServerState)

**Change:**
1. Add to `SessionManager`:
   - `append_event(event: &serde_json::Value) -> Result<()>` — appends one JSON line to `events.jsonl`.
   - `load_events() -> Vec<serde_json::Value>` — reads all events from `events.jsonl`.
2. Add `events_replay: Arc<Mutex<VecDeque<String>>>` to `ServerState` (capped at `REPLAY_BUFFER_SIZE = 5000`).
3. In `run_writer`, when an event fires:
   - `session_manager.append_event(&event_payload)`
   - push to `events_replay`
4. On new `/ws` client connect (in `ws_handler`), after sending config + log replay, send replayed events as individual `"type":"event"` messages.
5. Include `events.jsonl` path in the session manifest.

**Tests (unit, in `manager.rs`):**
- `append_event` writes valid JSONL.
- `load_events` reads back all events in order.
- Multiple appends produce correct multi-line JSONL.

**Tests (integration, in `ws_server.rs` tests):**
- New WS client receives replayed events after config message.

---

## Phase 4 — Rust: control API event subscription

**Files:**
- `crates/embed-log-core/src/net/control_ws.rs`

**Change:**
1. Extend `subscribe` command to accept `"events": true`.
2. Add `events_subscribed: bool` to `ControlSubscription`.
3. In the broadcast forward loop, forward `"type":"event"` messages to clients with `events_subscribed: true`.
4. Add `unsubscribe` support for events (`"events": false`).

**Tests (unit, in `control_ws.rs`):**
- Subscribe with `events:true` receives event messages.
- Subscribe without `events:true` does not receive event messages.
- Unsubscribe events stops delivery.
- Events interleave correctly with `log.entry` messages.

---

## Phase 5 — JS frontend: events tab + SVG timeline

**Files:**
- `frontend/events.js` (new)
- `frontend/ws.js` (event message handling)
- `frontend/state.js` (event state)
- `frontend/tabs.js` or `frontend/tabcreate.js` (dynamic events tab)
- `frontend/viewer.css` (timeline styles)
- `frontend/main.js` (import)

**Change:**
1. `state.js`: add `events: []`, `eventLanes: new Map()` (event_id → lane index).
2. `ws.js`:
   - Config message handler: if `msg.event_rules` is non-empty and has at least one definition, set `state.eventsEnabled = true`.
   - New message handler for `msg.type === "event"`: push to `state.events`, call `events.js` render.
   - New message handler for `msg.type === "events_replay"`: bulk-load historical events.
3. `events.js` (new module):
   - `initEventsTab()` — creates an "Events" tab dynamically when `state.eventsEnabled`.
   - `renderTimeline()` — SVG-based timeline:
     - X axis: time (`timestamp_num`), auto-ranging from `first_log_at` to latest event.
     - Y axis: one swimlane per unique `event_id`, labeled.
     - Each event = `<circle>` or `<rect>` positioned at `(time, lane)`, colored by severity.
   - `onEventHover(event)` — tooltip with full message + captures.
   - `onEventClick(event)` — calls existing sync:
     ```js
     state.syncTs = event.timestamp_num;
     state.syncTabSwitch = true;
     // switch to the tab containing event.source_id pane
     // scrollPaneToTs(sourcePaneId, event.timestamp_num);
     ```
   - Timeline controls: zoom in/out buttons, drag-to-pan, filter by source and severity (checkbox legend).
4. `viewer.css`: timeline container, lane styling, severity colors (info=blue, warn=yellow, error=red, fatal=dark-red), tooltip.

**Tests (Playwright/UI):**
- Config with event rules creates Events tab; config without does not.
- Event message received renders a dot on the timeline at correct position.
- Click event dot syncs log panes to event timestamp.
- Hover event dot shows tooltip with message.
- Zoom controls adjust time range.
- Severity filter toggles dot visibility.

---

## Phase 6 — JS frontend: event marker rendering

**Files:**
- `frontend/selection.js` (marker rendering)
- `frontend/lines.js` (line marker class)
- `frontend/viewer.css`

**Change:**
1. `lines.js`: in `_markerDescription()` and line rendering, check marker `kind`:
   - `kind: "event"` → apply severity-based left border color instead of default accent.
   - `kind: "user"` (or missing) → existing behavior.
2. `selection.js`: marker navigation (prev/next) skips `kind: "event"` markers by default. Add a toggle button to include event markers in navigation.
3. `viewer.css`: `.log-line.has-marker[data-kind="event"]` severity-colored borders.

**Tests (Playwright/UI):**
- Event marker renders with severity color on the log line.
- User marker navigation skips event markers.
- Toggle includes event markers in navigation.

---

## Phase 7 — Python SDK: event subscription + watcher integration

**Files:**
- `sdk/python/embed_log_sdk/client.py`
- `sdk/python/embed_log_sdk/models.py`
- `sdk/python/embed_log_sdk/watcher.py`
- `sdk/python/tests/test_events.py` (new)

**Change:**
1. `models.py`: add `Event` dataclass (`event_id`, `source_id`, `severity`, `timestamp_num`, `rel_num`, `line_idx`, `message`, `captures`).
2. `client.py`:
   - `subscribe(sources=None, events=False)` — extend existing subscribe to accept `events` flag.
   - `events()` — generator yielding `Event` objects from the control WS event stream.
   - `unsubscribe_events()`.
3. `watcher.py`: unchanged client-side matching. Add note in docstring that backend events are available via `client.subscribe(events=True)` for rules defined in `.events.yml`. The two approaches coexist:
   - Backend events: static rules from config, subscribe with `events=True`.
   - Watcher: runtime-defined rules, client-side matching on `log.entry`.

**Tests (unit):**
- `subscribe(events=True)` sends correct command.
- `events()` yields parsed `Event` objects from mock WS messages.
- Event and log.entry messages interleave without loss.
- Watcher still works independently of event subscription.

---

## Phase 8 — Demo integration

**Files:**
- `crates/embed-log-core/src/demo.rs`
- `demo.yml`
- `demo.events.yml` (new)

**Change:**
1. Create `demo.events.yml` with 5 events matching demo traffic:
   ```yaml
   DUT:
     - name: boot_complete
       pattern: "boot complete"
       severity: info
     - name: wifi_connected
       pattern: "WiFi connected"
       severity: info
     - name: mqtt_publish
       pattern: "MQTT publish"
       severity: info
     - name: watchdog_ok
       pattern: "watchdog fed"
       severity: info
   UART_DUT:
     - name: bootloader_banner
       pattern: "ROM bootloader"
       severity: warn
   ```

2. Rewrite demo log lines in `demo.rs` to ensure events fire at predictable intervals:
   - `demo_device_line`: keep existing patterns (they already contain "boot complete", "WiFi connected", "MQTT publish", "watchdog fed" — they match the events).
   - `demo_uart_main_line`: tick % 5 == 0 already emits "ROM bootloader banner" — matches `bootloader_banner`.
   - Verify each event fires at least once per demo cycle.

3. The demo already uses UDP sources for DUT and UART_DUT, so events will fire in real time as demo traffic flows.

**Tests (integration):**
- Run demo server, connect WS client, receive at least one of each defined event within one demo cycle.
- Verify `events.jsonl` is created in session directory with matching events.

---

## Phase 9 — Static export: events in session.html

**Files:**
- `crates/embed-log-core/src/session/exporter.rs`
- `frontend/export.js`
- `frontend/events.js`

**Change:**
1. `exporter.rs`:
   - Add `events: Vec<serde_json::Value>` field to `SessionExporter`.
   - Add `with_events(events)` builder method (mirrors `with_markers`).
   - Load events from `events.jsonl` in the session directory during export.
   - Serialize events into the HTML bootstrap script: `window.__embedLogEvents = [...]`.
   - Include `events.jsonl` in the freshness check (`session_html_is_current`).
2. `export.js`: bootstrap events from `window.__embedLogEvents` into `state.events` before rendering.
3. `events.js`: timeline renders from `state.events` regardless of live vs static mode.
4. Wire event_rules into config message so static export knows the event definitions (for lane labels and severity colors).

**Tests (unit, in `exporter.rs`):**
- Export with events produces HTML containing event data in bootstrap script.
- Events without markers still export correctly.
- `events.jsonl` freshness triggers re-export.

---

## Phase 10 — End-to-end tests

**Files:**
- `sdk/python/tests/test_events_e2e.py` (new)
- `tests-ui/parity-tests/events.spec.js` (new)

**Python E2E tests:**
- Start server with config + events.yml companion file.
- Send UDP log lines matching defined events.
- Assert events received via `client.subscribe(events=True)`.
- Assert `events.jsonl` contains matching events.
- Assert event markers appear in `markers.json` with `kind: "event"`.
- Assert Python watcher client-side matching works alongside backend events.

**Playwright UI tests:**
- Start demo server.
- Events tab appears and populates as demo traffic flows.
- Click event dot syncs log panes.
- Event markers visible on log lines with severity colors.
- Zoom and filter controls work.
- Static HTML export includes events timeline.

---

## Dependency order

```
Phase 1 (config loader)
  ↓
Phase 2 (matching + emission)    ← depends on Phase 1
  ↓
Phase 3 (persistence + replay)   ← depends on Phase 2
  ↓
Phase 4 (control API)            ← depends on Phase 2 (event payload shape)
  ↓
Phase 5 (frontend timeline)      ← depends on Phase 2 (event broadcast)
Phase 6 (frontend markers)       ← depends on Phase 2 (marker broadcast)
  ↓ (both can parallel after Phase 2)
Phase 7 (Python SDK)             ← depends on Phase 4 (control API)
  ↓
Phase 8 (demo)                   ← depends on Phase 2
Phase 9 (static export)          ← depends on Phase 3, 5
  ↓
Phase 10 (E2E)                   ← depends on all
```

Phases 5+6 can run in parallel. Phase 7 can run in parallel with 5+6 once Phase 4 is done. Phase 8 can start after Phase 2.

## Verification checklist (per phase)

| Phase | Build | Tests |
|---|---|---|
| 1 | `cargo check -p embed-log-core` | `cargo test -p embed-log-core config::events` |
| 2 | `cargo check -p embed-log-core` | `cargo test -p embed-log-core runtime::server -- events` |
| 3 | `cargo check -p embed-log-core` | `cargo test -p embed-log-core session::manager -- events` |
| 4 | `cargo check -p embed-log-core` | `cargo test -p embed-log-core net::control_ws -- events` |
| 5 | `just build` | Playwright events.spec.js |
| 6 | `just build` | Playwright marker rendering tests |
| 7 | `python -m compileall sdk/` | `pytest sdk/python/tests/test_events.py -q` |
| 8 | `just build` | Integration: demo server + WS event check |
| 9 | `cargo check -p embed-log-core` | `cargo test -p embed-log-core session::exporter -- events` |
| 10 | all | `pytest sdk/python/tests/test_events_e2e.py -q` + Playwright |

**Cross-phase gates (run at end):**
- `just fmt-check`
- `cargo clippy -p embed-log-core --all-targets -- -D warnings`
- `cargo check --target x86_64-pc-windows-msvc`
- `just demo` — visual confirmation of events tab + timeline + markers
