import { state, TABS, PANES } from './state.js';
import { scrollPaneToBottom, scrollPaneToTs } from './lines.js';

// ---------------------------------------------------------------------------
// Tab bar
// ---------------------------------------------------------------------------

export function renderTabBar() {
    const bar = document.getElementById("tab-bar");
    if (!bar) return;
    bar.innerHTML = "";

    const activeIdx = state.unwrap ? state.activePaneTab : state.activeTab;

    if (state.unwrap) {
        // One tab per pane, no "+" button
        PANES.forEach((paneId, idx) => {
            const btn = document.createElement("button");
            btn.className = "tab-btn" + (idx === activeIdx ? " active" : "");
            btn.textContent = paneId;
            btn.dataset.tabIdx = String(idx);
            btn.addEventListener("click", () => switchTab(idx));
            bar.appendChild(btn);
        });
        // Ensure correct visibility of unwrapped tab contents
        PANES.forEach((_, idx) => {
            const el = document.getElementById("u-tab-content-" + idx);
            if (el) el.style.display = idx === activeIdx ? "flex" : "none";
        });
    } else {
        // Original: one tab per config entry
        TABS.forEach((tab, idx) => {
            const btn = document.createElement("button");
            btn.className = "tab-btn" + (idx === activeIdx ? " active" : "");
            btn.textContent = tab.label;
            btn.dataset.tabIdx = String(idx);
            btn.addEventListener("click", () => switchTab(idx));
            bar.appendChild(btn);
        });
        // Ensure correct visibility
        TABS.forEach((_, idx) => {
            const el = document.getElementById("tab-content-" + idx);
            if (el) el.style.display = idx === activeIdx ? "flex" : "none";
        });
    }
}

// ---------------------------------------------------------------------------
// Tab switching
// ---------------------------------------------------------------------------

export function switchTab(newIdx) {
    const activeIdx = state.unwrap ? state.activePaneTab : state.activeTab;
    if (newIdx === activeIdx) return;

    // Hide current tab content
    const curId = state.unwrap ? "u-tab-content-" + activeIdx : "tab-content-" + activeIdx;
    const cur = document.getElementById(curId);
    if (cur) cur.style.display = "none";

    if (state.unwrap) state.activePaneTab = newIdx;
    else state.activeTab = newIdx;

    // Show new tab content
    const nextId = state.unwrap ? "u-tab-content-" + newIdx : "tab-content-" + newIdx;
    const next = document.getElementById(nextId);
    if (next) next.style.display = "flex";

    // Scroll logic
    const panesToScroll = state.unwrap ? [PANES[newIdx]] : TABS[newIdx]?.panes || [];
    if (state.syncTabSwitch && state.syncTs !== null) {
        panesToScroll.forEach(paneId => scrollPaneToTs(paneId, state.syncTs));
    } else {
        panesToScroll.forEach(paneId => scrollPaneToBottom(paneId));
    }

    // Update active button
    document.querySelectorAll("#tab-bar .tab-btn[data-tab-idx]").forEach(btn => {
        btn.classList.toggle("active", Number(btn.dataset.tabIdx) === newIdx);
    });
}

// ---------------------------------------------------------------------------
// Initialise on load
// ---------------------------------------------------------------------------

renderTabBar();
