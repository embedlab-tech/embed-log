import { state, TABS } from './state.js';
import { scrollPaneToBottom, scrollPaneToTs } from './lines.js';
import { createDynamicTab } from './tabcreate.js';

// ---------------------------------------------------------------------------
// Tab bar
// ---------------------------------------------------------------------------

export function renderTabBar() {
    const bar = document.getElementById("tab-bar");
    if (!bar) return;

    bar.innerHTML = "";

    // Tab buttons — always render, even for a single tab.
    TABS.forEach((tab, idx) => {
        const btn = document.createElement("button");
        btn.className = "tab-btn" + (idx === state.activeTab ? " active" : "");
        btn.textContent = tab.label;
        btn.dataset.tabIdx = String(idx);
        btn.addEventListener("click", () => switchTab(idx));
        bar.appendChild(btn);
    });

    // "+" button — always present
    const addBtn = document.createElement("button");
    addBtn.className   = "tab-btn tab-add";
    addBtn.textContent = "+";
    addBtn.title       = "New tab";
    addBtn.addEventListener("click", () => createDynamicTab());
    bar.appendChild(addBtn);

    // Ensure correct initial visibility of tab contents
    TABS.forEach((_, idx) => {
        const el = document.getElementById("tab-content-" + idx);
        if (el) el.style.display = idx === state.activeTab ? "flex" : "none";
    });
}

// ---------------------------------------------------------------------------
// Tab switching
// ---------------------------------------------------------------------------

export function switchTab(newIdx) {
    if (newIdx === state.activeTab) return;

    // Hide current tab content
    const cur = document.getElementById("tab-content-" + state.activeTab);
    if (cur) cur.style.display = "none";

    state.activeTab = newIdx;

    // Show new tab content
    const next = document.getElementById("tab-content-" + newIdx);
    if (next) next.style.display = "flex";

    // Plain tab switch: show the latest log content by default.
    // Explicit sync gesture (line click / middle-click): keep previous behavior
    // and land near the synced timestamp when switching tabs.
    if (state.syncTabSwitch && state.syncTs !== null) {
        TABS[newIdx].panes.forEach(paneId => scrollPaneToTs(paneId, state.syncTs));
    } else {
        TABS[newIdx].panes.forEach(paneId => scrollPaneToBottom(paneId));
    }

    // Update active button (ignore + button)
    document.querySelectorAll("#tab-bar .tab-btn[data-tab-idx]").forEach(btn => {
        btn.classList.toggle("active", Number(btn.dataset.tabIdx) === newIdx);
    });
}

// ---------------------------------------------------------------------------
// Initialise on load
// ---------------------------------------------------------------------------

renderTabBar();
