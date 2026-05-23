// Tab definitions — each tab shows 1 or 2 panes side-by-side.
// In live mode TABS/PANES start empty; ws.js populates them dynamically.
// In static mode (export/merge_logs) a classic <script> sets window.TABS and
// window.PANES before this module runs so the per-pane state is pre-seeded.
export const TABS  = window.TABS  ?? [];
export const PANES = window.PANES ?? [...new Set(TABS.flatMap(t => t.panes))];

export const state = {
    wrap:        false,
    showTs:      true,
    fontSize:    14,
    activeTab:   0,
    syncTs:      null,   // last-clicked numeric timestamp
    syncTabSwitch: false, // true after explicit line sync; next tab switches follow syncTs
    filters:     {},
    rawLines:    {},
    atBottom:    {},
    highlighted: {},
    selected:    {},
    selectionScope:  'exact', // 'exact' or 'context'
};

// Initialise per-pane state for every pane in the system
PANES.forEach(id => {
    state.filters[id]     = null;
    state.rawLines[id]    = [];
    state.atBottom[id]    = true;
    state.highlighted[id] = null;
    state.selected[id]    = new Set();
});
