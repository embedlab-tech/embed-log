"use strict";

// ---------------------------------------------------------------------------
// Tab bar
// ---------------------------------------------------------------------------

function renderTabBar() {
    const bar = document.getElementById("tab-bar");
    if (!bar) return;

    bar.innerHTML = "";

    // Tab buttons — only rendered when there is more than one tab
    if (TABS.length > 1) {
        TABS.forEach((tab, idx) => {
            const btn = document.createElement("button");
            btn.className = "tab-btn" + (idx === state.activeTab ? " active" : "");
            btn.textContent = tab.label;
            btn.addEventListener("click", () => switchTab(idx));
            bar.appendChild(btn);
        });
    }

    // "+" button — always present; calls createDynamicTab (defined in tabcreate.js)
    const addBtn = document.createElement("button");
    addBtn.className   = "tab-btn tab-add";
    addBtn.textContent = "+";
    addBtn.title       = "New tab";
    addBtn.addEventListener("click", () => {
        if (typeof createDynamicTab === "function") createDynamicTab();
    });
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

function switchTab(newIdx) {
    if (newIdx === state.activeTab) return;

    // Hide current tab content
    const cur = document.getElementById("tab-content-" + state.activeTab);
    if (cur) cur.style.display = "none";

    state.activeTab = newIdx;

    // Show new tab content
    const next = document.getElementById("tab-content-" + newIdx);
    if (next) next.style.display = "flex";

    // Scroll the new tab's panes to the last synced timestamp so the user
    // lands in the right context without having to click again.
    if (state.syncTs !== null && state.syncEnabled) {
        TABS[newIdx].panes.forEach(paneId => scrollPaneToTs(paneId, state.syncTs));
    }

    // Update active button
    document.querySelectorAll("#tab-bar .tab-btn").forEach((btn, idx) => {
        btn.classList.toggle("active", idx === newIdx);
    });
}

// ---------------------------------------------------------------------------
// Initialise on load
// ---------------------------------------------------------------------------

renderTabBar();
