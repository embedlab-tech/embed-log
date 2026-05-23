# merge_logs.py — offline log viewer

Generates a self-contained static HTML file from one or more `.log` files
produced by the embed-log server. The output can be opened directly in any
browser — no server, no dependencies, no build step.

The viewer is identical to the live browser UI: same themes, ANSI colour
rendering, per-pane regex filter, timestamp sync across panes and tabs, and
HTML export.

---

## Quick start

```bash
# Two UART logs in one tab
python3 utils/merge_logs.py \
    --tab "UART" "Device A" logs/DEVICE_A.log \
                 "Device B" logs/DEVICE_B.log

# Two UART logs + a pytest log in a separate tab
python3 utils/merge_logs.py \
    --tab "UART"   "Device A" logs/DEVICE_A.log \
                   "Device B" logs/DEVICE_B.log \
    --tab "PYTEST" "Pytest"             logs/pytest.log \
    --output run-42.html
```

Open the output file in a browser. No internet connection required (the
JetBrains Mono font is the only external resource; the viewer falls back to
system monospace if offline).

---

## CLI reference

```
python utils/merge_logs.py --tab TAB_LABEL PANE_LABEL FILE [PANE_LABEL FILE]
                            [--tab ...]
                            [--output FILE]
```

### `--tab`

Defines one tab. Repeat for multiple tabs.

```
TAB_LABEL   Label shown on the tab button, e.g. "UART" or "PYTEST"
PANE_LABEL  Display name shown in the pane header
FILE        Path to the .log file
```

Each tab holds **1 or 2 panes**. Two panes are shown side-by-side with a
draggable splitter between them.

| Scenario | `--tab` arguments |
|---|---|
| Single pane | `--tab "PYTEST" "Pytest" logs/pytest.log` |
| Two panes | `--tab "UART" "Device A" device-a.log "Device B" device-b.log` |

Pane labels must be unique across all tabs (they are used as HTML element IDs).

### `--output`

Output file path. Defaults to `merged.html`.

---

## Tabs and synchronisation

When the file contains more than one tab a tab bar appears at the top.

**Within a tab** clicking a line highlights it and scrolls the other pane in
the same tab to the nearest matching timestamp, mirroring the clicked line's
vertical position.

**Across tabs** the last-clicked timestamp is remembered globally. Switching
to another tab automatically scrolls all panes in that tab to the line closest
to that timestamp and highlights it. This lets you correlate a UART event with
a pytest step without having to scroll manually.

The **Sync** button in the toolbar enables / disables both within-tab and
cross-tab synchronisation.

---

## Log format

`merge_logs.py` parses the ISO 8601 timestamped lines written by `server.py`:

```
[2026-03-25T11:50:09.900+01:00] free: 62832, used: 93976
[2026-03-25T11:49:59.870+01:00] [demo] sending 'heap stat' command (cycle #1)
[2026-03-25T11:49:59.872+01:00] [TX::demo] heap stat
```

Lines that do not start with a bracketed ISO 8601 timestamp are silently
skipped (blank lines, partial writes, rotation artefacts).

TX lines (`[TX::<source>]`) are rendered at reduced opacity, matching the live
UI.

---

## CI usage example

```yaml
# .gitlab-ci.yml / GitHub Actions

- name: Merge logs
  if: always()
  run: |
    python utils/merge_logs.py \
      --tab "UART"   "Device A" $CI_PROJECT_DIR/logs/DEVICE_A.log \
                     "Device B" $CI_PROJECT_DIR/logs/DEVICE_B.log \
      --tab "PYTEST" "Pytest"             $CI_PROJECT_DIR/logs/pytest.log \
      --output $CI_PROJECT_DIR/logs/merged-$CI_JOB_ID.html

- name: Upload log viewer
  if: always()
  artifacts:
    paths:
      - logs/merged-*.html
```

---

## Assets

`utils/merge_logs.py` reads the following files from `frontend/` and inlines
them into the output HTML:

```
frontend/viewer.css   styles and themes
frontend/state.js     shared state and TABS / PANES constants
frontend/ansi.js      ANSI escape sequence parser
frontend/lines.js     line rendering and sync logic
frontend/tabs.js      tab bar and tab switching
frontend/ui.js        toolbar controls, filter, splitter
frontend/export.js    in-browser HTML export
```

`ws.js` is intentionally omitted — there is no WebSocket connection in static
mode. A no-op `wsSend()` stub is injected instead.
