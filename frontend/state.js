"use strict";

// Tab definitions — each tab shows 1 or 2 panes side-by-side.
// merge_logs.py / export.js inject a replacement before this file loads for static files.
// In live mode TABS starts empty and is populated dynamically by ws.js.
if (typeof TABS === "undefined") {
    var TABS = [];
}

// All pane IDs across all tabs (derived from TABS — do not set manually).
if (typeof PANES === "undefined") {
    var PANES = [...new Set(TABS.flatMap(t => t.panes))];
}

const state = {
    wrap:        false,
    showTs:      true,
    syncEnabled: true,
    fontSize:    14,
    activeTab:   0,
    syncTs:      null,   // last-clicked numeric timestamp, persists across tab switches
    filters:     {},
    rawLines:    {},
    atBottom:    {},
    highlighted: {},
    selected:    {},
    settings: {
        tsFormat:     "full",   // "full" | "time" | "compact"
        tagColors:    true,     // colorise <wrn> <dbg> <inf> <err> tags
        embedTsStrip: false,    // hide secondary [HH:MM:SS] timestamps in content
    },
};

// Initialise per-pane state for every pane in the system
PANES.forEach(id => {
    state.filters[id]     = null;
    state.rawLines[id]    = [];
    state.atBottom[id]    = true;
    state.highlighted[id] = null;
    state.selected[id]    = new Set();
});
