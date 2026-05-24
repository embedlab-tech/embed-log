# FRONTEND_REFACTOR

## Goal

Unify the frontend so that:

- `embed-log run` remains the canonical runtime UI,
- exported HTML, merged logs HTML, and session-exported HTML use the same viewer architecture,
- static/exported HTML exposes only the actions that make sense offline,
- the code stops relying on duplicated HTML strings, implicit globals, and mode-by-accident behavior.

## Non-goals

- No build step.
- No framework migration.
- No protocol redesign unless needed for correctness.
- No feature expansion beyond the agreed font-size controls and cleanup of dynamic tabs.

## Required product behavior

### Live runtime

Must support:

- Clear all logs
- Export HTML
- Download raw
- Unwrap
- Pane swap
- Theme toggle
- Font resize
- WebSocket status
- Serial TX
- Session actions
- Selection HTML export

Must not support:

- Dynamic tab add

### Static/exported HTML

Applies to:

- runtime `Export HTML`
- `embed-log merge`
- session HTML export

Must support:

- Download raw
- Unwrap
- Pane swap
- Theme toggle
- Font resize
- Filters
- Selection copy/download
- Selection HTML export (snippet from selected lines)
- Clipboard buffer if kept


Must not support:

- Clear all logs
- Export HTML (full session)
- WebSocket status
- Serial TX
- Session actions
- Dynamic tab add

## Current architectural problems

1. Mode differences are implicit:
   - `file:` checks
   - `window.wsSend` stubs
   - hidden DOM rows
   - omitted scripts
2. Pane HTML is duplicated in multiple places:
   - `frontend/tabcreate.js`
   - `frontend/export.js`
   - `utils/merge_logs.py`
3. Startup is side-effect driven:
   - modules auto-run on import
4. Runtime-only and static-only actions are not defined by a capability model.
5. Dynamic tabs still exist even though they are not a real product feature.
6. Font size has partial state/CSS support but no user-facing implementation.

## Target architecture

## 1. Explicit viewer profile

Add a single source of truth for frontend capabilities.

Suggested file:

- `frontend/profile.js`

It should define a profile object injected at boot time, for example:

- `kind: "live" | "static"`
- `capabilities.clearAll`
- `capabilities.exportHtml`
- `capabilities.downloadRaw`
- `capabilities.unwrap`
- `capabilities.paneSwap`
- `capabilities.themeToggle`
- `capabilities.fontSize`
- `capabilities.wsStatus`
- `capabilities.tx`
- `capabilities.sessionApi`
- `capabilities.selectionExportHtml`
- `capabilities.persistCache`

All gating must go through this profile. No feature should depend on `file:` checks or no-op service stubs to decide whether it exists.

## 2. Two bootstraps, one viewer core

Create two explicit entry paths:

- live bootstrap for `embed-log run`
- static bootstrap for exported/merged/session HTML

Both should use the same viewer core.

The only differences should be:

- initial data source
- available services
- enabled capabilities

## 3. Shared renderers

Extract shared DOM creation for:

- toolbar
- settings/options row
- pane shell
- optional static-only/live-only sections controlled by profile

The long-term source of truth must be renderer code plus capability definitions, not duplicated HTML strings or captured DOM.

## 4. Service boundaries

Split viewer code from runtime services.

Suggested responsibilities:

- viewer core: tabs, panes, filters, selection, wrap, swap, rendering, theme, font size
- live transport service: WebSocket connect/send/status
- session API service: save/open/rotate/list
- persistence service: cache restore/save
- export service: snapshot serialization and standalone HTML assembly

## 5. Shared static snapshot contract

All static HTML producers must target the same snapshot shape.

Suggested fields:

- `profile`
- `themeState`
- `fontSize`
- `tabs`
- `panes`
- `activeTab`
- `logData`

`frontend/export.js`, `utils/merge_logs.py`, and session export must all produce or consume this same contract.

---

# Task plan

## Phase 1 — Introduce profile-driven gating

### Tasks

- [ ] Create `frontend/profile.js`.
- [ ] Define the live profile.
- [ ] Define the static profile.
- [ ] Make bootstraps provide `window.__embedLogProfile` before UI modules run.
- [ ] Replace `window.location.protocol === 'file:'` checks with capability checks.
- [ ] Replace feature assumptions based on `window.wsSend` presence with capability checks.
- [ ] Gate toolbar/settings/session actions through the profile.
- [ ] Gate TX input wiring through the profile.
- [ ] Gate `ws-status` rendering through the profile.
- [ ] Gate selection HTML export through the profile.

### Files likely touched

- `frontend/main.js`
- `frontend/ui.js`
- `frontend/selection.js`
- `frontend/export.js`
- `frontend/settings.js`
- `frontend/ws.js`
- new `frontend/profile.js`

### Done when

- Live mode keeps all live-only actions.
- Static mode hides or omits disallowed actions.
- No frontend behavior depends on `file:` protocol for capability decisions.

## Phase 2 — Remove dynamic tabs

### Tasks

- [ ] Delete the `+` tab button from `frontend/tabs.js`.
- [ ] Remove `createDynamicTab()` from `frontend/tabcreate.js`.
- [ ] Remove prompt-based tab creation logic.
- [ ] Replace `ws.js` handling of unknown `source_id` with explicit policy.
- [ ] Decide and implement one policy for unknown runtime sources:
  - ignore with warning, or
  - map to a single explicit fallback pane if product requires it
- [ ] Remove any tests or code paths that depend on runtime-created tabs.

### Files likely touched

- `frontend/tabs.js`
- `frontend/tabcreate.js`
- `frontend/ws.js`
- related tests

### Done when

- No dynamic tab UI exists.
- No runtime prompt-based tab creation exists.
- Unknown source handling is explicit and test-covered.

## Phase 3 — Add font-size controls properly

### Tasks

- [ ] Add font-size controls to the toolbar or settings/options area.
- [ ] Implement decrease/increase/reset behavior.
- [ ] Apply font size through the existing `--font-size` CSS variable.
- [ ] Store font size in `state` as the single source of truth.
- [ ] Persist font size in live cache if persistence is enabled.
- [ ] Include font size in exported snapshot data.
- [ ] Restore font size in static/exported HTML.
- [ ] Verify font controls are available in both live and static profiles.

### Files likely touched

- `frontend/state.js`
- `frontend/ui.js`
- `frontend/settings.js`
- `frontend/viewer.css`
- `frontend/persist.js`
- `frontend/export.js`
- `utils/merge_logs.py`

### Done when

- User can resize log font in live and static HTML.
- Exported HTML preserves the chosen size.
- Font-size changes survive runtime refresh when persistence is enabled.

## Phase 4 — Extract shared pane renderer

### Tasks

- [ ] Create a shared pane renderer function/module.
- [ ] Make live runtime pane creation use it.
- [ ] Make unwrap rebuild use it.
- [ ] Make static export use the same pane renderer shape.
- [ ] Remove duplicated pane HTML strings from `tabcreate.js` and `export.js`.
- [ ] Align merge/session export pane structure to that same shape.
- [ ] Ensure capability-based omission of TX row rather than hiding dead UI when possible.

### Files likely touched

- `frontend/tabcreate.js`
- `frontend/export.js`
- new renderer module(s)
- `utils/merge_logs.py`

### Done when

- Pane DOM structure has one maintained source of truth.
- Live and static panes differ only by profile-driven capability choices.

## Phase 5 — Extract shared toolbar/options renderer

### Tasks

- [ ] Replace duplicated toolbar definitions with renderer code.
- [ ] Define toolbar actions declaratively.
- [ ] Render only actions enabled by the current profile.
- [ ] Move font-size controls into the shared toolbar/options model.
- [ ] Remove the need to capture toolbar DOM in `export.js`.
- [ ] Remove `frontend/toolbar.html` if it is no longer needed.

### Files likely touched

- `frontend/index.html`
- `frontend/settings.js`
- `frontend/ui.js`
- `frontend/export.js`
- `utils/merge_logs.py`
- new renderer/action modules

### Done when

- Runtime and static toolbar composition comes from one action model.
- No hand-maintained duplicated toolbar markup remains.

## Phase 6 — Split startup into explicit bootstraps

### Tasks

- [ ] Replace side-effect startup with explicit init functions.
- [ ] Introduce a live bootstrap.
- [ ] Introduce a static bootstrap.
- [ ] Ensure `ws.js` does not auto-connect on import.
- [ ] Ensure persistence does not auto-register globals without init.
- [ ] Ensure theme/settings/export wiring is initialized explicitly.

### Files likely touched

- `frontend/main.js`
- `frontend/ws.js`
- `frontend/persist.js`
- `frontend/export.js`
- `frontend/settings.js`
- new bootstrap/init modules

### Done when

- Importing a module does not unexpectedly start services.
- Live and static modes are assembled intentionally.

## Phase 7 — Unify static snapshot generation

### Tasks

- [ ] Define one snapshot schema for standalone HTML.
- [ ] Make browser-side export emit that schema.
- [ ] Make `merge_logs.py` emit or consume that schema.
- [ ] Make session export use the same path/contract.
- [ ] Consolidate asset manifest handling.
- [ ] Consolidate module-stripping / embedding rules.
- [ ] Ensure static HTML output from all three paths behaves the same.

### Files likely touched

- `frontend/export.js`
- `utils/merge_logs.py`
- `backend/session/exporter.py`
- possibly `backend/core/runtime.py`

### Done when

- Runtime export, merge output, and session export produce the same viewer behavior.
- Static output capability differences are intentional and identical.

---

# Testing tasks

## Runtime tests

- [ ] Verify live toolbar contains only live actions.
- [ ] Verify static-disallowed actions are absent in runtime exports.
- [ ] Verify TX still works in live mode.
- [ ] Verify save/rotate/session actions still work in live mode.
- [ ] Verify unknown source handling matches the chosen policy.

## Static/export tests

- [ ] Verify exported HTML has no clear action.
- [ ] Verify exported HTML has no HTML export action.
- [ ] Verify exported HTML has no TX UI.
- [ ] Verify exported HTML has no session actions.
- [ ] Verify exported HTML supports unwrap.
- [ ] Verify exported HTML supports pane swap.
- [ ] Verify exported HTML supports font resize.
- [ ] Verify exported HTML preserves selected font size from runtime export.
- [ ] Verify `embed-log merge` output matches runtime-exported static behavior.
- [ ] Verify session-exported HTML matches the same static behavior.

## Regression-sensitive areas

- [ ] cache restore after layout mutations
- [ ] selection actions after unwrap and pane swap
- [ ] selection raw export in both exact and context modes
- [ ] active tab restore in exported HTML
- [ ] theme restore and toggle in static HTML

---

# Recommended implementation order

1. Phase 1 — profile-driven gating
2. Phase 2 — remove dynamic tabs
3. Phase 3 — add font-size controls
4. Phase 4 — shared pane renderer
5. Phase 5 — shared toolbar/options renderer
6. Phase 6 — explicit bootstraps
7. Phase 7 — unified static snapshot contract

This order reduces risk:

- first make behavior explicit,
- then delete dead features,
- then add the missing user feature,
- then remove duplication,
- then clean startup,
- then finish full export-path unification.
