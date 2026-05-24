import { state, TABS, PANES } from './state.js';
import { _importSetupPane } from './import.js';
import { _linesSetupPane, repopulatePaneLogs } from './lines.js';
import { _uiSetupPane, _uiSetupTxPane } from './ui.js';
import { _selectionSetupPane } from './selection.js';
import { renderTabBar, switchTab } from './tabs.js';



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

export function createTabWithPanes(label, paneIds, { switchTo = true } = {}) {
    const tabIdx = TABS.length;

    // ---- 1. State ----
    TABS.push({ id: "tab-" + tabIdx, label, panes: paneIds });
    paneIds.forEach(paneId => {
        if (PANES.includes(paneId)) return;   // already registered
        PANES.push(paneId);
        state.filters[paneId]     = null;
        state.rawLines[paneId]    = [];
        state.renderBase[paneId]  = 0;
        state.atBottom[paneId]    = true;
        state.highlighted[paneId] = null;
        state.selected[paneId]    = new Set();

    state.wrap[paneId]        = false;
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

                <button class="pane-wrap-btn" title="Toggle word wrap in this pane">Wrap</button>

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
    window.__embedLogSchedulePersist?.();
}


export function createDynamicTab(label, paneId) {
    if (label === undefined) {
        const name = window.prompt("New tab name:", "Tab " + (TABS.length + 1));
        if (name === null || !name.trim()) return;
        label = name.trim();
    }
    if (paneId === undefined) paneId = "dyn-" + TABS.length;
    createTabWithPanes(label, [paneId], { switchTo: true });
}

// ---------------------------------------------------------------------------
// Layout rebuild — used when toggling UNWRAP mode or restoring from cache.
// Rebuilds the entire container from TABS/PANES state without losing log data.
// ---------------------------------------------------------------------------
export function rebuildLayout(previousUnwrap = state.unwrap) {
    const container = document.getElementById("container");
    if (!container) return;

    const activeGroupBefore = state.activeTab;
    const activePaneBefore = previousUnwrap
        ? state.activePaneTab
        : Math.max(0, PANES.indexOf(TABS[activeGroupBefore]?.panes?.[0] ?? PANES[0]));
    container.innerHTML = "";

    if (state.unwrap) {
        // One tab per pane, full-width, no splitters
        PANES.forEach((paneId, idx) => {
            const tc = document.createElement("div");
            tc.className = "tab-content";
            tc.id = "u-tab-content-" + idx;
            tc.style.display = idx === activePaneBefore ? "flex" : "none";
            tc.innerHTML = `\
        <div class="pane" id="pane-${paneId}">
            <div class="pane-header">
                <span class="pane-name">${_escHtml(paneId)}</span>
                <button class="pane-wrap-btn" title="Toggle word wrap in this pane">Wrap</button>
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
        </div>`;
            container.appendChild(tc);
        });
        state.activePaneTab = activePaneBefore < PANES.length ? activePaneBefore : 0;
    } else {
        const activePaneId = PANES[activePaneBefore] ?? TABS[activeGroupBefore]?.panes?.[0] ?? PANES[0];
        const groupedIdx = TABS.findIndex(tab => tab.panes.includes(activePaneId));
        state.activeTab = groupedIdx >= 0 ? groupedIdx : (activeGroupBefore < TABS.length ? activeGroupBefore : 0);

        // Rebuild original grouped layout from TABS
        TABS.forEach((tab, idx) => {
            const tc = document.createElement("div");
            tc.className = "tab-content";
            tc.id = "tab-content-" + idx;
            tc.style.display = idx === state.activeTab ? "flex" : "none";
            const parts = [];
            tab.panes.forEach((paneId, pi) => {
                if (pi > 0) parts.push('<div class="splitter"></div>');
                parts.push(`\
        <div class="pane" id="pane-${paneId}">
            <div class="pane-header">
                <span class="pane-name">${_escHtml(paneId)}</span>
                <button class="pane-wrap-btn" title="Toggle word wrap in this pane">Wrap</button>
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
        });
    }

    // Re-wire per-pane handlers and repopulate logs
    PANES.forEach(paneId => {
        _linesSetupPane(paneId);
        _uiSetupPane(paneId);
        _uiSetupTxPane(paneId);
        _selectionSetupPane(paneId);
        _importSetupPane(paneId);
    });
    PANES.forEach(paneId => repopulatePaneLogs(paneId));

    renderTabBar();
    const targetTab = state.unwrap
        ? (state.activePaneTab < PANES.length ? state.activePaneTab : 0)
        : (state.activeTab < TABS.length ? state.activeTab : 0);
    switchTab(targetTab);
}


export function _escHtml(str) {
    return str
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;")
        .replace(/"/g, "&quot;");
}
