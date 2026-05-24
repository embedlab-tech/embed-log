import { state, TABS, PANES } from './state.js';
import { clearPane, rerenderPane } from './lines.js';
import { rebuildLayout } from './tabcreate.js';



// ---------------------------------------------------------------------------
// Toolbar — UNWRAP toggle
// ---------------------------------------------------------------------------
document.getElementById("btn-unwrap")?.addEventListener("click", () => {
    if (PANES.length === 0) return;  // no panes loaded yet
    const wasUnwrapped = state.unwrap;
    state.unwrap = !state.unwrap;
    document.getElementById("btn-unwrap")?.classList.toggle("active", state.unwrap);
    rebuildLayout(wasUnwrapped);
});

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
// Settings panel — sessions list popup + current session links
// Save HTML action is in the main toolbar (reused former Sync button).
// ---------------------------------------------------------------------------
(function () {
    if (!window.__embedLogProfile?.capabilities?.sessionApi) return;
    
    const panel = document.getElementById("settings-panel");
    if (!panel) return;

    // Create panel buttons for server-side actions (formerly in toolbar)
    const btnSave = document.createElement("button");
    btnSave.id = "btn-save-to-server";
    btnSave.title = "Generate/refresh session HTML on the backend server";
    btnSave.textContent = "Save to server";

    const sep1 = document.createElement("span");
    sep1.className = "set-sep";
    sep1.textContent = "|";

    const btnCleanSession = document.createElement("button");
    btnCleanSession.id = "btn-clean-session";
    btnCleanSession.title = "Save current session HTML and start a clean session";
    btnCleanSession.textContent = "Clean session";

    const sep2 = document.createElement("span");
    sep2.className = "set-sep";
    sep2.textContent = "|";

    const btnCurrent = document.createElement("button");
    btnCurrent.id = "btn-current-session";
    btnCurrent.title = "Open current session HTML";
    btnCurrent.textContent = "Current HTML";

    const btn = document.createElement("button");
    btn.id = "btn-sessions";
    btn.title = "Browse saved sessions";
    btn.textContent = "Sessions";

    panel.appendChild(btnSave);
    panel.appendChild(sep1);
    panel.appendChild(btnCleanSession);
    panel.appendChild(sep2);
    panel.appendChild(btnCurrent);
    panel.appendChild(btn);

    const menu = document.createElement("div");
    menu.id = "sessions-menu";
    menu.innerHTML = `<div class="sessions-head">Saved sessions</div><div class="sessions-body">Loading…</div>`;
    document.body.appendChild(menu);

    let currentSession = null;

    function updateCurrentButtons() {
        const status = currentSession?.html_status || (currentSession?.html_ready ? "ready" : "pending");

        if (btnSave) {
            if (status === "updating") {
                btnSave.disabled = true;
                btnSave.textContent = "Saving…";
            } else {
                btnSave.disabled = false;
                btnSave.textContent = "Save HTML";
            }
        }

        if (btnCleanSession) {
            btnCleanSession.disabled = status === "updating";
            btnCleanSession.textContent = status === "updating" ? "Rotating…" : "Clean session";
        }

        if (currentSession?.html_ready && currentSession?.html) {
            btnCurrent.disabled = false;
            btnCurrent.textContent = "Current HTML";
            btnCurrent.title = currentSession?.html_updated_at
                ? `Open current session HTML (updated ${currentSession.html_updated_at})`
                : "Open current session HTML";
        } else {
            btnCurrent.disabled = true;
            btnCurrent.textContent = status === "error" ? "HTML error" : "No HTML yet";
            btnCurrent.title = status === "error"
                ? (currentSession?.html_error || "Last HTML export failed")
                : "Generate HTML first";
        }
    }

    async function refreshCurrentSession() {
        try {
            const res = await fetch("/api/session/current", { cache: "no-store" });
            if (!res.ok) throw new Error(String(res.status));
            currentSession = await res.json();
            updateCurrentButtons();
        } catch {
            currentSession = null;
            if (btnSave) btnSave.disabled = true;
            btnCurrent.disabled = true;
        }
    }

    async function saveCurrentSessionHtml() {
        if (btnSave) {
            btnSave.disabled = true;
            btnSave.textContent = "Saving…";
        }
        try {
            const res = await fetch("/api/session/export", {
                method: "POST",
                cache: "no-store",
            });
            if (!res.ok) throw new Error(String(res.status));
            const data = await res.json();
            if (data?.session) {
                currentSession = data.session;
            }
        } catch {
            const prev = btnSave ? btnSave.textContent : "Save HTML";
            if (btnSave) btnSave.textContent = "Save failed";
            setTimeout(() => {
                if (btnSave) btnSave.textContent = prev;
                updateCurrentButtons();
            }, 1200);
            return;
        }
        updateCurrentButtons();
        if (menu.classList.contains("open")) loadSessions();
    }

    function openCurrentSessionHtml() {
        if (!currentSession?.html_ready || !currentSession?.html) return;
        window.open(currentSession.html, "_blank", "noopener");
    }

    async function createCleanSession() {
        if (!window.confirm("Save current session HTML and start a clean session?")) return;
        if (btnCleanSession) {
            btnCleanSession.disabled = true;
            btnCleanSession.textContent = "Rotating…";
        }
        try {
            const res = await fetch("/api/session/rotate", {
                method: "POST",
                cache: "no-store",
            });
            if (!res.ok) throw new Error(String(res.status));
            const data = await res.json();
            if (data?.session) currentSession = data.session;
            PANES.forEach(paneId => clearPane(paneId));
            state.syncTs = null;
            state.syncTabSwitch = false;
            window.__embedLogSetSession?.(data?.session || null);
            window.__embedLogSchedulePersist?.();
            updateCurrentButtons();
            if (menu.classList.contains("open")) loadSessions();
        } catch {
            if (btnCleanSession) {
                btnCleanSession.textContent = "Rotate failed";
                setTimeout(updateCurrentButtons, 1200);
            }
        }
    }

    btnSave?.addEventListener("click", saveCurrentSessionHtml);
    btnCleanSession?.addEventListener("click", createCleanSession);
    btnCurrent.addEventListener("click", openCurrentSessionHtml);

    async function loadSessions() {
        const body = menu.querySelector(".sessions-body");
        body.textContent = "Loading…";
        try {
            const res = await fetch("/api/sessions", { cache: "no-store" });
            if (!res.ok) throw new Error(String(res.status));
            const data = await res.json();
            const sessions = Array.isArray(data.sessions) ? data.sessions : [];
            const current = data.current || null;

            if (!sessions.length) {
                body.textContent = "No sessions found.";
                return;
            }

            body.innerHTML = "";
            sessions.forEach(s => {
                const row = document.createElement("div");
                row.className = "session-row";

                const meta = document.createElement("div");
                meta.className = "session-meta";
                meta.textContent = s.started_at || s.id;

                const tags = document.createElement("div");
                tags.className = "session-tags";
                if (s.id === current) {
                    const tag = document.createElement("span");
                    tag.className = "session-tag current";
                    tag.textContent = "current";
                    tags.appendChild(tag);
                }
                if (s.html_status && s.html_status !== "ready") {
                    const tag = document.createElement("span");
                    tag.className = "session-tag pending";
                    tag.textContent = `html ${s.html_status}`;
                    tags.appendChild(tag);
                }

                const actions = document.createElement("div");
                actions.className = "session-actions";

                const htmlLink = document.createElement("a");
                htmlLink.target = "_blank";
                htmlLink.rel = "noopener";
                htmlLink.textContent = "open html";
                if (s.html_ready && s.html) {
                    htmlLink.href = s.html;
                } else {
                    htmlLink.href = "#";
                    htmlLink.classList.add("disabled");
                    htmlLink.addEventListener("click", ev => ev.preventDefault());
                }

                const manifestLink = document.createElement("a");
                manifestLink.target = "_blank";
                manifestLink.rel = "noopener";
                manifestLink.textContent = "manifest";
                manifestLink.href = s.manifest || "#";

                actions.appendChild(htmlLink);
                actions.appendChild(manifestLink);

                row.appendChild(meta);
                row.appendChild(tags);
                row.appendChild(actions);
                body.appendChild(row);
            });
        } catch {
            body.textContent = "Sessions API unavailable.";
        }
    }

    function openMenu() {
        loadSessions();
        const rect = btn.getBoundingClientRect();
        menu.style.left = `${Math.max(8, rect.left)}px`;
        menu.style.top = `${rect.bottom + 6}px`;
        menu.classList.add("open");
    }

    function closeMenu() {
        menu.classList.remove("open");
    }

    btn.addEventListener("click", ev => {
        ev.stopPropagation();
        if (menu.classList.contains("open")) closeMenu();
        else openMenu();
    });

    document.addEventListener("click", ev => {
        if (!menu.classList.contains("open")) return;
        if (menu.contains(ev.target) || ev.target === btn) return;
        closeMenu();
    });

    document.addEventListener("keydown", ev => {
        if (ev.key === "Escape") closeMenu();
    });

    window.__embedLogOnSessionHtmlStatus = function (msg) {
        if (!msg) return;
        currentSession = {
            ...(currentSession || {}),
            ...msg,
        };
        updateCurrentButtons();
        if (menu.classList.contains("open")) loadSessions();
    };


    refreshCurrentSession();
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
        opt.textContent = `${otherId} · ${tabLabel}`;
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
            name.textContent = paneId;

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
    window.wsSend?.({ cmd: "send_raw", id: paneId, data: text + "\n" });
}

export function _uiSetupTxPane(id) {
    const input = document.getElementById("input-" + id);
    if (!input) return;
    input.addEventListener("keydown", e => {
        if (e.key === "Enter") { e.preventDefault(); sendSerial(id); }
    });
    const sendBtn = document.querySelector(`.send-btn[data-pane="${id}"]`);
    if (sendBtn) sendBtn.addEventListener("click", () => sendSerial(id));
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
