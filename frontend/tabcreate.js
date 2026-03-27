"use strict";

// ---------------------------------------------------------------------------
// Tab creation
//
// createTabWithPanes(label, paneIds, [opts])
//   Core function. Creates a tab containing 1 or 2 panes side-by-side.
//   Used by ws.js (config-driven layout) and createDynamicTab (user prompt).
//
// createDynamicTab([label, [paneId]])
//   User-facing "+" button handler. Prompts for a name when label is
//   omitted, generates a pane ID when paneId is omitted, then delegates
//   to createTabWithPanes.  Also called by ws.js for unknown source_ids
//   that arrive without a prior config message.
// ---------------------------------------------------------------------------

function createTabWithPanes(label, paneIds, { switchTo = true } = {}) {
    const tabIdx = TABS.length;

    // ---- 1. State ----
    TABS.push({ id: "tab-" + tabIdx, label, panes: paneIds });
    paneIds.forEach(paneId => {
        if (PANES.includes(paneId)) return;   // already registered
        PANES.push(paneId);
        state.filters[paneId]     = null;
        state.rawLines[paneId]    = [];
        state.atBottom[paneId]    = true;
        state.highlighted[paneId] = null;
        state.selected[paneId]    = new Set();
    });

    // ---- 2. DOM ----
    const container = document.getElementById("container");
    const tc = document.createElement("div");
    tc.className  = "tab-content";
    tc.id         = "tab-content-" + tabIdx;
    tc.style.display = (switchTo || tabIdx === 0) ? "flex" : "none";

    const parts = [];
    paneIds.forEach((paneId, i) => {
        if (i > 0) parts.push('<div class="splitter"></div>');
        parts.push(`\
        <div class="pane" id="pane-${paneId}">
            <div class="pane-header">
                <span class="pane-name">${_escHtml(paneId)}</span>
                <button class="pane-clear-btn" data-pane="${paneId}">clear</button>
            </div>
            <div class="filter-bar">
                <input class="filter-input" data-pane="${paneId}" placeholder="Filter (regex)…">
            </div>
            <div class="pane-body">
                <div class="log-area" id="log-${paneId}"></div>
                <button class="jump-btn" id="jump-${paneId}">jump to bottom</button>
            </div>
            <div class="input-row">
                <input class="serial-input" id="input-${paneId}" placeholder="Serial TX — press Enter to send" autocomplete="off">
                <button class="send-btn" data-pane="${paneId}">Send</button>
            </div>
        </div>`);
    });
    tc.innerHTML = parts.join("\n");
    container.appendChild(tc);

    // ---- 3. Per-pane event wiring ----
    paneIds.forEach(paneId => {
        _linesSetupPane(paneId);
        _uiSetupPane(paneId);
        _uiSetupTxPane(paneId);
        _selectionSetupPane(paneId);
        _importSetupPane(paneId);
    });

    // ---- 4. Show ----
    renderTabBar();
    if (switchTo) switchTab(tabIdx);
}


function createDynamicTab(label, paneId) {
    if (label === undefined) {
        const name = window.prompt("New tab name:", "Tab " + (TABS.length + 1));
        if (name === null || !name.trim()) return;
        label = name.trim();
    }
    if (paneId === undefined) paneId = "dyn-" + TABS.length;
    createTabWithPanes(label, [paneId], { switchTo: true });
}


function _escHtml(str) {
    return str
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;")
        .replace(/"/g, "&quot;");
}
