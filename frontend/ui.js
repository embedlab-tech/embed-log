import { state, TABS, PANES, paneLabel } from './state.js';
import { rerenderPane, reanalyzePanePlugins, setTimestampMode, canDisplayTimestampMode, hidePluginOverlays, updatePaneSearch, navigatePaneSearch, paneEvents, revealRawLine, bootFilter, setBootFilter } from './lines.js';
import { PhraseSearch } from './search.js';
import {
    countLevels, hiddenLevelCount, hiddenLevels, paneLevelDefs, setLevelVisible, showAllLevels,
} from './levels.js';
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
        btn.setAttribute("aria-pressed", String(state.unwrap));
        btn.title = state.unwrap
            ? "Panes are shown as separate tabs. Click to group them back into their tabs"
            : "Show every pane of multi-pane tabs as a separate tab";
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
// Settings panel — read-only effective configuration (never raw YAML).
// ---------------------------------------------------------------------------
(function () {
    const panel = document.getElementById("settings-panel");
    if (!panel) return;
    const btn = document.createElement("button");
    btn.id = "btn-runtime-config";
    btn.textContent = "Configuration";
    btn.title = "View runtime configuration summary (requires a live server)";
    const dialog = document.createElement("dialog");
    dialog.className = "runtime-config-dialog";
    dialog.setAttribute("aria-labelledby", "runtime-config-title");
    dialog.innerHTML = '<h2 id="runtime-config-title">Runtime configuration (read-only)</h2><pre></pre><button type="button">Close</button>';
    const update = () => {
        const config = window.__embedLogRuntimeConfig;
        btn.disabled = !config;
        dialog.querySelector("pre").textContent = config ? JSON.stringify(config, null, 2) : "Configuration unavailable from this server.";
    };
    window.__embedLogUpdateRuntimeConfig = update;
    btn.addEventListener("click", () => { update(); dialog.showModal(); });
    dialog.querySelector("button").addEventListener("click", () => dialog.close());
    panel.appendChild(btn);
    document.body.appendChild(dialog);
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
// ---------------------------------------------------------------------------
// Per-pane log level filter ("Levels" button next to Highlight)
// ---------------------------------------------------------------------------
const _levelControlRefreshers = new Map(); // paneId -> refresh button label/visibility

function _closeLevelMenus(except = null) {
    document.querySelectorAll('.level-filter').forEach(wrap => {
        if (wrap === except) return;
        const menu = wrap.querySelector('.level-menu');
        if (menu && !menu.hidden) {
            menu.hidden = true;
            wrap.querySelector('.level-mode')?.setAttribute('aria-expanded', 'false');
        }
    });
}
document.addEventListener('click', event => {
    _closeLevelMenus(event.target.closest?.('.level-filter') || null);
});

function _setupLevelControl(id, after) {
    const wrap = document.createElement('span');
    wrap.className = 'level-filter';
    const button = document.createElement('button');
    button.type = 'button';
    button.className = 'level-mode';
    button.setAttribute('aria-haspopup', 'true');
    button.setAttribute('aria-expanded', 'false');
    const menu = document.createElement('div');
    menu.className = 'level-menu';
    menu.hidden = true;
    menu.setAttribute('role', 'group');
    menu.setAttribute('aria-label', 'Visible log levels');
    wrap.append(button, menu);
    after.after(wrap);

    const refreshButton = () => {
        const defs = paneLevelDefs(id);
        wrap.hidden = defs.length === 0;
        const hidden = hiddenLevelCount(id);
        button.textContent = hidden ? `Levels ${defs.length - hidden}/${defs.length}` : 'Levels';
        button.classList.toggle('filtered', hidden > 0);
        button.title = hidden
            ? `${hidden} log level${hidden === 1 ? '' : 's'} hidden in this pane`
            : 'Choose which log levels this pane shows';
    };
    const applyChange = () => {
        refreshButton();
        rerenderPane(id);
        updatePaneSearch(id);
    };
    const renderMenu = () => {
        const defs = paneLevelDefs(id);
        const counts = countLevels(id, state.rawLines[id] || []);
        const hidden = hiddenLevels(id);
        menu.replaceChildren();
        for (const def of defs) {
            const label = document.createElement('label');
            const box = document.createElement('input');
            box.type = 'checkbox';
            box.checked = !hidden.has(def.id);
            box.dataset.level = def.id;
            box.addEventListener('change', () => {
                setLevelVisible(id, def.id, box.checked);
                applyChange();
            });
            const name = document.createElement('span');
            name.className = 'level-name';
            name.textContent = def.label;
            const count = document.createElement('span');
            count.className = 'level-count';
            count.textContent = (counts.get(def.id) || 0).toLocaleString();
            label.append(box, name, count);
            menu.append(label);
        }
        const all = document.createElement('button');
        all.type = 'button';
        all.className = 'level-show-all';
        all.textContent = 'Show all';
        all.addEventListener('click', () => {
            showAllLevels(id);
            applyChange();
            renderMenu();
        });
        menu.append(all);
    };
    button.addEventListener('click', () => {
        if (menu.hidden) {
            renderMenu();
            menu.hidden = false;
            button.setAttribute('aria-expanded', 'true');
            menu.querySelector('input')?.focus();
        } else {
            menu.hidden = true;
            button.setAttribute('aria-expanded', 'false');
        }
    });
    menu.addEventListener('keydown', event => {
        if (event.key !== 'Escape') return;
        event.stopPropagation();
        menu.hidden = true;
        button.setAttribute('aria-expanded', 'false');
        button.focus();
    });
    _levelControlRefreshers.set(id, refreshButton);
    refreshButton();
}

// Level definitions arrive with the server config, possibly after panes exist.
window.__embedLogRefreshLevelControls = () => {
    for (const refresh of _levelControlRefreshers.values()) refresh();
};

// ---------------------------------------------------------------------------
// Device events: restart / crash counter in the pane header
// ---------------------------------------------------------------------------
const EVENT_LABELS = { boot: "Restart", crash: "Crash" };
const EVENT_ICONS = { boot: "\u27f3", crash: "\u2716" };
const MAX_LISTED_EVENTS = 300;
const _eventBadges = new Map(); // paneId -> { refresh, menu }

function _paneEventSources(paneId) {
    const merge = (window.__embedLogMerges || []).find(m => m && m.name === paneId);
    return merge && Array.isArray(merge.of) ? merge.of : [paneId];
}

/**
 * Session-wide counts from the server in live mode; counted from the loaded
 * lines in exported reports (which contain the whole session).
 */
function _paneEventCounts(paneId) {
    const server = window.__embedLogSourceEvents;
    if (server && typeof server === "object") {
        const counts = { boot: 0, crash: 0 };
        for (const source of _paneEventSources(paneId)) {
            counts.boot += Number(server[source]?.boot) || 0;
            counts.crash += Number(server[source]?.crash) || 0;
        }
        return counts;
    }
    const counts = { boot: 0, crash: 0 };
    for (const { event } of paneEvents(paneId)) counts[event]++;
    return counts;
}

function _closeEventMenus(except = null) {
    document.querySelectorAll(".pane-events").forEach(wrap => {
        if (wrap === except) return;
        const menu = wrap.querySelector(".events-menu");
        if (menu && !menu.hidden) {
            menu.hidden = true;
            wrap.querySelector(".pane-events-btn")?.setAttribute("aria-expanded", "false");
        }
    });
}
document.addEventListener("click", event => {
    _closeEventMenus(event.target.closest?.(".pane-events") || null);
});

function _setupEventBadge(id) {
    const header = document.querySelector(`#pane-${id} .pane-header`);
    if (!header || header.querySelector(".pane-events")) return;
    const wrap = document.createElement("span");
    wrap.className = "pane-events";
    wrap.hidden = true;
    const button = document.createElement("button");
    button.type = "button";
    button.className = "pane-events-btn";
    button.setAttribute("aria-haspopup", "true");
    button.setAttribute("aria-expanded", "false");
    const menu = document.createElement("div");
    menu.className = "events-menu";
    menu.hidden = true;
    menu.setAttribute("role", "menu");
    menu.setAttribute("aria-label", "Device restarts and crashes");
    wrap.append(button, menu);
    (header.querySelector(".pane-stats") || header.firstElementChild)?.after(wrap);

    const renderMenu = () => {
        const counts = _paneEventCounts(id);
        const events = paneEvents(id).reverse();
        menu.replaceChildren();
        for (const item of events.slice(0, MAX_LISTED_EVENTS)) {
            const row = document.createElement("button");
            row.type = "button";
            row.className = `events-item events-${item.event}`;
            row.setAttribute("role", "menuitem");
            const icon = document.createElement("span");
            icon.className = "events-icon";
            icon.textContent = EVENT_ICONS[item.event];
            const label = document.createElement("span");
            label.className = "events-kind";
            label.textContent = EVENT_LABELS[item.event];
            const time = document.createElement("span");
            time.className = "events-time";
            time.textContent = item.ts;
            const text = document.createElement("span");
            text.className = "events-text";
            text.textContent = item.text;
            row.append(icon, label, time, text);
            row.addEventListener("click", () => {
                menu.hidden = true;
                button.setAttribute("aria-expanded", "false");
                revealRawLine(id, item.rawIndex);
            });
            const entry = document.createElement("div");
            entry.className = "events-entry";
            entry.append(row);
            if (item.event === "boot" && Number.isFinite(item.sequence)) {
                const only = document.createElement("button");
                only.type = "button";
                only.className = "events-boot-filter";
                only.textContent = "Only this boot";
                only.title = "Show only the lines from this restart until the next restart of the same device";
                only.addEventListener("click", () => {
                    menu.hidden = true;
                    button.setAttribute("aria-expanded", "false");
                    _applyBootFilter(id, item.sourceId, item.sequence);
                });
                entry.append(only);
            }
            if (item.event === "crash" && _canDecode(item)) {
                const decode = document.createElement("button");
                decode.type = "button";
                decode.className = "events-decode";
                decode.textContent = "Decode";
                decode.title = "Symbolize this crash dump with the firmware ELF";
                decode.addEventListener("click", () => {
                    menu.hidden = true;
                    button.setAttribute("aria-expanded", "false");
                    revealRawLine(id, item.rawIndex);
                    _showCrashDecode(item);
                });
                entry.append(decode);
            }
            menu.append(entry);
        }
        const missing = counts.boot + counts.crash - events.length;
        if (missing > 0 || !events.length) {
            const note = document.createElement("div");
            note.className = "events-note";
            note.textContent = missing > 0
                ? `${missing} earlier event${missing === 1 ? " is" : "s are"} older than the loaded lines`
                : "No events in the loaded lines";
            menu.append(note);
        }
    };
    const refresh = () => {
        const counts = _paneEventCounts(id);
        wrap.hidden = counts.boot + counts.crash === 0;
        button.classList.toggle("has-crash", counts.crash > 0);
        button.replaceChildren();
        for (const event of ["boot", "crash"]) {
            if (!counts[event]) continue;
            const part = document.createElement("span");
            part.className = `ev-${event}`;
            part.textContent = `${EVENT_ICONS[event]} ${counts[event].toLocaleString("en-US")}`;
            button.append(part);
        }
        const plural = (n, one, many) => `${n} ${n === 1 ? one : many}`;
        button.title = `${plural(counts.boot, "restart", "restarts")}, ${plural(counts.crash, "crash", "crashes")}`
            + " detected in this session. Click to list them.";
        if (!menu.hidden) renderMenu();
    };
    button.addEventListener("click", () => {
        if (menu.hidden) {
            renderMenu();
            menu.hidden = false;
            button.setAttribute("aria-expanded", "true");
            menu.querySelector("button")?.focus();
        } else {
            menu.hidden = true;
            button.setAttribute("aria-expanded", "false");
        }
    });
    menu.addEventListener("keydown", event => {
        if (event.key !== "Escape") return;
        event.stopPropagation();
        menu.hidden = true;
        button.setAttribute("aria-expanded", "false");
        button.focus();
    });
    _eventBadges.set(id, { refresh });
    refresh();
}

// ── Boot filter (one restart-to-restart range) ──
/** Restarts of one source in a pane, oldest first, with their sequence ranges. */
function _paneBoots(paneId, sourceId) {
    const boots = paneEvents(paneId).filter(e => e.event === "boot" && e.sourceId === sourceId && Number.isFinite(e.sequence));
    return boots.map((boot, i) => ({
        ...boot,
        number: i + 1,
        total: boots.length,
        toSequence: i + 1 < boots.length ? boots[i + 1].sequence : null,
    }));
}

function _applyBootFilter(paneId, sourceId, sequence) {
    const boot = _paneBoots(paneId, sourceId).find(b => b.sequence === sequence);
    if (!boot) return;
    setBootFilter(paneId, {
        sourceId, fromSequence: boot.sequence, toSequence: boot.toSequence,
        number: boot.number, total: boot.total, ts: boot.ts,
    });
}

function _setupBootChip(id) {
    const bar = document.querySelector(`#pane-${id} .filter-bar`);
    if (!bar || bar.querySelector(".boot-chip")) return;
    const chip = document.createElement("span");
    chip.className = "boot-chip";
    chip.hidden = true;
    chip.innerHTML = '<button type="button" class="boot-prev" aria-label="Previous boot" title="Previous boot">\u2039</button>'
        + '<span class="boot-label"></span>'
        + '<button type="button" class="boot-next" aria-label="Next boot" title="Next boot">\u203a</button>'
        + '<button type="button" class="boot-clear" aria-label="Show all boots" title="Show all boots">\u2715</button>';
    bar.append(chip);
    const step = direction => {
        const filter = bootFilter(id);
        if (!filter) return;
        const boots = _paneBoots(id, filter.sourceId);
        const at = boots.findIndex(b => b.sequence === filter.fromSequence);
        const next = boots[at + direction];
        if (next) _applyBootFilter(id, filter.sourceId, next.sequence);
    };
    chip.querySelector(".boot-prev").addEventListener("click", () => step(-1));
    chip.querySelector(".boot-next").addEventListener("click", () => step(1));
    chip.querySelector(".boot-clear").addEventListener("click", () => setBootFilter(id, null));
    _bootChips.set(id, () => {
        const filter = bootFilter(id);
        chip.hidden = !filter;
        if (!filter) return;
        const boots = _paneBoots(id, filter.sourceId);
        const at = boots.findIndex(b => b.sequence === filter.fromSequence);
        const merged = _paneEventSources(id).length > 1 ? `${paneLabel(filter.sourceId)} ` : "";
        chip.querySelector(".boot-label").textContent =
            `\u27f3 ${merged}boot ${at + 1}/${boots.length} \u00b7 ${filter.ts}${filter.toSequence === null ? " \u2192 now" : ""}`;
        chip.querySelector(".boot-prev").disabled = at <= 0;
        chip.querySelector(".boot-next").disabled = at < 0 || at >= boots.length - 1;
    });
    _bootChips.get(id)();
}

const _bootChips = new Map();
window.__embedLogOnBootFilterChange = paneId => _bootChips.get(paneId)?.();

// ── Crash decoding with the firmware ELF (live server only) ──
function _canDecode(item) {
    const session = window.__embedLogSession;
    return !!(session?.id && Number.isFinite(item.sequence) && session.symbols?.[item.sourceId]);
}

function _crashDialog() {
    let dialog = document.getElementById("crash-dialog");
    if (dialog) return dialog;
    dialog = document.createElement("dialog");
    dialog.id = "crash-dialog";
    dialog.innerHTML = '<div class="crash-head"><span class="crash-title"></span>'
        + '<button type="button" class="crash-close" aria-label="Close">\u2715</button></div>'
        + '<div class="crash-meta"></div><div class="crash-body"></div>';
    dialog.querySelector(".crash-close").addEventListener("click", () => dialog.close());
    dialog.addEventListener("click", event => { if (event.target === dialog) dialog.close(); });
    document.body.append(dialog);
    return dialog;
}

function _frameLocation(frame) {
    if (!frame.file) return "??";
    const file = frame.file.split(/[\\/]/).slice(-2).join("/");
    return frame.line ? `${file}:${frame.line}` : file;
}

async function _showCrashDecode(item) {
    const dialog = _crashDialog();
    const label = paneLabel(item.sourceId);
    dialog.querySelector(".crash-title").textContent = `Crash on ${label}`;
    dialog.querySelector(".crash-meta").textContent = `${item.ts} \u00b7 decoding\u2026`;
    const body = dialog.querySelector(".crash-body");
    body.replaceChildren();
    if (!dialog.open) dialog.showModal();
    const session = window.__embedLogSession;
    let report;
    try {
        const response = await fetch(`/api/sessions/${encodeURIComponent(session.id)}/crashes/${item.sequence}?source=${encodeURIComponent(item.sourceId)}`, { cache: "no-store" });
        const result = await response.json().catch(() => ({}));
        if (!response.ok || !result.report) throw new Error(result.error || `HTTP ${response.status}`);
        report = result.report;
    } catch (error) {
        dialog.querySelector(".crash-meta").textContent = `${item.ts} \u00b7 cannot decode: ${error.message}`;
        return;
    }
    dialog.querySelector(".crash-meta").textContent = `${item.ts} \u00b7 record #${report.sequence} \u00b7 ELF ${report.elf}`;
    const table = document.createElement("table");
    table.className = "crash-frames";
    table.innerHTML = "<thead><tr><th>Register</th><th>Address</th><th>Function</th><th>Location</th></tr></thead>";
    const tbody = document.createElement("tbody");
    for (const address of report.addresses) {
        const frames = address.frames.length ? address.frames : [null];
        frames.forEach((frame, i) => {
            const tr = document.createElement("tr");
            if (i > 0) tr.className = "crash-inline-row";
            const cells = [
                i === 0 ? address.label : "",
                i === 0 ? `0x${address.address.toString(16).padStart(8, "0")}` : "",
                frame ? (frame.function || "??") : "?? (not in this ELF)",
                frame ? _frameLocation(frame) : "",
            ];
            cells.forEach((text, c) => {
                const td = document.createElement("td");
                td.textContent = text;
                if (c === 2 && frame?.inlined) {
                    const tag = document.createElement("span");
                    tag.className = "crash-inlined";
                    tag.textContent = "inlined";
                    td.append(" ", tag);
                }
                if (c === 3 && frame?.file) td.title = frame.file;
                tr.append(td);
            });
            tbody.append(tr);
        });
    }
    table.append(tbody);
    if (!report.addresses.length) {
        const note = document.createElement("p");
        note.className = "crash-note";
        note.textContent = "No code addresses (pc, lr, call trace) were found in this fault dump.";
        body.append(note);
    } else {
        body.append(table);
    }
    const dump = document.createElement("details");
    dump.className = "crash-dump";
    const summary = document.createElement("summary");
    summary.textContent = `Fault dump (${report.lines.length} lines)`;
    const pre = document.createElement("pre");
    pre.textContent = report.lines.join("\n");
    dump.append(summary, pre);
    body.append(dump);
}

// A filter on the latest boot ("→ now") ends when the device restarts again.
function _closeOpenBootFilter(paneId) {
    const filter = bootFilter(paneId);
    if (!filter || filter.toSequence !== null) return;
    const boot = _paneBoots(paneId, filter.sourceId).find(b => b.sequence === filter.fromSequence);
    if (boot && boot.toSequence !== null) {
        setBootFilter(paneId, { ...filter, toSequence: boot.toSequence, total: boot.total });
    }
}

window.__embedLogRefreshEventBadges = paneId => {
    for (const id of paneId ? [paneId, ...(window.__embedLogMerges || []).filter(m => m?.of?.includes(paneId)).map(m => m.name)] : _bootChips.keys()) {
        _closeOpenBootFilter(id);
        _bootChips.get(id)?.();
    }
    if (paneId) {
        _eventBadges.get(paneId)?.refresh();
        // A source's events also count in merged panes that include it.
        for (const merge of window.__embedLogMerges || []) {
            if (merge?.of?.includes(paneId)) _eventBadges.get(merge.name)?.refresh();
        }
        return;
    }
    for (const badge of _eventBadges.values()) badge.refresh();
};

const FILTER_INPUT_DEBOUNCE_MS = 150;

export function _uiSetupPane(id) {
    _setupEventBadge(id);
    _setupBootChip(id);
    const input = document.querySelector(`.filter-input[data-pane="${id}"]`);
    if (input && !input.dataset.searchInitialized) {
        input.dataset.searchInitialized = 'true';
        const mode = document.createElement('button');
        mode.type = 'button';
        mode.className = 'search-mode';
        mode.textContent = 'Highlight';
        mode.setAttribute('aria-pressed', String(!!state.searches[id]));
        mode.title = 'Find and count a literal phrase without hiding surrounding lines';
        const controls = document.createElement('span');
        controls.className = 'search-controls';
        controls.hidden = !state.searches[id];
        controls.innerHTML = '<button type="button" class="search-prev" aria-label="Previous match" title="Previous match (Shift+Enter)">▲</button><span class="search-count" role="status" aria-live="polite"></span><button type="button" class="search-next" aria-label="Next match" title="Next match (Enter)">▼</button>';
        input.after(mode, controls);
        input.setAttribute('aria-label', 'Filter or highlight phrase');
        controls.querySelector('.search-prev').addEventListener('click', () => navigatePaneSearch(id, -1));
        controls.querySelector('.search-next').addEventListener('click', () => navigatePaneSearch(id, 1));
        input.addEventListener('keydown', async event => {
            // Enter applies what was typed right away instead of after the debounce.
            const applying = !event.isComposing && event.key === 'Enter' ? flushPendingApply() : null;
            if (!event.isComposing && state.searches[id] && (event.key === 'Enter' || event.key === 'F3')) {
                event.preventDefault();
                // A new phrase may still be loading history; navigate once it is counted.
                if (applying) await applying;
                navigatePaneSearch(id, event.shiftKey ? -1 : 1);
            }
        });
        const apply = async () => {
            if (mode.getAttribute('aria-pressed') === 'true') {
                state.filters[id] = null;
                input.classList.remove('invalid');
                input.placeholder = 'Highlight phrase…';
                const search = new PhraseSearch(input.value);
                state.searches[id] = search;
                search.loading = !!search.phrase && !!window.__embedLogLoadSearchHistory;
                rerenderPane(id);
                if (search.loading) {
                    try { await window.__embedLogLoadSearchHistory(id); }
                    catch (error) { search.error = error.message; }
                    search.loading = false;
                    if (state.searches[id] === search) rerenderPane(id);
                }
                return;
            }
            state.searches[id] = null;
            input.placeholder = 'Filter (regex)…';
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
        };
        // Filtering or highlighting scans every line of the pane, so apply it
        // once typing pauses rather than on every keystroke.
        let pendingApply = null;
        function flushPendingApply() {
            if (pendingApply === null) return null;
            clearTimeout(pendingApply);
            pendingApply = null;
            return apply();
        }
        input.addEventListener('input', () => {
            clearTimeout(pendingApply);
            pendingApply = setTimeout(() => {
                pendingApply = null;
                apply();
            }, FILTER_INPUT_DEBOUNCE_MS);
        });
        mode.addEventListener('click', () => {
            clearTimeout(pendingApply);
            pendingApply = null;
            const enabled = mode.getAttribute('aria-pressed') !== 'true';
            mode.setAttribute('aria-pressed', String(enabled));
            controls.hidden = !enabled;
            apply();
        });
        _setupLevelControl(id, controls);
        updatePaneSearch(id);
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
