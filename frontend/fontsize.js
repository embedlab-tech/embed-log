import { state } from './state.js';

// Initialise font-size from saved state or bootstrap config.
// In exported/static HTML, window.__embedLogInitialFontSize is set by configJs.
// In live mode it is absent, so state.fontSize (default 14) is used.
const initialFontSize = window.__embedLogInitialFontSize;
if (typeof initialFontSize === 'number' && initialFontSize > 0) {
    state.fontSize = initialFontSize;
}

export function applyFontSize() {
    document.documentElement.style.setProperty('--font-size', state.fontSize + 'px');
    window.__embedLogInvalidateVirtualMetrics?.();
}

(function () {
    // Apply the font size on load.
    applyFontSize();

    const panel = document.getElementById("settings-panel");
    if (!panel) return;

    // Separator
    const sep = document.createElement("span");
    sep.className = "set-sep";
    sep.textContent = "|";

    // Decrease
    const decBtn = document.createElement("button");
    decBtn.id = "btn-font-dec";
    decBtn.title = "Decrease font size";
    decBtn.textContent = "A-";
    decBtn.addEventListener("click", () => {
        state.fontSize = Math.max(8, state.fontSize - 1);
        applyFontSize();
        window.__embedLogSchedulePersist?.();
    });

    // Reset
    const resetBtn = document.createElement("button");
    resetBtn.id = "btn-font-reset";
    resetBtn.title = "Reset font size to 14px";
    resetBtn.textContent = "A";
    resetBtn.addEventListener("click", () => {
        state.fontSize = 14;
        applyFontSize();
        window.__embedLogSchedulePersist?.();
    });

    // Increase
    const incBtn = document.createElement("button");
    incBtn.id = "btn-font-inc";
    incBtn.title = "Increase font size";
    incBtn.textContent = "A+";
    incBtn.addEventListener("click", () => {
        state.fontSize = Math.min(32, state.fontSize + 1);
        applyFontSize();
        window.__embedLogSchedulePersist?.();
    });

    panel.appendChild(decBtn);
    panel.appendChild(resetBtn);
    panel.appendChild(incBtn);
    panel.appendChild(sep);
})();
