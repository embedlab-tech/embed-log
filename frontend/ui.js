"use strict";

// ---------------------------------------------------------------------------
// Toolbar — wrap
// ---------------------------------------------------------------------------
const btnWrap = document.getElementById("btn-wrap");
btnWrap.addEventListener("click", () => {
    state.wrap = !state.wrap;
    btnWrap.classList.toggle("active", state.wrap);
    PANES.forEach(id => document.getElementById("log-" + id).classList.toggle("wrap", state.wrap));
});

// ---------------------------------------------------------------------------
// Toolbar — timestamps
// ---------------------------------------------------------------------------
const btnTs = document.getElementById("btn-ts");
btnTs.addEventListener("click", () => {
    state.showTs = !state.showTs;
    btnTs.classList.toggle("active", state.showTs);
    PANES.forEach(rerenderPane);
});

// ---------------------------------------------------------------------------
// Toolbar — font size
// ---------------------------------------------------------------------------
document.getElementById("btn-font-dec").addEventListener("click", () => {
    state.fontSize = Math.max(9, state.fontSize - 1);
    document.documentElement.style.setProperty("--font-size", state.fontSize + "px");
});
document.getElementById("btn-font-inc").addEventListener("click", () => {
    state.fontSize = Math.min(24, state.fontSize + 1);
    document.documentElement.style.setProperty("--font-size", state.fontSize + "px");
});

// ---------------------------------------------------------------------------
// Toolbar — sync toggle
// ---------------------------------------------------------------------------
const btnSync = document.getElementById("btn-sync");
btnSync.addEventListener("click", () => {
    state.syncEnabled = !state.syncEnabled;
    btnSync.classList.toggle("active", state.syncEnabled);
});

// ---------------------------------------------------------------------------
// Toolbar — theme toggle (single button: shows target-mode icon)
//   light (Whitesand) → button shows 🌙  (click to go dark)
//   dark  (Mocha)     → button shows ☀  (click to go light)
// ---------------------------------------------------------------------------
(function () {
    const btn = document.getElementById("btn-theme");

    function setTheme(theme) {
        document.documentElement.setAttribute("data-theme", theme);
        btn.textContent = theme === "whitesand" ? "🌙" : "☀";
    }

    btn.addEventListener("click", () => {
        const isDark = document.documentElement.getAttribute("data-theme") === "";
        setTheme(isDark ? "whitesand" : "");
    });

    // Sync icon with the initial data-theme set in the HTML
    setTheme(document.documentElement.getAttribute("data-theme") || "");
})();

// ---------------------------------------------------------------------------
// Filter inputs
// ---------------------------------------------------------------------------
function _uiSetupPane(id) {
    const input = document.querySelector(`.filter-input[data-pane="${id}"]`);
    if (!input) return;
    input.addEventListener("input", () => {
        const val = input.value.trim();
        if (!val) {
            state.filters[id] = null;
            input.classList.remove("invalid");
        } else {
            try {
                state.filters[id] = new RegExp(val, "i");
                input.classList.remove("invalid");
            } catch {
                state.filters[id] = null;
                input.classList.add("invalid");
            }
        }
        rerenderPane(id);
    });
}
PANES.forEach(_uiSetupPane);

// ---------------------------------------------------------------------------
// Serial TX input — Enter or Send button
// ---------------------------------------------------------------------------
function sendSerial(paneId) {
    const input = document.getElementById("input-" + paneId);
    if (!input) return;
    const text  = input.value.trim();
    if (!text) return;
    input.value = "";
    wsSend({ cmd: "send_raw", id: paneId, data: text + "\n" });
}

function _uiSetupTxPane(id) {
    const input = document.getElementById("input-" + id);
    if (!input) return;
    input.addEventListener("keydown", e => {
        if (e.key === "Enter") { e.preventDefault(); sendSerial(id); }
    });
    const sendBtn = document.querySelector(`.send-btn[data-pane="${id}"]`);
    if (sendBtn) sendBtn.addEventListener("click", () => sendSerial(id));
}

PANES.forEach(_uiSetupTxPane);

// ---------------------------------------------------------------------------
// Splitter drag — works for any .splitter element; auto-detects left/right panes
// ---------------------------------------------------------------------------
document.querySelectorAll(".splitter").forEach(splitter => {
    splitter.addEventListener("pointerdown", e => {
        const tabContent = splitter.parentElement;

        // Find the nearest .pane before and after the splitter
        let paneLeft = null, paneRight = null, passed = false;
        for (const child of tabContent.children) {
            if (child === splitter) { passed = true; continue; }
            if (child.classList.contains("pane")) {
                if (!passed) paneLeft = child;
                else if (!paneRight) paneRight = child;
            }
        }
        if (!paneLeft || !paneRight) return;

        splitter.classList.add("dragging");
        splitter.setPointerCapture(e.pointerId);

        const startX     = e.clientX;
        const startLeftW = paneLeft.getBoundingClientRect().width;
        const totalW     = tabContent.getBoundingClientRect().width - splitter.offsetWidth;

        function onMove(ev) {
            const newLeft = Math.min(Math.max(startLeftW + ev.clientX - startX, 120), totalW - 120);
            paneLeft.style.flex  = "none";
            paneRight.style.flex = "none";
            paneLeft.style.width  = newLeft + "px";
            paneRight.style.width = (totalW - newLeft) + "px";
        }
        function onUp() {
            splitter.classList.remove("dragging");
            splitter.removeEventListener("pointermove", onMove);
            splitter.removeEventListener("pointerup",   onUp);
        }
        splitter.addEventListener("pointermove", onMove);
        splitter.addEventListener("pointerup",   onUp);
    });
});
