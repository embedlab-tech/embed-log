import { state, TABS, PANES, paneLabel, unwrapPaneLabel } from './state.js';

import { _linesSetupPane, repopulatePaneLogs } from './lines.js';
import { _uiSetupPane, _uiSetupTxPane } from './ui.js';
import { _selectionSetupPane, applyMarkers } from './selection.js';
import { renderTabBar, switchTab } from './tabs.js';
import { renderPaneShell } from './renderPane.js';

// ---------------------------------------------------------------------------
// Tab creation
//
// createTabWithPanes(label, paneIds, [opts])
//   Core function. Creates a tab containing 1 or 2 panes side-by-side.
//   Used by ws.js (config-driven layout).
// ---------------------------------------------------------------------------


export function createTabWithPanes(label, paneIds, { switchTo = true, paneLabels = {} } = {}) {
    const tabIdx = TABS.length;

    // ---- 1. State ----
    TABS.push({ id: "tab-" + tabIdx, label, panes: paneIds, paneLabels: { ...paneLabels } });
    paneIds.forEach(paneId => {
        if (PANES.includes(paneId)) return;   // already registered
        PANES.push(paneId);
        state.filters[paneId]     = null;
        state.rawLines[paneId]    = [];
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
        const panekind = window.__embedLogPaneKinds?.[paneId];
        const showTx = panekind === "uart";
        parts.push(renderPaneShell(paneId, paneLabel(paneId), { showTx }));
    });
    tc.innerHTML = parts.join("\n");
    container.appendChild(tc);

    // ---- 3. Per-pane event wiring ----
    paneIds.forEach(paneId => {
        _linesSetupPane(paneId);
        _uiSetupPane(paneId);
        if (window.__embedLogPaneKinds?.[paneId] === "uart") _uiSetupTxPane(paneId);
        _selectionSetupPane(paneId);

    });

    // ---- 4. Show ----
    renderTabBar();
    if (switchTo) switchTab(tabIdx);
    window.__embedLogSchedulePersist?.();
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
            const showTxU = window.__embedLogPaneKinds?.[paneId] === "uart";
            tc.innerHTML = renderPaneShell(paneId, unwrapPaneLabel(paneId), { showTx: showTxU });

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
                const showTxG = window.__embedLogPaneKinds?.[paneId] === "uart";
                parts.push(renderPaneShell(paneId, paneLabel(paneId), { showTx: showTxG }));

            });
            tc.innerHTML = parts.join("\n");
            container.appendChild(tc);
        });
    }

    // Re-wire per-pane handlers and repopulate logs
    PANES.forEach(paneId => {
        _linesSetupPane(paneId);
        _uiSetupPane(paneId);
        if (window.__embedLogPaneKinds?.[paneId] === "uart") _uiSetupTxPane(paneId);
        _selectionSetupPane(paneId);
    });
    PANES.forEach(paneId => repopulatePaneLogs(paneId));
    applyMarkers();
    renderTabBar();
    const targetTab = state.unwrap
        ? (state.activePaneTab < PANES.length ? state.activePaneTab : 0)
        : (state.activeTab < TABS.length ? state.activeTab : 0);
    switchTab(targetTab);
}


