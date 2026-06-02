import {
    state, TABS, PANES, buildTimestampInfo, applyTimestampModeToLine,
    lineHasTimestampMode,
} from './state.js';
import { parseAnsi } from './ansi.js';
import { analyzeLinePlugins, getLinePluginTooltip, getConfiguredPanePlugins, getPanePluginSettings, setPanePluginSetting } from './pluginRuntime.js';

// ---------------------------------------------------------------------------
// Line rendering
// ---------------------------------------------------------------------------


// parseAnsi HTML-escapes < and >, so <wrn> becomes &lt;wrn&gt; in stored HTML.
// <inf> is intentionally excluded — it stays unstyled.
// Also matches bracket-style markers like [ERR], [WRN], [error], [warning].
const _LINE_TAG_RE = /(?:&lt;(wrn|warn|dbg|debug|err|error)&gt;|\[(err|wrn|dbg|warn|debug|error|warning|ERROR|WARNING|ERR|WRN|DBG|DEBUG)\])/;
function _lineTagClass(html) {
    const m = _LINE_TAG_RE.exec(html);
    if (!m) return "";
    const tag = (m[1] || m[2]).toLowerCase();
    switch (tag) {
        case "wrn":  case "warn":  return " line-wrn";
        case "dbg":  case "debug": return " line-dbg";
        case "err":  case "error": return " line-err";
        default: return "";
    }
}
function _escapeHtml(text) {
    return String(text)
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;")
        .replace(/"/g, "&quot;");
}

export function buildStoredLine(paneId, ts, rawText, isTx, meta = null) {
    const html = parseAnsi(rawText);
    const line = {
        paneId,
        ...buildTimestampInfo(ts, typeof meta === "object" && meta !== null
            ? meta
            : (Number.isFinite(meta) ? { numTs: meta } : {})),
        html,
        rawText,
        isTx,
        pluginData: null,
        pluginFilterText: "",
        pluginClassNames: [],
        pluginInlineText: "",
    };
    analyzeLinePlugins(paneId, line);
    return line;
}

export function buildLineHtml(line, showTs, filterRx) {
    const tsClass = "ts" + (showTs ? "" : " hidden");
    let content = line.pluginInlineText ? _escapeHtml(line.pluginInlineText) : line.html;
    if (filterRx) {
        content = content.replace(filterRx, m => `<mark class="hl">${m}</mark>`);
    }
    return `<span class="${tsClass}">${line.ts}</span>${content}`;
}

// Build the full className string for a log-line div, preserving selection state.
export function _lineClass(line, idx, paneId) {
    const pluginClasses = Array.isArray(line.pluginClassNames) && line.pluginClassNames.length
        ? " " + line.pluginClassNames.join(" ")
        : "";
    return "log-line"
        + (line.isTx ? " tx-line" : "")
        + _lineTagClass(line.html)
        + pluginClasses
        + (state.selected[paneId].has(idx) ? " selected" : "");
}

export function matchesFilter(line, rx) {
    if (!rx) return true;
    const rendered = line.pluginInlineText || line.html.replace(/<[^>]+>/g, "");
    const rawText = typeof line.rawText === "string" ? line.rawText : "";
    const plain = `${rendered} ${rawText} ${line.ts} ${line.pluginFilterText || ""}`;
    return rx.test(plain);
}
const _pluginTooltipEl = document.createElement("div");
_pluginTooltipEl.id = "plugin-tooltip";
document.body.appendChild(_pluginTooltipEl);

const _pluginInfoEl = document.createElement("div");
_pluginInfoEl.id = "pane-plugin-hover-card";
document.body.appendChild(_pluginInfoEl);

let _pluginInfoHideTimer = null;
let _pluginInfoPaneId = null;
let _pluginInfoAnchorEl = null;

function _hasAnySelection() {
    return PANES.some(id => state.selected[id]?.size > 0);
}

function _showPluginTooltip(lineDiv) {
    const text = lineDiv?.dataset?.pluginTooltip || "";
    if (!text) {
        _pluginTooltipEl.classList.remove("visible");
        return;
    }
    const rect = lineDiv.getBoundingClientRect();
    _pluginTooltipEl.textContent = text;
    _pluginTooltipEl.style.left = Math.max(4, rect.left) + "px";
    _pluginTooltipEl.style.bottom = (window.innerHeight - rect.top + 4) + "px";
    _pluginTooltipEl.classList.add("visible");
}

function _hidePluginTooltip() {
    _pluginTooltipEl.classList.remove("visible");
}

function _cancelPluginInfoHide() {
    if (_pluginInfoHideTimer !== null) {
        clearTimeout(_pluginInfoHideTimer);
        _pluginInfoHideTimer = null;
    }
}

function _positionPluginInfo(anchor) {
    const margin = 8;
    const gap = 8;
    const rect = anchor.getBoundingClientRect();
    _pluginInfoEl.style.visibility = "hidden";
    _pluginInfoEl.classList.add("visible");
    const width = _pluginInfoEl.offsetWidth;
    const height = _pluginInfoEl.offsetHeight;
    const maxLeft = Math.max(margin, window.innerWidth - width - margin);
    const left = Math.min(Math.max(margin, rect.right - width), maxLeft);
    const belowTop = rect.bottom + gap;
    const aboveTop = rect.top - gap - height;
    const maxTop = Math.max(margin, window.innerHeight - height - margin);
    const top = belowTop <= maxTop
        ? belowTop
        : Math.max(margin, aboveTop >= margin ? aboveTop : maxTop);
    _pluginInfoEl.style.left = `${left}px`;
    _pluginInfoEl.style.top = `${top}px`;
    _pluginInfoEl.style.visibility = "";
}

function _hidePluginInfo() {
    _cancelPluginInfoHide();
    _pluginInfoEl.classList.remove("visible");
    _pluginInfoPaneId = null;
    _pluginInfoAnchorEl = null;
}

function _schedulePluginInfoHide(delay = 140) {
    _cancelPluginInfoHide();
    _pluginInfoHideTimer = window.setTimeout(() => {
        _pluginInfoHideTimer = null;
        _hidePluginInfo();
    }, delay);
}

function _renderPluginInfo(paneId, anchor) {
    if (_hasAnySelection()) {
        _hidePluginInfo();
        return false;
    }
    const plugins = getPanePluginSettings(paneId);
    if (!plugins.length) {
        _hidePluginInfo();
        return false;
    }

    _pluginInfoEl.innerHTML = "";
    plugins.forEach(plugin => {
        const section = document.createElement("section");
        section.className = "pane-plugin-hover-section";

        const title = document.createElement("div");
        title.className = "pane-plugin-hover-title";
        title.textContent = plugin.displayName;
        section.appendChild(title);

        plugin.settings.forEach(setting => {
            if (setting.type !== "boolean") return;

            const row = document.createElement("label");
            row.className = "pane-plugin-hover-toggle";

            const input = document.createElement("input");
            input.type = "checkbox";
            input.checked = setting.value === true;
            input.addEventListener("change", ev => {
                ev.stopPropagation();
                if (!setPanePluginSetting(paneId, plugin.name, setting.key, input.checked)) return;
                reanalyzePanePlugins(paneId);
                window.__embedLogSchedulePersist?.();
                _renderPluginInfo(paneId, anchor);
            });
            row.appendChild(input);

            const textWrap = document.createElement("span");
            textWrap.className = "pane-plugin-hover-copy";

            const label = document.createElement("span");
            label.className = "pane-plugin-hover-label";
            label.textContent = setting.label;
            textWrap.appendChild(label);

            const desc = document.createElement("div");
            desc.className = "pane-plugin-hover-desc";
            desc.textContent = `${input.checked ? "Enabled" : "Disabled"} — ${setting.description || setting.label}`;
            textWrap.appendChild(desc);

            row.appendChild(textWrap);
            section.appendChild(row);
        });

        _pluginInfoEl.appendChild(section);
    });

    _pluginInfoPaneId = paneId;
    _pluginInfoAnchorEl = anchor;
    _positionPluginInfo(anchor);
    return true;
}

function _showPluginInfo(paneId, anchor) {
    _cancelPluginInfoHide();
    _renderPluginInfo(paneId, anchor);
}

_pluginInfoEl.addEventListener("mouseenter", _cancelPluginInfoHide);
_pluginInfoEl.addEventListener("mouseleave", () => _schedulePluginInfoHide());

window.addEventListener("resize", () => {
    if (_pluginInfoEl.classList.contains("visible") && _pluginInfoAnchorEl) {
        _positionPluginInfo(_pluginInfoAnchorEl);
    }
});

export function hidePluginOverlays() {
    _hidePluginTooltip();
    _hidePluginInfo();
}
window.__embedLogHidePluginOverlays = hidePluginOverlays;
export function applyLineDom(div, line, paneId, idx, filterRx) {
    div.className = _lineClass(line, idx, paneId);
    const tooltip = getLinePluginTooltip(line);
    if (tooltip) {
        div.dataset.pluginTooltip = tooltip;
    } else {
        delete div.dataset.pluginTooltip;
    }
    if (!matchesFilter(line, filterRx)) {
        div.style.display = "none";
        div.innerHTML = "";
        return;
    }
    div.style.display = "";
    div.innerHTML = buildLineHtml(line, state.showTs, filterRx);
}

export function appendLine(paneId, ts, rawText, isTx, meta = null) {
    appendLineBatch([{ paneId, ts, rawText, isTx, meta }]);
}

export function appendLineBatch(entries) {
    const fragments = new Map();
    const touched = new Set();

    entries.forEach(({ paneId, ts, rawText, isTx, meta = null }) => {
        if (!state.rawLines[paneId]) return;

        const line = buildStoredLine(paneId, ts, rawText, isTx, meta);
        state.rawLines[paneId].push(line);

        const logEl = document.getElementById("log-" + paneId);
        if (!logEl) return;

        const idx = state.rawLines[paneId].length - 1;
        const div = document.createElement("div");
        div.dataset.ts = line.ts;
        div.dataset.idx = idx;

        const rx = state.filters[paneId];
        applyLineDom(div, line, paneId, idx, rx);

        if (!fragments.has(paneId)) fragments.set(paneId, document.createDocumentFragment());
        fragments.get(paneId).appendChild(div);
        touched.add(paneId);
    });

    touched.forEach(paneId => {
        const logEl = document.getElementById("log-" + paneId);
        const fragment = fragments.get(paneId);
        if (!logEl || !fragment) return;
        logEl.appendChild(fragment);
        if (state.atBottom[paneId]) logEl.scrollTop = logEl.scrollHeight;
        updateJumpBtn(paneId);
    });

    if (touched.size > 0) {
        window.__embedLogSchedulePersist?.();
        window.__embedLogUpdateTimestampModeUi?.();
        window.applyMarkers?.();
    }
}

export function rerenderPane(paneId) {
    const logEl = document.getElementById("log-" + paneId);
    const lines = state.rawLines[paneId];
    const divs  = logEl.children;
    const rx    = state.filters[paneId];

    for (let i = 0; i < lines.length; i++) {
        const line = lines[i];
        const div  = divs[i];
        if (!div) continue;
        applyLineDom(div, line, paneId, i, rx);
    }
    if (state.atBottom[paneId]) logEl.scrollTop = logEl.scrollHeight;
}
export function reanalyzePanePlugins(paneId) {
    const lines = state.rawLines[paneId] || [];
    lines.forEach(line => analyzeLinePlugins(paneId, line));
    rerenderPane(paneId);
}
export function setTimestampMode(mode) {
    const nextMode = mode === "relative" ? "relative" : "absolute";
    if (state.timestampMode === nextMode) return;

    state.timestampMode = nextMode;
    state.syncTs = null;
    state.syncTabSwitch = false;

    PANES.forEach(paneId => {
        const lines = state.rawLines[paneId] || [];
        lines.forEach(applyTimestampModeToLine);
        rerenderPane(paneId);

        highlightLine(paneId, null);
    });
    window.__embedLogSchedulePersist?.();
    window.applyMarkers?.();

    window.__embedLogUpdateTimestampModeUi?.();
}

export function canDisplayTimestampMode(mode) {
    for (const paneId of PANES) {
        const lines = state.rawLines[paneId] || [];
        for (const line of lines) {
            if (lineHasTimestampMode(line, mode)) return true;
        }
    }
    return false;
}

// ---------------------------------------------------------------------------
// Jump-to-bottom
// ---------------------------------------------------------------------------

export function updateJumpBtn(paneId) {
    document.getElementById("jump-" + paneId)
        .classList.toggle("visible", !state.atBottom[paneId]);
}

export function scrollPaneToBottom(paneId) {
    const logEl = document.getElementById("log-" + paneId);
    if (!logEl) return;
    logEl.scrollTop = logEl.scrollHeight;
    state.atBottom[paneId] = true;
    updateJumpBtn(paneId);
}

export function _linesSetupPane(id) {
    const logEl = document.getElementById("log-" + id);
    logEl.addEventListener("scroll", () => {
        state.atBottom[id] = logEl.scrollHeight - logEl.scrollTop - logEl.clientHeight < 40;
        updateJumpBtn(id);
    });
    // Event delegation — replaces per-line listeners for better performance
    logEl.addEventListener("click", e => {
        const lineDiv = e.target.closest(".log-line");
        if (!lineDiv) return;
        const idx = parseInt(lineDiv.dataset.idx, 10);
        const line = state.rawLines[id] ? state.rawLines[id][idx] : null;
        if (!line) return;
        hidePluginOverlays();
        onLineClick(id, line.numTs, lineDiv);
    });
    logEl.addEventListener("mousedown", e => { if (e.button === 1) e.preventDefault(); });
    logEl.addEventListener("auxclick", e => {
        if (e.button !== 1) return;
        const lineDiv = e.target.closest(".log-line");
        if (!lineDiv) return;
        const idx = parseInt(lineDiv.dataset.idx, 10);
        const line = state.rawLines[id] ? state.rawLines[id][idx] : null;
        if (!line) return;
        onMiddleClick(id, line.numTs, lineDiv);
    });
    logEl.addEventListener("mousemove", e => {
        const lineDiv = e.target.closest(".log-line");
        if (!lineDiv || !logEl.contains(lineDiv)) {
            _hidePluginTooltip();
            return;
        }
        _showPluginTooltip(lineDiv);
    });
    logEl.addEventListener("mouseleave", _hidePluginTooltip);
    document.getElementById("jump-" + id).addEventListener("click", () => {
        state.syncTabSwitch = false;
        scrollPaneToBottom(id);
    });

    // Per-pane wrap toggle
    const wrapBtn = document.querySelector(`#pane-${id} .pane-wrap-btn`);
    if (wrapBtn) {
        wrapBtn.addEventListener("click", () => {
            state.wrap[id] = !state.wrap[id];
            wrapBtn.classList.toggle("active", state.wrap[id]);
            document.getElementById("log-" + id)?.classList.toggle("wrap", state.wrap[id]);
        });
    }

    // Per-pane raw download
    const dlBtn = document.querySelector(`#pane-${id} .pane-download-btn`);
    if (dlBtn) {
        dlBtn.addEventListener("click", () => {
            const lines = state.rawLines[id] || [];
            if (!lines.length) return;
            const text = lines.map(line => {
                const clean = (line.rawText ?? "").replace(/\x1b(?:\[[0-9;]*[A-Za-z]|\][^\x07]*\x07|[^[\]])/g, "").trim();
                return `[${line.ts}] ${clean}`;
            }).join("\n");
            const blob = new Blob([text + "\n"], { type: "text/plain" });
            const url = URL.createObjectURL(blob);
            const a = document.createElement("a");
            a.href = url;
            a.download = `${id}.log`;
            a.click();
            URL.revokeObjectURL(url);
        });
    }

    // Plugin indicator — shown when this pane has plugins enabled
    const header = document.querySelector(`#pane-${id} .pane-header`);
    if (header) {
        const indicator = document.createElement("button");
        indicator.type = "button";
        indicator.className = "pane-plugin-indicator";
        indicator.id = "plugin-indicator-" + id;
        header.appendChild(indicator);
        _refreshPluginIndicator(id);
        if (_pluginInfoPaneId === id && _hasAnySelection()) _hidePluginInfo();
    }
}
PANES.forEach(_linesSetupPane);

// ── Plugin indicator helpers ───────────────────────────────────────

function _refreshPluginIndicator(paneId) {
    const el = document.getElementById("plugin-indicator-" + paneId);
    if (!el) return;
    const panePlugins = getConfiguredPanePlugins();
    const refs = panePlugins[paneId];
    if (!refs || !refs.length) {
        el.style.display = "none";
        if (_pluginInfoPaneId === paneId) _hidePluginInfo();
        return;
    }
    const configurable = getPanePluginSettings(paneId).length > 0;
    el.style.display = "";
    el.textContent = "\u26A1";  // ⚡
    el.title = refs.map(r => r.name).join("\n") + (configurable ? "\n\nClick to configure" : "");
    el.classList.toggle("configurable", configurable);
    el.tabIndex = configurable ? 0 : -1;
    el.setAttribute("aria-label", configurable ? "Configure pane plugins" : "Active pane plugins");
    if (el.dataset.hoverBound === "1") return;
    el.dataset.hoverBound = "1";
    el.addEventListener("mouseenter", () => _showPluginInfo(paneId, el));
    el.addEventListener("mouseleave", () => _schedulePluginInfoHide());
    el.addEventListener("focus", () => _showPluginInfo(paneId, el));
    el.addEventListener("blur", () => _schedulePluginInfoHide(0));
}
export function refreshPluginIndicators() {
    PANES.forEach(_refreshPluginIndicator);
}
window.__embedLogRefreshPluginIndicators = refreshPluginIndicators;

// Clear
// ---------------------------------------------------------------------------

export function clearPane(paneId) {
    state.rawLines[paneId] = [];
    state.selected[paneId] = new Set();
    document.getElementById("log-" + paneId).innerHTML = "";
    highlightLine(paneId, null);
    hidePluginOverlays();
    state.atBottom[paneId] = true;
    updateJumpBtn(paneId);
    // Hide copy-selection actions if selection.js has added them
    document.getElementById("copy-actions-" + paneId)?.classList.remove("visible");
    // Close any open More dropdown for this pane
    document.getElementById("more-dropdown-" + paneId)?.classList.remove("open");
    window.__embedLogSchedulePersist?.();
    window.__embedLogUpdateTimestampModeUi?.();
}

document.getElementById("btn-clear")?.addEventListener("click", () => {
    window.wsSend?.({ cmd: "clear_logs", scope: "all" });
    PANES.forEach(clearPane);
});


// Rebuild DOM for a pane from stored state — used after layout rebuild (UNWRAP toggle)
export function repopulatePaneLogs(paneId) {
    const logEl = document.getElementById("log-" + paneId);
    if (!logEl) return;
    logEl.innerHTML = "";
    const lines = state.rawLines[paneId] || [];
    const rx = state.filters[paneId];
    lines.forEach((line, idx) => {
        const div = document.createElement("div");
        div.dataset.ts  = line.ts;
        div.dataset.idx = idx;
        applyLineDom(div, line, paneId, idx, rx);

        logEl.appendChild(div);
    });
    if (state.atBottom[paneId]) logEl.scrollTop = logEl.scrollHeight;
    updateJumpBtn(paneId);
}
// ---------------------------------------------------------------------------
// Sync
// ---------------------------------------------------------------------------

export function highlightLine(paneId, div) {
    const prev = state.highlighted[paneId];
    if (prev) prev.classList.remove("sync-highlight");
    state.highlighted[paneId] = div;
    if (div) div.classList.add("sync-highlight");
}

// Scroll a pane to the line closest to numTs — used when switching tabs.
// Centers the matched line at ~1/3 from the top.
export function scrollPaneToTs(paneId, numTs) {
    if (numTs === null) return;
    const lines = state.rawLines[paneId];
    if (!lines.length) return;

    let lo = 0, hi = lines.length - 1;
    while (lo < hi) {
        const mid = (lo + hi) >> 1;
        if (lines[mid].numTs < numTs) lo = mid + 1;
        else hi = mid;
    }
    if (lo > 0 && Math.abs(lines[lo - 1].numTs - numTs) < Math.abs(lines[lo].numTs - numTs)) lo--;

    const logEl = document.getElementById("log-" + paneId);
    const div   = logEl.children[lo];
    if (!div) return;

    logEl.scrollTop = Math.max(0, div.offsetTop - Math.floor(logEl.clientHeight / 3));
    state.atBottom[paneId] = false;
    updateJumpBtn(paneId);
    highlightLine(paneId, div);
}

// Middle-click: always clear the filter for this pane, scroll to the line
// in full context, and sync — the deliberate "zoom out to this moment" gesture.
export function onMiddleClick(paneId, numTs, div) {
    const logEl = document.getElementById("log-" + paneId);

    if (state.filters[paneId]) {
        const input = document.querySelector(`.filter-input[data-pane="${paneId}"]`);
        input.value = "";
        state.filters[paneId] = null;
        input.classList.remove("invalid");
        rerenderPane(paneId);
    }

    logEl.scrollTop = div.offsetTop - Math.floor(logEl.clientHeight / 3);
    state.atBottom[paneId] = false;
    updateJumpBtn(paneId);

    state.syncTs = numTs;
    state.syncTabSwitch = true;
    highlightLine(paneId, div);
    syncPanes(paneId, numTs, div);
}

// Click handler:
//   • filter active  → clear filter, re-render, scroll source to line in context
//   • no filter      → source pane stays exactly where user was (no scroll)
//   • always         → store syncTs, highlight clicked line, sync other panes in active tab
export function onLineClick(paneId, numTs, div) {
    const logEl = document.getElementById("log-" + paneId);

    if (state.filters[paneId]) {
        const filterInput = document.querySelector(`.filter-input[data-pane="${paneId}"]`);
        filterInput.value = "";
        state.filters[paneId] = null;
        filterInput.classList.remove("invalid");
        rerenderPane(paneId);
        logEl.scrollTop = div.offsetTop - Math.floor(logEl.clientHeight / 3);
        state.atBottom[paneId] = false;
        updateJumpBtn(paneId);
    }

    state.syncTs = numTs;
    state.syncTabSwitch = true;
    highlightLine(paneId, div);
    syncPanes(paneId, numTs, div);
}

// Sync all OTHER panes in the active tab to numTs, mirroring the clicked
// line's Y position within the viewport.
export function syncPanes(fromId, numTs, clickedDiv) {
    if (state.unwrap) return;

    const activePanes = TABS[state.activeTab]?.panes || [];
    if (activePanes.length < 2) return;

    const fromLogEl     = document.getElementById("log-" + fromId);
    const clickedRelTop = clickedDiv.offsetTop - fromLogEl.scrollTop;

    activePanes.forEach(toId => {
        if (toId === fromId) return;
        const lines = state.rawLines[toId];
        if (!lines.length) return;

        // Binary search for closest timestamp
        let lo = 0, hi = lines.length - 1;
        while (lo < hi) {
            const mid = (lo + hi) >> 1;
            if (lines[mid].numTs < numTs) lo = mid + 1;
            else hi = mid;
        }
        if (lo > 0 && Math.abs(lines[lo - 1].numTs - numTs) < Math.abs(lines[lo].numTs - numTs)) {
            lo--;
        }

        const logEl     = document.getElementById("log-" + toId);
        const targetDiv = logEl.children[lo];
        if (!targetDiv) return;

        logEl.scrollTop = targetDiv.offsetTop - clickedRelTop;
        state.atBottom[toId] = false;
        updateJumpBtn(toId);
        highlightLine(toId, targetDiv);
    });
}
