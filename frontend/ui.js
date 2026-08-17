import { state, TABS, PANES, paneLabel } from './state.js';
import { rerenderPane, reanalyzePanePlugins, setTimestampMode, canDisplayTimestampMode, hidePluginOverlays } from './lines.js';
import { rebuildLayout } from './tabcreate.js';
import { getPanePluginSettings, setPanePluginSetting } from './pluginRuntime.js';




// ---------------------------------------------------------------------------
// Toolbar — clear live view
// ---------------------------------------------------------------------------
// Clearing is a presentation action, but goes through the backend so every
// connected viewer gets the same clear_logs broadcast. The backend does not
// delete or rotate the persisted session.
document.getElementById("btn-clear")?.addEventListener("click", () => {
    window.wsSend?.({ cmd: "clear_logs" });
});

// ---------------------------------------------------------------------------
// Toolbar — UNWRAP toggle
// ---------------------------------------------------------------------------
document.getElementById("btn-unwrap")?.addEventListener("click", () => {
    if (PANES.length === 0) return;  // no panes loaded yet
    const wasUnwrapped = state.unwrap;
    state.unwrap = !state.unwrap;
    const btn = document.getElementById("btn-unwrap");
    if (btn) {
        btn.classList.toggle("active", state.unwrap);
        btn.textContent = state.unwrap ? "Wrap" : "Unwrap";
        btn.title = state.unwrap
            ? "Wrap single-pane tabs back into tab groups"
            : "Unwrap multi-pane tabs into single-pane tabs";
    }
    rebuildLayout(wasUnwrapped);
});

// ---------------------------------------------------------------------------
// Toolbar — timestamp mode toggle (cycles absolute/relative/hidden)
// ---------------------------------------------------------------------------
(function () {
    const btn = document.getElementById("btn-timestamp-mode");
    if (!btn) return;

    function update() {
        const current = state.timestampMode;
        const next = current === "absolute" ? "relative" : current === "relative" ? "hidden" : "absolute";
        const hasLines = PANES.some(id => (state.rawLines[id] || []).length > 0);
        const canSwitch = canDisplayTimestampMode(next) || !hasLines;
        btn.textContent = current === "hidden" ? "No time" : current === "relative" ? "Relative" : "Absolute";
        btn.title = canSwitch
            ? `Switch timestamps to ${next === "hidden" ? "No time" : next}`
            : `${next} timestamps are unavailable for the current data`;
        btn.disabled = !canSwitch;
        btn.classList.toggle("active", current === "relative");
    }

    btn.addEventListener("click", () => {
        const next = state.timestampMode === "absolute" ? "relative"
            : state.timestampMode === "relative" ? "hidden" : "absolute";
        setTimestampMode(next);
        update();
    });

    window.__embedLogUpdateTimestampModeUi = update;
    update();
})();
// ---------------------------------------------------------------------------
// Settings panel — clear cached session (localStorage restore cache)
// ---------------------------------------------------------------------------
(function () {
    const panel = document.getElementById("settings-panel");
    if (!panel) return;

    const sep = document.createElement("span");
    sep.className = "set-sep";
    sep.textContent = "|";

    const btn = document.createElement("button");
    btn.id = "btn-clear-cache";
    btn.title = "Clear refresh cache (kept logs/layout in this browser)";
    btn.textContent = "Clear cache";

    btn.addEventListener("click", () => {
        window.__embedLogClearCache?.();
        const prev = btn.textContent;
        btn.textContent = "Cache cleared";
        btn.disabled = true;
        setTimeout(() => {
            btn.textContent = prev;
            btn.disabled = false;
        }, 1200);
    });

    panel.appendChild(sep);
    panel.appendChild(btn);
})();
// ---------------------------------------------------------------------------
// Settings panel — download raw logs (merged or per pane)
// ---------------------------------------------------------------------------
(function () {
    const panel = document.getElementById("settings-panel");
    if (!panel) return;

    const sep = document.createElement("span");
    sep.className = "set-sep";
    sep.textContent = "|";

    const btn = document.createElement("button");
    btn.id = "btn-download-raw";
    btn.title = "Download all logs as merged raw text file";
    btn.textContent = "Download raw";

    panel.appendChild(sep);
    panel.appendChild(btn);
})();

// ---------------------------------------------------------------------------
// Toolbar — theme toggle (light/dark quick switch)
// Detailed palette selection lives in the settings panel.
// ---------------------------------------------------------------------------
(function () {
    const btn = document.getElementById("btn-theme");

    function themeMgr() {
        return window.__embedLogTheme;
    }

    function syncIcon() {
        const mgr = themeMgr();
        const isDark = mgr?.isDark ? mgr.isDark() : (document.documentElement.getAttribute("data-theme") === "");
        btn.textContent = isDark ? "☀" : "🌙";
    }

    btn.addEventListener("click", () => {
        const mgr = themeMgr();
        if (mgr?.toggle) mgr.toggle();
        else {
            const isDark = document.documentElement.getAttribute("data-theme") === "";
            document.documentElement.setAttribute("data-theme", isDark ? "whitesand" : "");
        }
        syncIcon();
    });

    const tryBindThemeEvents = () => {
        const mgr = themeMgr();
        if (!mgr?.onChange) return false;
        mgr.onChange(syncIcon);
        return true;
    };

    if (!tryBindThemeEvents()) {
        window.addEventListener("embedlog-theme-ready", () => {
            tryBindThemeEvents();
            syncIcon();
        }, { once: true });
    }

    syncIcon();
})();

// ---------------------------------------------------------------------------
// Pane swapping (dynamic layout)
// ---------------------------------------------------------------------------
function _findPaneLocation(paneId) {
    for (let tabIdx = 0; tabIdx < TABS.length; tabIdx++) {
        const paneIdx = TABS[tabIdx].panes.indexOf(paneId);
        if (paneIdx !== -1) return { tabIdx, paneIdx };
    }
    return null;
}

function _rebuildTabContent(tabIdx, paneElMap) {
    const tabContent = document.getElementById("tab-content-" + tabIdx);
    if (!tabContent) return;

    const paneIds = TABS[tabIdx].panes;
    const paneEls = paneIds
        .map(paneId => paneElMap[paneId] || document.getElementById("pane-" + paneId))
        .filter(Boolean);

    tabContent.innerHTML = "";
    paneEls.forEach((paneEl, i) => {
        if (i > 0) {
            const splitter = document.createElement("div");
            splitter.className = "splitter";
            tabContent.appendChild(splitter);
        }
        // Reset manual split sizing so the new placement lays out cleanly.
        paneEl.style.width = "";
        paneEl.style.flex = "";
        tabContent.appendChild(paneEl);
    });
}

function _pulsePane(paneId) {
    const el = document.getElementById("pane-" + paneId);
    if (!el) return;
    el.classList.remove("swap-anim");
    // Force reflow so repeated swaps retrigger animation
    void el.offsetWidth;
    el.classList.add("swap-anim");
    setTimeout(() => el.classList.remove("swap-anim"), 320);
}

function _swapPanes(a, b) {
    if (!a || !b || a === b) return;
    const locA = _findPaneLocation(a);
    const locB = _findPaneLocation(b);
    if (!locA || !locB) return;

    // Snapshot pane DOM nodes BEFORE any tab content is rebuilt.
    const paneElMap = {};
    PANES.forEach(id => {
        const el = document.getElementById("pane-" + id);
        if (el) paneElMap[id] = el;
    });

    TABS[locA.tabIdx].panes[locA.paneIdx] = b;
    TABS[locB.tabIdx].panes[locB.paneIdx] = a;

    [...new Set([locA.tabIdx, locB.tabIdx])].forEach(tabIdx => _rebuildTabContent(tabIdx, paneElMap));
    _pulsePane(a);
    _pulsePane(b);
    window.__embedLogSchedulePersist?.();
}

function _buildSwapTargetOptions(select, fromPaneId) {
    select.innerHTML = "";
    const placeholder = document.createElement("option");
    placeholder.value = "";
    placeholder.textContent = "swap with…";
    placeholder.selected = true;
    select.appendChild(placeholder);

    PANES.forEach(otherId => {
        if (otherId === fromPaneId) return;
        const loc = _findPaneLocation(otherId);
        const tabLabel = loc ? TABS[loc.tabIdx].label : "?";
        const opt = document.createElement("option");
        opt.value = otherId;
        opt.textContent = `${paneLabel(otherId)} · ${tabLabel}`;
        select.appendChild(opt);
    });
}

// Hover popup on tab labels — compact place for pane-swap controls.
(function _setupTabSwapPopup() {
    const tabBar = document.getElementById("tab-bar");
    if (!tabBar) return;

    const menu = document.createElement("div");
    menu.id = "tab-swap-menu";
    document.body.appendChild(menu);

    let hideTimer = null;

    function hideMenu() {
        menu.classList.remove("open");
    }

    function scheduleHide() {
        clearTimeout(hideTimer);
        hideTimer = setTimeout(hideMenu, 40);
    }

    function cancelHide() {
        clearTimeout(hideTimer);
    }

    function showForButton(btn) {
        const tabIdx = Number(btn.dataset.tabIdx);
        if (!Number.isInteger(tabIdx) || !TABS[tabIdx]) return;

        const panes = TABS[tabIdx].panes;
        menu.innerHTML = "";

        const title = document.createElement("div");
        title.className = "tab-swap-title";
        title.textContent = `Swap panes in “${TABS[tabIdx].label}”`;
        menu.appendChild(title);

        panes.forEach(paneId => {
            const row = document.createElement("div");
            row.className = "tab-swap-row";

            const name = document.createElement("span");
            name.className = "tab-swap-pane-name";
            name.textContent = paneLabel(paneId);

            const select = document.createElement("select");
            select.className = "tab-swap-select";
            _buildSwapTargetOptions(select, paneId);
            select.addEventListener("change", () => {
                const target = select.value;
                if (!target) return;
                _swapPanes(paneId, target);
                showForButton(btn); // refresh options + tab membership labels
            });

            row.appendChild(name);
            row.appendChild(select);
            menu.appendChild(row);
        });

        const rect = btn.getBoundingClientRect();
        menu.style.left = `${Math.max(8, rect.left)}px`;
        menu.style.top = `${rect.bottom + 6}px`;
        menu.classList.add("open");
    }

    tabBar.addEventListener("mouseover", ev => {
        const btn = ev.target.closest(".tab-btn");
        if (!btn || btn.classList.contains("tab-add")) return;
        cancelHide();
        showForButton(btn);
    });

    tabBar.addEventListener("mouseout", ev => {
        const fromBtn = ev.target.closest(".tab-btn");
        if (!fromBtn) return;
        if (menu.contains(ev.relatedTarget)) return;
        scheduleHide();
    });

    menu.addEventListener("mouseenter", cancelHide);
    menu.addEventListener("mouseleave", scheduleHide);
})();

// ---------------------------------------------------------------------------
// Filter inputs
// ---------------------------------------------------------------------------
export function _uiSetupPane(id) {
    const input = document.querySelector(`.filter-input[data-pane="${id}"]`);
    if (input) {
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
                    // Keep the last valid filter while showing the error —
                    // don't clear it, so the user can fix the regex without
                    // losing their filtering context.
                    input.classList.add("invalid");
                }
            }
            rerenderPane(id);
        });
    }


}
PANES.forEach(_uiSetupPane);

// ---------------------------------------------------------------------------
// Serial TX input — Enter or Send button
// wsSend is provided by ws.js in live mode, or stubbed in static exports.
// ---------------------------------------------------------------------------
function sendSerial(paneId) {
    const input = document.getElementById("input-" + paneId);
    if (!input) return;
    const text  = input.value.trim();
    if (!text) return;
    input.value = "";
    window.wsSend?.({ cmd: "send_raw", id: paneId, data: text + "\r" });
}

export function _uiSetupTxPane(id) {
    const input = document.getElementById("input-" + id);
    if (!input) return;

    const commands = window.__embedLogPaneCommands?.[id] || [];
    const row = input.closest(".input-row");
    const hint = document.createElement("span");
    hint.className = "tx-hint";
    hint.setAttribute("aria-live", "polite");
    if (row) row.appendChild(hint);

    // per-pane send history (most recent last)
    if (!window.__embedLogTxHistory) window.__embedLogTxHistory = {};
    if (!window.__embedLogTxHistIdx) window.__embedLogTxHistIdx = {};
    const histKey = id;

    let matches = [];       // indexes into commands[] matching typed prefix
    let hintIdx = -1;       // which match the ghost hint shows (was 0; now -1 = no hint)
    let _suppressInput = false;  // prevent input-event re-trigger when Tab fills value

    function fuzzyMatch(typed) {
        if (!typed || !commands.length) return [];
        const lower = typed.toLowerCase();
        return commands
            .map((cmd, i) => ({ cmd, i, score: cmd.toLowerCase().indexOf(lower) }))
            .filter(m => m.score >= 0)
            .sort((a, b) => a.score - b.score || a.cmd.length - b.cmd.length)
            .map(m => m.i);
    }

    function showHint() {
        if (hintIdx >= 0 && hintIdx < matches.length) {
            hint.textContent = commands[matches[hintIdx]];
            hint.classList.add("visible");
        } else {
            hint.classList.remove("visible");
        }
    }

    function resetHistoryIndex() {
        window.__embedLogTxHistIdx[histKey] = -1;
    }

    // ── typing → fuzzy match saved commands, show ghost hint ──────
    input.addEventListener("input", () => {
        if (_suppressInput) return;
        const typed = input.value;
        matches = fuzzyMatch(typed);
        hintIdx = matches.length > 0 ? 0 : -1;
        showHint();
        resetHistoryIndex();
    });

    // ── keydown ───────────────────────────────────────────────────
    input.addEventListener("keydown", e => {
        const history = window.__embedLogTxHistory[histKey] || [];

        // Escape → hide hint
        if (e.key === "Escape") {
            hintIdx = -1; matches = []; showHint();
            return;
        }

        // Tab / Shift+Tab → cycle through command suggestions, filling the input
        if (e.key === "Tab" && commands.length > 0) {
            e.preventDefault();

            if (matches.length === 0) {
                // No matches yet (empty input or no fuzzy match) — show all commands
                matches = commands.map((_, i) => i);
                hintIdx = e.shiftKey ? matches.length - 1 : 0;
            }

            // Fill input with the currently-selected command
            _suppressInput = true;
            input.value = commands[matches[hintIdx]];
            _suppressInput = false;
            input.setSelectionRange(input.value.length, input.value.length);

            // Advance hintIdx for next Tab (wrapping)
            if (e.shiftKey) {
                hintIdx = hintIdx <= 0 ? matches.length - 1 : hintIdx - 1;
            } else {
                hintIdx = (hintIdx + 1) % matches.length;
            }
            showHint();
            resetHistoryIndex();
            return;
        }

        // ↓/↑ → history navigation
        if (e.key === "ArrowDown" || e.key === "ArrowUp") {
            if (!history.length) return;

            let idx = window.__embedLogTxHistIdx[histKey];
            // If no history navigation in progress, start now
            if (idx < 0) {
                idx = e.key === "ArrowUp" ? history.length - 1 : 0;
            } else {
                const step = e.key === "ArrowDown" ? 1 : -1;
                idx += step;
                if (idx >= history.length) idx = 0;
                if (idx < 0) idx = history.length - 1;
            }
            e.preventDefault();
            window.__embedLogTxHistIdx[histKey] = idx;
            input.value = history[idx];
            matches = []; hintIdx = -1; showHint();
            // Place cursor at end
            input.setSelectionRange(input.value.length, input.value.length);
            return;
        }

        // Any other key resets history navigation
        resetHistoryIndex();
    });

    // ── send ─────────────────────────────────────────────────────
    function sendCurrent() {
        const text = input.value.trim();
        if (!text) return;
        input.value = "";
        matches = []; hintIdx = -1; showHint(); resetHistoryIndex();

        // Record in history (dedup consecutive, cap at 50)
        const history = window.__embedLogTxHistory[histKey] || [];
        if (!history.length || history[history.length - 1] !== text) {
            history.push(text);
            if (history.length > 50) history.shift();
        }
        window.__embedLogTxHistory[histKey] = history;

        window.wsSend?.({ cmd: "send_raw", id, data: text + "\r" });
    }

    // Enter → send
    input.addEventListener("keydown", e => {
        if (e.key === "Enter") { e.preventDefault(); sendCurrent(); }
    });

    // Send button
    const sendBtn = document.querySelector(`.send-btn[data-pane="${id}"]`);
    if (sendBtn) sendBtn.addEventListener("click", sendCurrent);
}

if (window.__embedLogProfile?.capabilities?.tx) {
    PANES.forEach(_uiSetupTxPane);
}

// ---------------------------------------------------------------------------
// Splitter drag — delegated, with pointer + mouse + touch fallback
// (Safari/macOS trackpad friendly)
// ---------------------------------------------------------------------------
(function setupSplitterDrag() {
    function findNeighborPanes(splitter) {
        const tabContent = splitter.parentElement;
        let paneLeft = null, paneRight = null, passed = false;
        for (const child of tabContent.children) {
            if (child === splitter) { passed = true; continue; }
            if (child.classList.contains("pane")) {
                if (!passed) paneLeft = child;
                else if (!paneRight) paneRight = child;
            }
        }
        return { tabContent, paneLeft, paneRight };
    }

    function eventX(ev) {
        if (ev.touches && ev.touches[0]) return ev.touches[0].clientX;
        if (ev.changedTouches && ev.changedTouches[0]) return ev.changedTouches[0].clientX;
        return ev.clientX;
    }

    function startDrag(splitter, ev) {
        const { tabContent, paneLeft, paneRight } = findNeighborPanes(splitter);
        if (!paneLeft || !paneRight) return;

        ev.preventDefault();
        splitter.classList.add("dragging");
        document.body.style.cursor = "col-resize";

        const startX = eventX(ev);
        const startLeftW = paneLeft.getBoundingClientRect().width;
        const totalW = tabContent.getBoundingClientRect().width - splitter.offsetWidth;

        function onMove(moveEv) {
            moveEv.preventDefault();
            const x = eventX(moveEv);
            const newLeft = Math.min(Math.max(startLeftW + x - startX, 120), totalW - 120);
            paneLeft.style.flex = "none";
            paneRight.style.flex = "none";
            paneLeft.style.width = newLeft + "px";
            paneRight.style.width = (totalW - newLeft) + "px";
        }

        function onEnd() {
            splitter.classList.remove("dragging");
            document.body.style.cursor = "";
            window.removeEventListener("pointermove", onMove);
            window.removeEventListener("pointerup", onEnd);
            window.removeEventListener("pointercancel", onEnd);
            window.removeEventListener("mousemove", onMove);
            window.removeEventListener("mouseup", onEnd);
            window.removeEventListener("touchmove", onMove);
            window.removeEventListener("touchend", onEnd);
            window.removeEventListener("touchcancel", onEnd);
        }

        // Register all move/end listeners; whichever event model fires will work.
        window.addEventListener("pointermove", onMove);
        window.addEventListener("pointerup", onEnd);
        window.addEventListener("pointercancel", onEnd);
        window.addEventListener("mousemove", onMove);
        window.addEventListener("mouseup", onEnd);
        window.addEventListener("touchmove", onMove, { passive: false });
        window.addEventListener("touchend", onEnd);
        window.addEventListener("touchcancel", onEnd);
    }

    document.addEventListener("pointerdown", ev => {
        const splitter = ev.target.closest(".splitter");
        if (!splitter) return;
        startDrag(splitter, ev);
    });

    document.addEventListener("mousedown", ev => {
        const splitter = ev.target.closest(".splitter");
        if (!splitter) return;
        startDrag(splitter, ev);
    });

    document.addEventListener("touchstart", ev => {
        const splitter = ev.target.closest(".splitter");
        if (!splitter) return;
        startDrag(splitter, ev);
    }, { passive: false });
})();
