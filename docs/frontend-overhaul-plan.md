# Frontend overhaul plan

## Status and scope

This is the authoritative plan for the next web-frontend reliability and simplification pass. It covers the live browser viewer, full-session static HTML, and selection-only HTML where explicitly stated. It does not change persisted record schemas, virtual-merge semantics, the TUI, or compact CLI output.

The plan is based on these product decisions:

- Microsoft Edge must support normal keyboard input in live controls.
- Live and exported HTML should share one stable visual structure.
- Browser selection copy always uses Full formatting.
- The selection-bar **Add note** action is removed, while marker display/navigation and backend APIs remain.
- The timestamp button gains a presentation-only **No time** mode.
- No time removes timestamps from clipboard selection text only; HTML and persisted artifacts always retain timestamp data.
- Full-session browser export continues to download the exact canonical HTML atomically published by the Rust exporter.

## 1. Edge keyboard and focus reliability

### Problem

In Microsoft Edge, controls can be clicked but keyboard input does not work reliably in:

- the Serial TX input;
- the Filter regex input;
- Enter-to-send from Serial TX.

Serial TX is a live-view capability. It must remain absent from offline exported HTML. Filtering must work in both modes.

### Investigation

Audit:

- global `keydown` and `keyup` handlers;
- calls to `preventDefault()` and `stopPropagation()`;
- focus changes caused by live rerendering;
- input elements being replaced while records arrive;
- invisible overlays and `pointer-events`;
- disabled/read-only state;
- paste, autocomplete, IME, and composition events.

Global shortcuts must ignore editable targets unless the event belongs to that input:

```js
function isEditableTarget(target) {
    return target instanceof HTMLInputElement
        || target instanceof HTMLTextAreaElement
        || target?.isContentEditable;
}
```

### Acceptance criteria

In the live viewer:

- Filter accepts typing, paste, and correction of an invalid regex.
- Serial TX accepts typing and paste.
- Enter in Serial TX sends exactly once.
- Inputs retain focus while records arrive.
- Global shortcuts do not fire while typing.
- IME/composition input is not interrupted.

In exported HTML:

- Filter accepts keyboard input.
- Serial TX is not shown.

## 2. Shared live and exported layout

### Problem

The exported report is visually close to the live viewer, but toolbar controls—especially Options/Settings—can wrap onto another line and break alignment.

### Direction

Live and static reports should construct shared visual elements from the same frontend components and capability model rather than maintain divergent toolbar markup.

Capability differences remain intentional:

| Capability | Live | Static HTML |
| --- | --- | --- |
| Filter | Yes | Yes |
| Settings/theme/timestamps | Yes | Yes |
| Serial TX | Yes, for writable sources | No |
| WebSocket status | Yes | No |
| Full-session export trigger | Yes | No |
| Marker display/navigation | Yes | Yes |

### Layout requirements

- Buttons do not wrap internally.
- Options/Settings remains in its intended toolbar group.
- Omitted controls do not leave gaps or orphan another button.
- Toolbar groups shrink predictably.
- Narrow layouts use controlled wrapping or overflow.
- Long application/session names do not push controls out of view.
- Live and static reports use the same spacing, typography, button dimensions, tabs, and pane headers.

### Acceptance criteria

- Options/Settings does not unexpectedly occupy a line by itself.
- Layout remains usable in Edge and Chromium at standard and narrow viewport sizes.
- Static capability differences do not distort the shared toolbar.

## 3. Simplify selected-log actions

### Always copy Full

Remove browser selection-copy choices for Full, Compact, and any other format cycle/menu. Selection copy immediately uses the existing Full representation.

Remove obsolete browser-only code for:

- copy-format state and persistence;
- format buttons and menus;
- format keyboard shortcuts;
- format-specific token estimates/status text;
- compact-selection serialization branches no longer used elsewhere.

This does not remove compact or mini formats from the recorded-session CLI.

### Remove Add note

Remove **Add note** from the selection action bar and delete its selection-specific handler and styling.

Retain:

- marker display;
- marker navigation;
- marker persistence;
- marker APIs;
- non-selection marker behavior.

### Preserve selection scopes

The existing evidence scopes remain:

- Exact;
- Range;
- Context;
- Selected panes.

Only clipboard serialization choices are simplified.

### Acceptance criteria

- No selection copy-format control is visible.
- No Add note selection action is visible.
- Every scope copies Full output directly.
- Raw and HTML download actions remain separate from clipboard copy.
- Existing marker display/navigation still works.

## 4. Add No time mode

### State and toolbar

Extend the timestamp button to cycle:

```text
Absolute → Relative → No time → Absolute
```

Use explicit internal state:

```js
"absolute" | "relative" | "hidden"
```

Suggested visible labels are `Absolute`, `Relative`, and `No time`.

### Viewer behavior

When `hidden` is active:

- timestamp elements are hidden;
- panes reclaim timestamp-column width;
- filtering and selection continue normally;
- internal ordering and cross-pane synchronization may still use timestamps;
- switching back restores timestamps without reloading.

### Clipboard behavior

Selection copy remains Full, with one presentation exception:

```text
Absolute/Relative: timestamp + source + message
No time:           source + message
```

Timestamp omission applies to Exact, Range, Context, Selected panes, and clipboard-buffer copy if that feature remains.

### Artifact rule

No time never removes timestamp data from:

- `combined.jsonl`;
- physical source `.log` files;
- full-session HTML;
- selection HTML;
- raw downloads;
- embedded HTML line metadata;
- internal ordering/synchronization metadata.

Full-session and selection HTML retain complete timestamps and initially use the session's configured Absolute/Relative mode. After opening an exported HTML report, a user may choose No time interactively.

### Acceptance criteria

- No time hides timestamps and reclaims their display width.
- Selected clipboard text contains no timestamp in No time mode.
- HTML exports still contain timestamp metadata and can show Absolute/Relative time.
- Returning from No time restores timestamps without data loss or reload.

## 5. Preserve canonical full-session export

Do not reintroduce a separate browser full-session renderer. The existing contract remains:

```text
Browser full export ─┐
TUI/daemon export ───┼→ Rust SessionExporter → canonical session.html
Active CLI export ───┘

Recorded CLI export ───→ same Rust SessionExporter
```

Required safeguards:

- Browser full-session Export downloads the exact atomically published `session.html`.
- `embed-log export` uses the daemon path.
- `embed-log sessions export` uses the same renderer post-factum.
- Equal input snapshots and builds produce byte-identical full-session HTML.
- Selection HTML may remain client-generated because it represents a selected subset, but it should reuse shared visual components.

## 6. Test matrix

### Modes and browsers

| Behavior | Live Chromium | Live Edge | Static Chromium | Static Edge |
| --- | ---: | ---: | ---: | ---: |
| Filter typing/paste | Required | Required | Required | Required |
| Invalid regex correction | Required | Required | Required | Required |
| Serial TX typing | Required | Required | N/A | N/A |
| Enter sends exactly once | Required | Required | N/A | N/A |
| Input survives incoming logs | Required | Required | N/A | N/A |
| No time display | Required | Required | Required | Required |
| No-time selection copy | Required | Required | Required | Required |
| Options layout | Required | Required | Required | Required |
| Timestamp metadata retained | Required | Required | Required | Required |

### Layout coverage

At minimum test:

- 1920×1080;
- 1440×900;
- 1280×720;
- one narrow responsive viewport;
- 100%, 125%, and 150% effective scaling where practical.

### Export regressions

Continue verifying:

- downloaded browser bytes equal canonical session HTML;
- daemon and recorded CLI HTML parity;
- virtual merge panes retain original source identity and global sequence;
- full HTML remains replayable offline;
- selection HTML retains timestamps even when clipboard No time is active.

## 7. Implementation order

1. Add an Edge Playwright project and reproduce keyboard failures.
2. Fix editable-target, focus, paste, composition, and Enter handling.
3. Consolidate shared toolbar rendering and repair responsive layout.
4. Remove selection Full/Compact controls and obsolete state.
5. Remove the selection-bar Add note action.
6. Implement Absolute/Relative/No time state and layout.
7. Apply No time only to clipboard selection text.
8. Verify all HTML and raw artifacts retain timestamps.
9. Run the Chromium/Edge live/static matrix.
10. Remove stale frontend CSS/handlers/docs and update the user documentation.

## 8. Explicit non-goals

This overhaul does not:

- remove marker storage, rendering, navigation, or APIs;
- remove compact output from CLI session analysis;
- remove timestamps from any persisted or downloaded artifact;
- enable Serial TX in offline HTML;
- change backend sequence ordering;
- change presentation-only virtual merge semantics;
- replace the canonical Rust full-session exporter.
