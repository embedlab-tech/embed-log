# Embed-log product backlog

This document captures planned product directions rather than committed release dates. The features share one pipeline:

```text
UART / UDP / file bytes
  -> built-in or user decoder
  -> canonical ordered log record
  -> runtime analysis (watch rules, later measurement extraction)
  -> session artifacts (`combined.jsonl`, `events.jsonl`, later measurements)
  -> browser / exported HTML / terminal views
```

The canonical log record and its session-global `sequence` remain the source of truth. Derived views must point back to that record instead of creating duplicate log records or consuming new sequence numbers.

## Current baseline

- `embed-log watch` already means a temporary, process-local, one-shot condition with a TTL. It matches future committed RX records and is never persisted.
- Every committed record is ordered in `combined.jsonl` by a single runtime commit lock.
- Backend parsers implement the internal `StreamParser` trait and are selected by a static Rust match in `create_parser`.
- The browser has a compatibility-only JavaScript line-plugin runtime, but config v2 does not expose it as a supported general decoder mechanism.
- A no-config UART quick run already exists: `embed-log run /dev/ttyUSB0 --baud 115200`. `--port` currently means the HTTP/WebSocket server port, not a serial device.
- The frontend currently has no chart library dependency and exported HTML is designed to remain self-contained and usable offline.

---

## 1. Configured runtime watcher and event view — priority

### Goal

Allow users to define interesting log lines or words in the normal YAML config. When explicitly enabled, Embed-log evaluates those rules as records are committed, saves sparse matches to a per-session `events.jsonl`, and presents all matched events in a merged **Events** tab in the browser and exported HTML.

This is separate from temporary `embed-log watch`:

- **temporary watch**: created through the CLI/control API, waits for one future condition, retained only in memory;
- **configured watcher**: declared in YAML, active for the run, records occurrences durably;
- **event**: one configured rule matching one canonical log record.

The temporary watch API must keep its existing behavior and must not create persistent events.

### Proposed configuration

Use an extensible `analysis` section so later measurement extraction can share validation and matching infrastructure:

```yaml
version: 2

server:
  listen: 127.0.0.1:18080
  job_id: hardware-nightly-42

logs:
  dir: captures/

sources:
  DUT:
    type: uart
    path: /dev/ttyUSB0
    baud: 115200
  HOST:
    type: file
    path: pytest.log

analysis:
  watcher:
    enabled: true
    tab:
      enabled: true
      title: Events
    rules:
      hard-fault:
        sources: [DUT]
        contains: "HardFault"
        case_sensitive: true
        severity: error
        tags: [firmware, crash]

      watchdog-reset:
        sources: [DUT, HOST]
        regex: '(?i)\\bwatchdog.*reset\\b'
        severity: warning
        tags: [reset]
```

Initial rules should accept exactly one matcher:

- `contains`: literal substring;
- `regex`: Rust regular expression. Exact lines and words can be expressed with anchors and word boundaries.

True similarity-based fuzzy matching is deferred until real examples define the expected normalization, scoring, threshold, and false-positive behavior. Regex and literal matching provide deterministic CI results for the first version.

Recommended defaults and validation:

- The watcher is off unless `analysis.watcher.enabled: true`.
- Omitted `sources` means all physical RX sources.
- A configured virtual merge may be accepted and expanded to its physical members.
- TX records do not match unless a later explicit option adds that behavior.
- Matching uses the canonical decoded `message`, not ANSI-rendered browser text.
- Rule IDs are unique, stable identifiers from the YAML mapping keys.
- Invalid regexes, empty matchers, unknown sources, and invalid severities fail config validation before sources start.
- Set documented limits for rule count and pattern length to protect the runtime hot path.

### Runtime behavior

The detector engine should compile all rules once at startup and index them by source. For every committed record:

1. Acquire the existing global commit lock.
2. Append the canonical record to `combined.jsonl` and assign its `sequence`.
3. Evaluate applicable configured rules.
4. Append one event per matching rule to `events.jsonl`.
5. Attach compact event metadata to the live/replay payload.
6. Continue temporary-watch matching, replay storage, and WebSocket publication.

Running detection after canonical persistence gives every event a valid location. Keeping it inside the existing commit section preserves deterministic order across concurrent sources.

A line matching two rules produces two events. Event order is canonical record sequence, then stable rule ID. The deduplication identity is `(session_id, sequence, rule_id)`.

### Session artifact

When the configured watcher is enabled, create `events.jsonl` at session creation even when there are no matches. An empty file means “watcher ran and found nothing”; a missing file means it was disabled or artifact creation failed.

Example event:

```json
{"schema_version":1,"rule_id":"hard-fault","rule_revision":"sha256:...","severity":"error","tags":["firmware","crash"],"session_id":"2026-09-08_19-30-00_hardware-nightly-42","job_id":"hardware-nightly-42","sequence":719,"source_id":"DUT","line_idx":428,"timestamp_iso":"2026-09-08T19:31:04.123+00:00","message":"fatal: HardFault in network thread","match_kind":"contains","captures":[]}
```

`manifest.json` should advertise:

- `events_file` using a portable session-relative path;
- `events_schema_version`;
- the normalized rule definitions or stable fingerprints;
- emitted counts and any write/degradation status available at session close.

The event includes the matched message for convenient CI inspection but not surrounding context. The canonical context remains available by `session_id` and `sequence` through `embed-log sessions around`.

### Live and exported Events tab

The Events tab is a virtual projection, similar in spirit to merged sources:

- It combines matches from all selected physical sources.
- It does not create duplicate entries in `combined.jsonl` or consume sequences.
- A row shows timestamp, rule, severity, physical source, and message.
- Severity and tags can drive styling and filters.
- Selecting an event should eventually jump to or open context around the original source record.

For live and replayed clients, add the matching rule summaries to the existing record payload so the frontend can route the same record into both its physical pane and the Events view. For static HTML, the exporter joins `events.jsonl` to `combined.jsonl` by sequence. Saved sessions therefore reproduce the same Events tab without executing detection again.

Event artifact and frontend failures must not silently stop raw log capture. Surface matcher/write failures in runtime statistics and logs, and make offline readers report missing, malformed, or truncated event data explicitly.

### CLI and CI follow-up

Add an offline sparse reader rather than requiring every query to rescan `combined.jsonl`:

```bash
embed-log events search --dir captures
embed-log events search --dir captures --rule hard-fault
embed-log events search --dir captures --severity error --job hardware-nightly-42
embed-log events search --dir captures --count
```

Add a runtime job identity override suitable for CI:

```bash
embed-log run --config embed-log.yml \
  --job-id "$GITHUB_RUN_ID:$GITHUB_JOB:$GITHUB_RUN_ATTEMPT"
```

Proposed precedence: `--job-id`, then `EMBED_LOG_JOB_ID`, then `server.job_id`.

Detection is observational by default and does not fail a job. A later explicit `embed-log events check --severity error` command may provide opt-in gating.

CI should upload `manifest.json`, `events.jsonl`, and the corresponding canonical logs with `if: always()`. Multiple jobs can download their session directories under one logs root and query the sparse event files together.

### Implementation slices

1. Add config models, strict validation, serialization, normalized rule fingerprints, and matcher unit tests.
2. Add the detector engine and per-session event writer at the ordered runtime commit point.
3. Handle session rotation, empty event-file creation, manifest metadata, counters, and failure reporting.
4. Add live/replay event metadata and the virtual browser Events tab.
5. Include the Events tab in self-contained HTML exports.
6. Add `events search`, machine-readable schema descriptions, stable errors, and multi-session integration tests.
7. Document CI identity, artifact upload, aggregation, and context lookup.

### Acceptance criteria

- Enabled watcher plus a matching RX line writes one valid event pointing to the same session and sequence in `combined.jsonl`.
- Enabled watcher with no matches leaves a valid empty `events.jsonl`.
- Disabled watcher does not evaluate rules or create the artifact.
- Concurrent source matches remain in global sequence order.
- Session rotation starts a new event file without leaking prior state.
- Live, replayed, and exported Events tabs contain the same matched records.
- A late browser client receives event metadata through replay without rerunning rules.
- Invalid rules fail before source capture starts.
- Existing temporary watch add/wait/remove behavior remains unchanged and non-persistent.
- Offline event searches work across multiple sessions and job IDs.

---

## 2. Native time-series charts — placeholder

### Goal

Allow users to extract numeric measurements from selected canonical log records and display them as native time-series charts in the browser and exported HTML.

Illustrative future configuration, not a committed schema:

```yaml
analysis:
  series:
    battery-voltage:
      source: DUT
      regex: 'battery voltage=(?<value>-?[0-9]+(?:\\.[0-9]+)?) V'
      value_capture: value
      unit: V
      chart: Power
```

### Direction

- Reuse the watcher’s compiled source filtering and regex/capture infrastructure, but emit typed measurement points rather than ordinary events.
- Every point should carry source, session, original record sequence, timestamp, series ID, numeric value, and unit.
- Persist derived points separately, likely as `measurements.jsonl`, so charts do not need to reparse all raw logs and historical exports remain reproducible.
- Define missing-value, invalid-number, duplicate-timestamp, unit, scaling, and multi-axis behavior before implementation.
- Bound live memory and rendering cost through a rolling viewport and downsampling while retaining the canonical persisted points.
- Support multiple series in one chart and navigation from a point back to its originating log record.

There is no existing chart dependency. Before implementation, compare a small time-series-focused library such as uPlot with Chart.js and Apache ECharts. Any chosen library must be vendored/embedded with a compatible license—never loaded from a CDN—because release binaries and exported HTML must work offline and remain self-contained.

This feature needs a separate design document covering the measurement schema, chart configuration, downsampling, export size, accessibility, and performance tests.

---

## 3. User-supplied Rust decoders

### Goal

Let organizations maintain proprietary decoders outside the Embed-log repository and attach them to UART, UDP, or file sources without upstreaming company-specific code.

Current built-in parsing is statically linked: `StreamParser::feed(&mut self, &[u8]) -> Vec<String>` and `create_parser` selects `text`, `hex-coap`, `slip-coap`, or `zephyr-dict`. A plugin system must preserve streaming state and run before canonical records, watchers, measurements, persistence, and every frontend.

### Proposed configuration direction

Illustrative only:

```yaml
decoders:
  company-wire-v1:
    plugin: ./plugins/company-wire-v1.wasm
    sha256: "..."
    options:
      channel: diagnostic

sources:
  DUT:
    type: uart
    path: /dev/ttyUSB0
    baud: 115200
    parser:
      type: plugin
      decoder: company-wire-v1
```

Relative plugin paths should resolve against the config directory. `doctor` should verify existence, checksum, API compatibility, and source compatibility without opening the UART.

### Required design spike

Rust does not provide a stable native dynamic-library ABI. Do not expose `Box<dyn StreamParser>` directly across a downloaded `.so`, `.dylib`, or `.dll` boundary.

Evaluate these approaches before selecting one:

1. **Wasm component/plugin with a Rust SDK** — preferred starting point for versioning, cross-platform packaging, isolation, and company-owned Rust source.
2. **Versioned C ABI or `abi_stable` dynamic library** — lower runtime overhead but platform-specific packaging and a larger unsafe compatibility surface.
3. **Out-of-process decoder protocol** — strongest process isolation and language independence, but requires framing, lifecycle, and restart handling.
4. **Compile-time Cargo plugin** — simplest Rust integration but requires companies to maintain a custom Embed-log build, so it does not fully meet the deployment goal.

### Decoder API requirements

The eventual versioned API needs more than the current minimal trait:

- create an instance with validated JSON-like options;
- feed arbitrary byte chunks while retaining state;
- return zero or more decoded records;
- flush/end-of-stream and reset on source reconnect or file replacement;
- return a message plus optional structured metadata for watchers and future charts;
- distinguish incomplete input, malformed data, recoverable warnings, and fatal decoder failure;
- declare API version, decoder identity/version, supported source kinds, and resource limits;
- isolate plugin panic/trap/failure so raw capture and other sources remain observable;
- define whether and how undecoded raw bytes are retained for diagnostics.

Persist decoder identity, version, checksum, and normalized options in the session manifest. Exported HTML contains decoded records and metadata only; it must not execute backend decoder plugins.

### Security and operations

- Treat decoder plugins as trusted local configuration unless a sandboxed model is explicitly guaranteed.
- Require explicit paths; do not scan arbitrary directories or download plugins automatically.
- Provide deterministic checksum/version diagnostics.
- Bound memory, output records per input chunk, and execution time where the selected runtime permits it.
- Document cross-platform packaging and failure behavior.

### Acceptance criteria

- A separately built Rust decoder can be referenced from config and used by all supported source types it declares.
- Its output follows the same persistence, watch, chart, browser, TUI, and export path as built-ins.
- Partial chunks and reconnect/reset behavior are deterministic.
- Bad version/checksum/configuration fails before capture; a runtime plugin failure is visible and isolated.
- Built-in parser behavior and configurations remain backward compatible.

---

## 4. Simple serial-terminal mode

### Goal

Offer a low-ceremony, single-port experience close to picocom or minicom while retaining Embed-log capture, timestamps, TX ownership, optional decoding, and saved sessions.

The UART quick-run source already exists. This backlog item is therefore a simplified terminal interaction profile, not a second serial backend.

### Proposed command

Prefer an unambiguous subcommand or the existing positional source syntax:

```bash
embed-log terminal /dev/ttyUSB0 --baud 115200

# Possible compatibility form
embed-log run /dev/ttyUSB0 --baud 115200 --terminal
```

Do not use `embed-log --port /dev/ttyUSB0`: `--port` already means the HTTP/WebSocket server port. `--serial /dev/ttyUSB0` can remain the explicit spelling where needed.

Future custom decoder selection should use the same registry as config-based runs:

```bash
embed-log terminal /dev/ttyUSB0 --baud 115200 \
  --decoder company-wire-v1 \
  --decoder-plugin ./plugins/company-wire-v1.wasm
```

Exact flags depend on the decoder-plugin design and should not create a terminal-only plugin mechanism.

### Experience

- One full-screen scrolling pane, minimal chrome, immediate keyboard-to-UART TX.
- Clear quit/control key that does not collide with ordinary device input.
- Configurable CR, LF, CRLF, or raw send behavior; optional local echo.
- Visible connection state, RX/TX counters, and decoder errors without a multi-tab dashboard.
- Preserve RX and TX in the normal session artifacts.
- Optional timestamps, hex view, follow/pause, search, copy, and reconnect behavior.
- Work without opening a browser. A local server may remain an internal implementation detail so there is still one source owner and one canonical persistence path.
- Accept either direct CLI arguments or `--config` for advanced decoder/watch settings, but keep the common one-port command short.

Prefer reusing the existing Rust TUI client and backend with a single-pane “terminal profile” over implementing another serial read/write loop. This prevents different behavior between terminal mode, browser mode, watches, decoders, and saved sessions.

### Acceptance criteria

- One command opens a UART and reaches an interactive terminal without YAML or a browser.
- Typed bytes and configured line endings are transmitted reliably; RX/TX are visibly distinct and persisted.
- Exit restores the host terminal and shuts down the source cleanly.
- Built-in and user-supplied decoders use the same configuration and runtime path as normal runs.
- The existing quick-run and full TUI modes remain compatible.

---

## 5. Independent horizontal scrolling for frontend panes

### Goal

Allow every browser log pane to scroll horizontally when word wrapping is disabled, so long lines remain readable without changing the width or position of neighboring panes.

The frontend already has a per-pane Wrap toggle and renders each pane in its own `.log-area`, but `frontend/viewer.css` currently sets `overflow-x: hidden`. This improvement should extend the existing pane behavior rather than add one shared scrollbar for an entire tab.

### Expected behavior

- Each pane owns an independent horizontal scrollbar and `scrollLeft` position.
- No-wrap mode preserves each line on one row and exposes its complete width through horizontal scrolling.
- Wrap mode continues wrapping within the pane and does not leave a misleading horizontal overflow area.
- Vertical scrolling, follow-tail behavior, filtering, selection, markers, synchronized navigation, and virtualized rendering continue to work.
- Appending records or jumping to the bottom must not unexpectedly reset the user's horizontal position.
- One long line must not resize the pane, tab, toolbar, or neighboring pane.
- Live browser sessions and self-contained exported HTML behave the same way.
- Trackpad/touch horizontal gestures and the native scrollbar should work without custom gesture interception. Keyboard support may use the browser's native behavior initially.

### Implementation notes

- Change the pane-local overflow policy rather than applying horizontal overflow to `.pane-body` or the whole page.
- Verify the absolute positioning used by `.log-window` and virtualized `.log-line` elements still contributes the correct horizontal scroll width; add an explicit content/minimum width wrapper if native overflow alone is insufficient.
- Decide whether toggling Wrap resets horizontal position to zero or preserves and restores the prior no-wrap position. Preserving a per-pane position is preferable if it remains predictable.
- Keep horizontal positions independent; synchronized timestamp navigation should not synchronize horizontal scrolling.

### Acceptance criteria

- A line wider than its pane can be scrolled to its final character in no-wrap mode.
- Two panes in one tab can hold different horizontal positions.
- Scrolling or appending vertically does not reset horizontal position.
- Enabling Wrap removes the need for horizontal scrolling without breaking row-height virtualization.
- Filtering, selecting, copying, marking, and jumping to a record still target the correct line after horizontal scrolling.
- The behavior passes focused frontend tests in both live and exported-session layouts.

---

## Suggested delivery order

1. Implement configured watcher persistence and the Events tab.
2. Perform the decoder ABI/runtime design spike and select a safe plugin boundary.
3. Add the simple terminal profile, initially with built-in parsers and then the selected plugin mechanism.
4. Design and implement measurement extraction and native charts after the watcher analysis pipeline is stable.
5. Horizontal pane scrolling is a small independent frontend improvement and can ship in any release without waiting for the larger pipeline work.

Charts are listed as the second product idea but intentionally remain a placeholder. Their implementation should follow the shared analysis/event foundations rather than introducing an unrelated frontend-only parser.

## Cross-feature principles

- Decode once in the backend; browser, TUI, watcher, charts, CLI, and exports consume the same canonical result.
- Persist raw/canonical evidence before derived watcher or measurement artifacts.
- Derived records always retain `session_id`, global `sequence`, physical source, source-local index, and timestamp.
- Keep offline and self-contained operation: no required CDN, cloud service, or runtime download.
- Preserve session rotation and global ordering semantics.
- Keep proprietary decoder code and configuration outside the Embed-log repository.
- Validate configuration and plugin compatibility before opening hardware whenever possible.
- Make failures visible without silently discarding the primary log stream.
- Extend `embed-log schema`, docs, unit tests, integration tests, and export tests with every public contract.
