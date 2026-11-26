import { state, TABS, PANES, paneLabel } from './state.js';
import { rerenderPane, scrollPaneToBottom, reanalyzePanePlugins, setTimestampMode, canDisplayTimestampMode, hidePluginOverlays, updatePaneSearch, navigatePaneSearch } from './lines.js';
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

    _ensureConfiguredLayout();
    TABS[locA.tabIdx].panes[locA.paneIdx] = b;
    TABS[locB.tabIdx].panes[locB.paneIdx] = a;

    // Same path as add/remove: re-flow, refresh the ✕ buttons, save the layout.
    const tabs = [...new Set([locA.tabIdx, locB.tabIdx])];
    _commitLayout(tabs, tabs.flatMap(i => TABS[i].panes));
    _pulsePane(a);
    _pulsePane(b);
}

// ---------------------------------------------------------------------------
// Pane add / remove (dynamic layout)
//
// A pane that is not in any view is "hidden": its element is parked in
// #hidden-panes (display: none) and keeps receiving and storing its lines, so
// it shows its full history when it is added back. Hiding only changes the
// layout; the source keeps recording.
// ---------------------------------------------------------------------------
const _LAYOUT_KEY = "embed-log:layout:v1:";
let _configuredLayout = null;

const _layoutSnapshot = () => TABS.map(tab => ({ label: tab.label, panes: [...tab.panes] }));
const _ensureConfiguredLayout = () => { _configuredLayout ||= _layoutSnapshot(); };
// The saved layout belongs to this configuration: same views with the same panes.
const _layoutKey = () => _LAYOUT_KEY + JSON.stringify(
    (_configuredLayout || []).map(tab => [tab.label, [...tab.panes].sort()]),
);
const _sameLayout = (a, b) => JSON.stringify(a) === JSON.stringify(b);

function _hiddenPanes() {
    return PANES.filter(id => !_findPaneLocation(id));
}

function _canRemovePane(paneId) {
    const loc = _findPaneLocation(paneId);
    return !!loc && TABS[loc.tabIdx].panes.length > 1;
}

function _parkingEl() {
    let el = document.getElementById("hidden-panes");
    if (!el) {
        el = document.createElement("div");
        el.id = "hidden-panes";
        document.getElementById("container")?.appendChild(el);
    }
    return el;
}

export function refreshPaneControls() {
    PANES.forEach(id => {
        const button = document.querySelector(`#pane-${id} .pane-close-btn`);
        if (button) button.hidden = state.unwrap || !_canRemovePane(id);
    });
}

function _saveLayout() {
    if (window.__embedLogProfile?.capabilities?.persistCache !== true || !_configuredLayout) return;
    try {
        const key = _layoutKey();
        if (_sameLayout(_layoutSnapshot(), _configuredLayout)) localStorage.removeItem(key);
        else localStorage.setItem(key, JSON.stringify(_layoutSnapshot()));
    } catch { /* storage unavailable: the layout just does not persist */ }
}

// Apply a layout that was already written to TABS: move elements, re-flow, keep scroll.
function _commitLayout(tabIdxs, paneIds) {
    const paneElMap = {};
    PANES.forEach(id => {
        const el = document.getElementById("pane-" + id);
        if (el) paneElMap[id] = el;
    });
    const scrollTops = new Map();
    paneIds.forEach(id => {
        const el = document.getElementById("log-" + id);
        if (el) scrollTops.set(id, el.scrollTop);
    });

    const parking = _parkingEl();
    _hiddenPanes().forEach(id => { if (paneElMap[id]) parking.appendChild(paneElMap[id]); });
    tabIdxs.forEach(tabIdx => _rebuildTabContent(tabIdx, paneElMap));
    refreshPaneControls();

    // Moving an element resets its scroll position.
    paneIds.forEach(id => {
        if (state.atBottom[id] !== false) scrollPaneToBottom(id);
        else {
            const el = document.getElementById("log-" + id);
            if (el && scrollTops.has(id)) el.scrollTop = scrollTops.get(id);
        }
    });
    window.__embedLogSchedulePersist?.();
    _saveLayout();
}

function _removePane(paneId) {
    const loc = _findPaneLocation(paneId);
    if (!loc || !_canRemovePane(paneId)) return;
    _ensureConfiguredLayout();
    TABS[loc.tabIdx].panes.splice(loc.paneIdx, 1);
    _commitLayout([loc.tabIdx], TABS[loc.tabIdx].panes);
}

// From the hidden panes, or moved here from another view that keeps at least one pane.
function _addPane(tabIdx, paneId) {
    if (!TABS[tabIdx] || TABS[tabIdx].panes.includes(paneId)) return;
    const from = _findPaneLocation(paneId);
    if (from && !_canRemovePane(paneId)) return;
    _ensureConfiguredLayout();
    if (from) TABS[from.tabIdx].panes.splice(from.paneIdx, 1);
    TABS[tabIdx].panes.push(paneId);
    const tabs = [...new Set([tabIdx, from?.tabIdx].filter(i => i !== undefined))];
    _commitLayout(tabs, tabs.flatMap(i => TABS[i].panes));
    _pulsePane(paneId);
}

function _resetLayout() {
    if (!_configuredLayout) return;
    _configuredLayout.forEach((tab, i) => { if (TABS[i]) TABS[i].panes = [...tab.panes]; });
    _commitLayout(TABS.map((_, i) => i), PANES);
}

function _layoutIsCustom() {
    return !!_configuredLayout && !_sameLayout(_layoutSnapshot(), _configuredLayout);
}

// Called by ws.js once the configured views exist: restore this browser's layout.
window.__embedLogApplySavedLayout = function __embedLogApplySavedLayout() {
    _configuredLayout = _layoutSnapshot();
    if (window.__embedLogProfile?.capabilities?.persistCache !== true) return;
    let saved = null;
    try { saved = JSON.parse(localStorage.getItem(_layoutKey()) || "null"); } catch { saved = null; }
    const ids = Array.isArray(saved) ? saved.flatMap(tab => (Array.isArray(tab?.panes) ? tab.panes : [])) : [];
    const valid = Array.isArray(saved) && saved.length === TABS.length
        && saved.every((tab, i) => tab && tab.label === TABS[i].label && Array.isArray(tab.panes) && tab.panes.length > 0)
        && ids.every(id => PANES.includes(id)) && new Set(ids).size === ids.length;
    if (!valid || _sameLayout(saved.map(({ label, panes }) => ({ label, panes })), _configuredLayout)) return;
    saved.forEach((tab, i) => { TABS[i].panes = [...tab.panes]; });
    rebuildLayout();
};

// One custom listbox open at a time; a click anywhere else closes it.
let _openMenuSelect = null;
function _closeMenuSelect() {
    if (!_openMenuSelect) return;
    _openMenuSelect.menu.hidden = true;
    _openMenuSelect.trigger.setAttribute("aria-expanded", "false");
    _openMenuSelect = null;
}
document.addEventListener("pointerdown", event => {
    if (_openMenuSelect && !_openMenuSelect.wrap.contains(event.target)) _closeMenuSelect();
});
window.addEventListener("resize", _closeMenuSelect);

/**
 * Custom listbox instead of <select>, in the look of the other dropdowns.
 * groups: [{ label?, items: [{ value, name, detail? }] }]; onPick(value) runs on choice.
 */
function _createMenuSelect({ placeholder, ariaLabel, groups, onPick, disabled = false, title = "" }) {
    const wrap = document.createElement("div");
    wrap.className = "dictionary-choice dd-block";
    const trigger = document.createElement("button");
    trigger.type = "button";
    trigger.className = "dd-trigger";
    trigger.setAttribute("role", "combobox");
    trigger.setAttribute("aria-haspopup", "listbox");
    trigger.setAttribute("aria-expanded", "false");
    trigger.setAttribute("aria-label", ariaLabel);
    trigger.disabled = disabled;
    trigger.title = title;
    const text = document.createElement("span");
    text.className = "dd-trigger-name dd-placeholder";
    text.textContent = placeholder;
    const caret = document.createElement("span");
    caret.className = "dd-caret";
    caret.textContent = "▾";
    trigger.append(text, caret);

    const menu = document.createElement("ul");
    menu.className = "dd-menu";
    menu.setAttribute("role", "listbox");
    menu.hidden = true;
    groups.forEach(group => {
        if (group.label) {
            const heading = document.createElement("li");
            heading.className = "dd-group";
            heading.setAttribute("role", "presentation");
            heading.textContent = group.label;
            menu.appendChild(heading);
        }
        group.items.forEach(item => {
            const option = document.createElement("li");
            option.className = "dd-option";
            option.setAttribute("role", "option");
            option.tabIndex = -1;
            option.dataset.value = item.value;
            const top = document.createElement("span");
            top.className = "dd-option-top";
            const name = document.createElement("span");
            name.className = "dd-option-name";
            name.textContent = item.name;
            top.appendChild(name);
            if (item.detail) {
                const detail = document.createElement("span");
                detail.className = "dd-option-build";
                detail.textContent = item.detail;
                top.appendChild(detail);
            }
            option.appendChild(top);
            menu.appendChild(option);
        });
    });
    wrap.append(trigger, menu);

    const options = () => [...menu.querySelectorAll('[role="option"]')];
    const focusAt = index => {
        const list = options();
        if (list.length) list[Math.max(0, Math.min(list.length - 1, index))].focus();
    };
    const open = () => {
        if (trigger.disabled) return;
        _closeMenuSelect();
        const box = trigger.getBoundingClientRect();
        Object.assign(menu.style, { left: `${box.left}px`, top: `${box.bottom + 2}px`, minWidth: `${box.width}px` });
        menu.hidden = false;
        trigger.setAttribute("aria-expanded", "true");
        _openMenuSelect = { wrap, menu, trigger };
        focusAt(0);
    };
    trigger.addEventListener("click", () => (_openMenuSelect?.trigger === trigger ? _closeMenuSelect() : open()));
    trigger.addEventListener("keydown", event => {
        if (event.key === "ArrowDown" || event.key === "ArrowUp") { event.preventDefault(); open(); }
    });
    menu.addEventListener("click", event => {
        const option = event.target.closest('[role="option"]');
        if (!option) return;
        _closeMenuSelect();
        onPick(option.dataset.value);
    });
    menu.addEventListener("keydown", event => {
        const list = options();
        const at = list.indexOf(document.activeElement);
        if (event.key === "ArrowDown") { event.preventDefault(); focusAt(at + 1); }
        else if (event.key === "ArrowUp") { event.preventDefault(); focusAt(at - 1); }
        else if (event.key === "Home") { event.preventDefault(); focusAt(0); }
        else if (event.key === "End") { event.preventDefault(); focusAt(list.length - 1); }
        else if ((event.key === "Enter" || event.key === " ") && at >= 0) {
            event.preventDefault();
            _closeMenuSelect();
            onPick(list[at].dataset.value);
        } else if (event.key === "Escape") { event.preventDefault(); _closeMenuSelect(); trigger.focus(); }
        else if (event.key === "Tab") _closeMenuSelect();
    });
    return wrap;
}

function _swapTargetGroups(fromPaneId) {
    // Swapping is between panes that are in a view; hidden ones are brought back with "Add".
    const items = PANES.filter(id => id !== fromPaneId && _findPaneLocation(id)).map(id => ({
        value: id,
        name: paneLabel(id),
        detail: TABS[_findPaneLocation(id).tabIdx].label,
    }));
    return items.length ? [{ items }] : [];
}

function _addPaneGroups(tabIdx) {
    const hidden = _hiddenPanes().map(id => ({ value: id, name: paneLabel(id) }));
    const movable = PANES.filter(id => {
        const loc = _findPaneLocation(id);
        return loc && loc.tabIdx !== tabIdx && TABS[loc.tabIdx].panes.length > 1;
    }).map(id => ({ value: id, name: paneLabel(id), detail: TABS[_findPaneLocation(id).tabIdx].label }));
    return [
        ...(hidden.length ? [{ label: "Hidden", items: hidden }] : []),
        ...(movable.length ? [{ label: "Move from another view", items: movable }] : []),
    ];
}

// Hover popup on tab labels — compact place for pane controls (swap, add, remove).
(function _setupTabSwapPopup() {
    const tabBar = document.getElementById("tab-bar");
    if (!tabBar) return;

    const menu = document.createElement("div");
    menu.id = "tab-swap-menu";
    document.body.appendChild(menu);

    let hideTimer = null;

    function hideMenu() {
        _closeMenuSelect();
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
        _closeMenuSelect();
        menu.innerHTML = "";

        const title = document.createElement("div");
        title.className = "tab-swap-title";
        title.textContent = `Panes in “${TABS[tabIdx].label}”`;
        menu.appendChild(title);

        panes.forEach(paneId => {
            const row = document.createElement("div");
            row.className = "tab-swap-row";

            const name = document.createElement("span");
            name.className = "tab-swap-pane-name";
            name.textContent = paneLabel(paneId);

            const swapGroups = _swapTargetGroups(paneId);
            const select = _createMenuSelect({
                placeholder: "swap with…",
                ariaLabel: `Swap ${paneLabel(paneId)} with`,
                groups: swapGroups,
                disabled: !swapGroups.length,
                onPick: target => {
                    _swapPanes(paneId, target);
                    showForButton(btn); // refresh options + tab membership labels
                },
            });

            const remove = document.createElement("button");
            remove.type = "button";
            remove.className = "tab-swap-remove";
            remove.textContent = "✕";
            remove.disabled = !_canRemovePane(paneId);
            remove.title = remove.disabled
                ? "A view keeps at least one pane"
                : "Remove from this view (the source keeps recording)";
            remove.setAttribute("aria-label", `Remove ${paneLabel(paneId)} from this view`);
            remove.addEventListener("click", () => {
                _removePane(paneId);
                showForButton(btn);
            });

            row.appendChild(name);
            row.appendChild(select);
            row.appendChild(remove);
            menu.appendChild(row);
        });

        const addRow = document.createElement("div");
        addRow.className = "tab-swap-row tab-add-row";
        const addLabel = document.createElement("span");
        addLabel.className = "tab-swap-pane-name";
        addLabel.textContent = "Add";
        const addGroups = _addPaneGroups(tabIdx);
        const addSelect = _createMenuSelect({
            placeholder: "add pane…",
            ariaLabel: `Add a pane to ${TABS[tabIdx].label}`,
            groups: addGroups,
            disabled: !addGroups.length,
            title: addGroups.length ? "" : "Every source is already in this view",
            onPick: value => {
                _addPane(tabIdx, value);
                showForButton(btn);
            },
        });
        addRow.append(addLabel, addSelect);
        menu.appendChild(addRow);

        if (_layoutIsCustom()) {
            const reset = document.createElement("button");
            reset.type = "button";
            reset.className = "tab-reset-layout";
            reset.textContent = "Reset layout";
            reset.title = "Back to the configured views and panes";
            reset.addEventListener("click", () => {
                _resetLayout();
                showForButton(btn);
            });
            menu.appendChild(reset);
        }

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
    // A dropdown inside the popup can close under the pointer without a mouseleave;
    // the popup would then stay open. Any move away from it and the tab bar hides it.
    document.addEventListener("mousemove", ev => {
        if (!menu.classList.contains("open")) return;
        if (menu.contains(ev.target) || tabBar.contains(ev.target)) return;
        scheduleHide();
    });
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

const FILTER_INPUT_DEBOUNCE_MS = 150;

export function _uiSetupPane(id) {
    const header = document.querySelector(`#pane-${id} .pane-header`);
    if (header && !header.querySelector('.pane-close-btn')) {
        const close = document.createElement('button');
        close.type = 'button';
        close.className = 'pane-close-btn';
        close.textContent = '✕';
        close.title = 'Remove from this view (the source keeps recording)';
        close.setAttribute('aria-label', `Remove ${paneLabel(id)} from this view`);
        close.hidden = true; // shown by refreshPaneControls once the layout is known
        close.addEventListener('click', () => _removePane(id));
        header.append(close);
    }
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
        controls.querySelector('.search-prev').addEventListener('click', () => navigate(-1));
        controls.querySelector('.search-next').addEventListener('click', () => navigate(1));
        input.addEventListener('keydown', event => {
            if (event.isComposing) return;
            if (state.searches[id] && (event.key === 'Enter' || event.key === 'F3')) {
                event.preventDefault();
                navigate(event.shiftKey ? -1 : 1);
            } else if (event.key === 'Enter') {
                flushPendingApply();
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
        let applying = null;
        const sessionId = () => window.__embedLogSession?.id;
        let pendingSession;
        function flushPendingApply() {
            if (pendingApply !== null) {
                clearTimeout(pendingApply);
                pendingApply = null;
                if (input.isConnected && pendingSession === sessionId()) applying = apply();
            }
        }
        async function navigate(direction) {
            if (!input.isConnected) return;
            flushPendingApply();
            const search = state.searches[id];
            const value = input.value;
            const session = sessionId();
            if (search?.loading) await applying;
            if (input.isConnected && session === sessionId() && input.value === value && state.searches[id] === search) {
                navigatePaneSearch(id, direction);
            }
        }
        input.addEventListener('input', () => {
            clearTimeout(pendingApply);
            pendingSession = sessionId();
            pendingApply = setTimeout(flushPendingApply, FILTER_INPUT_DEBOUNCE_MS);
        });
        mode.addEventListener('click', () => {
            clearTimeout(pendingApply);
            pendingApply = null;
            const enabled = mode.getAttribute('aria-pressed') !== 'true';
            mode.setAttribute('aria-pressed', String(enabled));
            controls.hidden = !enabled;
            applying = apply();
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
        return { paneLeft, paneRight };
    }

    function eventX(ev) {
        if (ev.touches && ev.touches[0]) return ev.touches[0].clientX;
        if (ev.changedTouches && ev.changedTouches[0]) return ev.changedTouches[0].clientX;
        return ev.clientX;
    }

    function startDrag(splitter, ev) {
        const { paneLeft, paneRight } = findNeighborPanes(splitter);
        if (!paneLeft || !paneRight) return;

        ev.preventDefault();
        splitter.classList.add("dragging");
        document.body.style.cursor = "col-resize";

        const startX = eventX(ev);
        const startLeftW = paneLeft.getBoundingClientRect().width;
        // Only the two neighbours trade width. Sharing the whole tab would squeeze
        // a third pane to nothing.
        const totalW = startLeftW + paneRight.getBoundingClientRect().width;

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
