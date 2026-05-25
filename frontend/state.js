// Tab definitions — each tab shows 1 or 2 panes side-by-side.
// In live mode TABS/PANES start empty; ws.js populates them dynamically.
// In static mode (export/merge_logs) a classic <script> sets window.TABS and
// window.PANES before this module runs so the per-pane state is pre-seeded.
export const TABS  = window.TABS  ?? [];
export const PANES = window.PANES ?? [...new Set(TABS.flatMap(t => t.panes))];
export const PANE_LABELS = window.PANE_LABELS ?? {};
export function paneLabel(paneId) {
    return PANE_LABELS[paneId] || paneId;
}

export function unwrapPaneLabel(paneId) {
    const base = paneLabel(paneId);
    const tab = TABS.find(t => t.panes.includes(paneId));
    if (!tab) return base;
    return base + '-' + tab.label;
}

export const state = {
    showTs:      true,

    fontSize:    14,
    activeTab:   0,
    activePaneTab: 0,
    syncTs:      null,   // last-clicked numeric timestamp
    syncTabSwitch: false, // true after explicit line sync; next tab switches follow syncTs
    filters:     {},
    wrap:        {},
    rawLines:    {},
    atBottom:    {},
    highlighted: {},
    selected:    {},
    selectionScope:  'exact', // 'exact', 'context', or 'context-selected'
    contextPanes:    {},       // paneId → bool; only used when selectionScope === 'context-selected'
    unwrap:        false,
};

// Initialise per-pane state for every pane in the system
PANES.forEach(id => {
    state.filters[id]     = null;
    state.rawLines[id]    = [];
    state.atBottom[id]    = true;

    state.wrap[id]        = false;
    state.highlighted[id] = null;
    state.selected[id]    = new Set();
});
