import {
    state, TABS, PANES, buildTimestampInfo, applyTimestampModeToLine,
    lineHasTimestampMode, resetRelativeTimestampBase, noteRelativeTimestampCandidate,
} from './state.js';
import { parseAnsi } from './ansi.js';
import { analyzeLinePlugins, getLinePluginTooltip, getConfiguredPanePlugins, getPanePluginSettings, setPanePluginSetting } from './pluginRuntime.js';

// ---------------------------------------------------------------------------
// Line rendering
// ---------------------------------------------------------------------------
// Windowed rendering state
// ---------------------------------------------------------------------------

const OVERSCAN = 60;

// paneId → virtual render controller for spacer/window geometry and raw-index DOM cache
const _virtualPanes = new Map();
// paneId → boolean — rAF debounce guard for scroll-triggered window shifts
const _pendingRaf = new Map();
// paneId → running UTF-8 byte total for stats display
const _paneBytes = new Map();
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
    const lineMeta = typeof meta === "object" && meta !== null
        ? meta
        : (Number.isFinite(meta) ? { numTs: meta } : {});
    const line = {
        paneId,
        ...buildTimestampInfo(ts, lineMeta),
        serverLineIdx: Number.isFinite(lineMeta.lineIdx) ? lineMeta.lineIdx : null,
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
let _pluginInfoPinned = false;
let _pluginInfoClickInside = false;
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
    _pluginInfoPinned = false;
}

function _schedulePluginInfoHide(delay = 400) {
    if (_pluginInfoPinned) return;
    _cancelPluginInfoHide();
    _pluginInfoHideTimer = window.setTimeout(() => {
        _pluginInfoHideTimer = null;
        _hidePluginInfo();
    }, delay);
}

function _renderPluginInfo(paneId, anchor) {
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
_pluginInfoEl.addEventListener("mousedown", () => { _pluginInfoClickInside = true; });

document.addEventListener("click", ev => {
    if (_pluginInfoClickInside) {
        _pluginInfoClickInside = false;
        return;
    }
    if (!_pluginInfoPinned) return;
    if (_pluginInfoAnchorEl?.contains(ev.target)) return;
    _hidePluginInfo();
});

let _resizeRefreshRaf = null;
function _scheduleVirtualResizeRefresh() {
    if (_resizeRefreshRaf !== null) return;
    _resizeRefreshRaf = requestAnimationFrame(() => {
        _resizeRefreshRaf = null;
        PANES.forEach(paneId => {
            const vp = _virtualPanes.get(paneId);
            if (vp) vp.rowHeight = 0;
            rerenderPane(paneId);
        });
    });
}

window.addEventListener("resize", () => {
    if (_pluginInfoEl.classList.contains("visible") && _pluginInfoAnchorEl) {
        _positionPluginInfo(_pluginInfoAnchorEl);
    }
    _scheduleVirtualResizeRefresh();
});
document.addEventListener("keydown", ev => {
    if (ev.key === "Escape") _hidePluginInfo();
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

    const highlightedIdx = state.highlightedIdx[paneId];
    div.classList.toggle("sync-highlight", highlightedIdx === idx);
    if (highlightedIdx === idx) {
        state.highlighted[paneId] = div;
    }

    const marker = _markerAt(paneId, idx, line);
    div.classList.toggle("has-marker", marker !== null);
    if (marker !== null) {
        div.dataset.markerTooltip = marker.description || "";
        div.dataset.kind = marker.kind || "user";
        div.dataset.severity = marker.severity || "";
    } else {
        delete div.dataset.markerTooltip;
        delete div.dataset.kind;
        delete div.dataset.severity;
    }
    if (!matchesFilter(line, filterRx)) {
        div.style.display = "none";
        div.innerHTML = "";
        return;
    }
    div.style.display = "";
    div.innerHTML = buildLineHtml(line, state.showTs, filterRx);
}


// ── Virtual pane helpers ───────────────────────────────────────────

function _getVirtual(paneId) {
    let vp = _virtualPanes.get(paneId);
    if (vp) return vp;
    const logEl = document.getElementById("log-" + paneId);
    if (!logEl) return null;

    let spacerEl = logEl.querySelector(".log-spacer");
    let windowEl = logEl.querySelector(".log-window");

    if (!spacerEl) {
        spacerEl = document.createElement("div");
        spacerEl.className = "log-spacer";
        if (windowEl) {
            logEl.insertBefore(spacerEl, windowEl);
            spacerEl.appendChild(windowEl);
        } else {
            logEl.appendChild(spacerEl);
        }
    }

    if (!windowEl) {
        windowEl = document.createElement("div");
        windowEl.className = "log-window";
        spacerEl.appendChild(windowEl);
    } else if (windowEl.parentElement !== spacerEl) {
        spacerEl.appendChild(windowEl);
    }

    vp = {
        spacerEl,
        windowEl,
        rendered: new Map(),
        firstRendered: -1,
        lastRendered: -1,
        rowHeight: 0,
        visibleIndices: null,
        projectionFilter: undefined,
        projectionSourceLen: -1,
    };
    _virtualPanes.set(paneId, vp);
    return vp;
}

function _measureRowHeight(paneId) {
    const vp = _getVirtual(paneId);
    if (!vp) return 20;
    if (vp.rowHeight > 0) return vp.rowHeight;
    for (const el of vp.rendered.values()) {
        if (el.offsetHeight > 0) {
            vp.rowHeight = el.offsetHeight;
            return vp.rowHeight;
        }
    }
    const lines = state.rawLines[paneId];
    if (lines && lines.length > 0) {
        const entry = lines[0];
        const line = _isRawTuple(entry) ? getLine(paneId, 0) : entry;
        if (line) {
            const div = document.createElement("div");
            div.className = "log-line";
            div.style.position = "absolute";
            div.style.visibility = "hidden";
            div.innerHTML = buildLineHtml(line, state.showTs, null);
            vp.windowEl.appendChild(div);
            vp.rowHeight = div.offsetHeight || 20;
            vp.windowEl.removeChild(div);
            return vp.rowHeight;
        }
    }
    vp.rowHeight = 20;
    return 20;
}

function _isRawTuple(entry) {
    return Array.isArray(entry);
}

export function getLine(paneId, idx) {
    const lines = state.rawLines[paneId];
    if (!lines || idx < 0 || idx >= lines.length) return null;
    const entry = lines[idx];
    if (!_isRawTuple(entry)) return entry;
    const [ts, rawText, isTx, meta] = entry;
    const line = buildStoredLine(paneId, ts, rawText, isTx, meta);
    lines[idx] = line;
    return line;
}

function _getNumTs(entry, paneId = null, idx = -1) {
    if (_isRawTuple(entry)) {
        const meta = entry[3];
        if (meta && typeof meta === "object") {
            if (Number.isFinite(meta.numTs)) return meta.numTs;
            if (state.timestampMode === "relative" && Number.isFinite(meta.relNum)) return meta.relNum;
            if (state.timestampMode === "absolute" && Number.isFinite(meta.absNum)) return meta.absNum;
            if (Number.isFinite(meta.relNum)) return meta.relNum;
            if (Number.isFinite(meta.absNum)) return meta.absNum;
        }
        if (Number.isFinite(meta)) return meta;
        if (paneId !== null) {
            const line = getLine(paneId, idx);
            return line ? line.numTs : NaN;
        }
        return NaN;
    }
    return entry.numTs;
}

function _getVisibleIndices(paneId, vp) {
    const lines = state.rawLines[paneId] || [];
    const rx = state.filters[paneId] || null;
    if (!rx) {
        vp.visibleIndices = null;
        vp.projectionFilter = null;
        vp.projectionSourceLen = lines.length;
        return null;
    }
    if (vp.projectionFilter === rx && vp.projectionSourceLen === lines.length && Array.isArray(vp.visibleIndices)) {
        return vp.visibleIndices;
    }
    const indices = [];
    for (let rawIndex = 0; rawIndex < lines.length; rawIndex++) {
        const line = getLine(paneId, rawIndex);
        if (line && matchesFilter(line, rx)) indices.push(rawIndex);
    }
    vp.visibleIndices = indices;
    vp.projectionFilter = rx;
    vp.projectionSourceLen = lines.length;
    return indices;
}

function _visibleCount(paneId, vp) {
    const lines = state.rawLines[paneId] || [];
    const visible = _getVisibleIndices(paneId, vp);
    return visible ? visible.length : lines.length;
}

function _rawIndexAt(paneId, vp, ordinal) {
    const visible = _getVisibleIndices(paneId, vp);
    return visible ? visible[ordinal] : ordinal;
}

function _ordinalForRawIndex(paneId, vp, rawIndex) {
    const visible = _getVisibleIndices(paneId, vp);
    if (!visible) return rawIndex;
    let lo = 0, hi = visible.length;
    while (lo < hi) {
        const mid = (lo + hi) >> 1;
        if (visible[mid] < rawIndex) lo = mid + 1;
        else hi = mid;
    }
    return Math.max(0, Math.min(Math.max(0, visible.length - 1), lo));
}

function _markerAt(paneId, idx, line = null) {
    const markers = state.markers[paneId];
    if (!markers) return null;
    for (const m of markers) {
        const isEvent = (m.kind || "user") === "event";
        const lineKey = isEvent && Number.isFinite(line?.serverLineIdx) ? line.serverLineIdx : idx;
        const end = m.endIdx ?? m.lineIdx;
        if (lineKey >= m.lineIdx && lineKey <= end) return m;
    }
    return null;
}

function _ensureLine(paneId, rawIndex, ordinal, filterRx, rowH) {
    const vp = _getVirtual(paneId);
    if (!vp) return null;
    const lines = state.rawLines[paneId];
    if (!lines || rawIndex < 0 || rawIndex >= lines.length) return null;
    const line = _isRawTuple(lines[rawIndex]) ? getLine(paneId, rawIndex) : lines[rawIndex];
    if (!line) return null;

    const existing = vp.rendered.get(rawIndex);
    if (existing) {
        applyLineDom(existing, line, paneId, rawIndex, filterRx);
        existing.dataset.ts = line.ts;
        existing.style.position = "absolute";
        existing.style.top = (ordinal * rowH) + "px";
        existing.style.left = "0";
        existing.style.right = "0";
        return existing;
    }

    const div = document.createElement("div");
    div.className = "log-line";
    div.dataset.idx = rawIndex;
    div.dataset.ts = line.ts;
    applyLineDom(div, line, paneId, rawIndex, filterRx);
    div.style.position = "absolute";
    div.style.top = (ordinal * rowH) + "px";
    div.style.left = "0";
    div.style.right = "0";
    vp.windowEl.appendChild(div);
    vp.rendered.set(rawIndex, div);
    if (vp.firstRendered < 0 || rawIndex < vp.firstRendered) vp.firstRendered = rawIndex;
    if (vp.lastRendered < 0 || rawIndex > vp.lastRendered) vp.lastRendered = rawIndex;
    return div;
}

function _ensureRange(paneId, startOrdinal, endOrdinal, filterRx, rowH) {
    const vp = _getVirtual(paneId);
    if (!vp) return;
    for (let ordinal = startOrdinal; ordinal <= endOrdinal; ordinal++) {
        const rawIndex = _rawIndexAt(paneId, vp, ordinal);
        if (rawIndex === undefined) continue;
        _ensureLine(paneId, rawIndex, ordinal, filterRx, rowH);
    }
}

function _pruneToRawSet(paneId, keepRaw) {
    const vp = _getVirtual(paneId);
    if (!vp) return;
    const toRemove = [];
    for (const [idx, el] of vp.rendered) {
        if (!keepRaw.has(idx)) {
            toRemove.push(idx);
            if (el.parentNode) el.parentNode.removeChild(el);
        }
    }
    for (const idx of toRemove) {
        vp.rendered.delete(idx);
    }
    vp.firstRendered = -1;
    vp.lastRendered = -1;
    for (const idx of vp.rendered.keys()) {
        if (vp.firstRendered < 0 || idx < vp.firstRendered) vp.firstRendered = idx;
        if (vp.lastRendered < 0 || idx > vp.lastRendered) vp.lastRendered = idx;
    }
}

export function getRenderedLineElement(paneId, rawIndex) {
    const vp = _virtualPanes.get(paneId);
    return vp ? vp.rendered.get(rawIndex) || null : null;
}

export function ensureLineVisible(paneId, rawIndex, { align = "center" } = {}) {
    const logEl = document.getElementById("log-" + paneId);
    if (!logEl) return;
    _renderVirtualWindow(paneId, { targetIdx: rawIndex });
    const div = getRenderedLineElement(paneId, rawIndex);
    if (!div) return;
    const top = div.offsetTop;
    if (align === "center") {
        logEl.scrollTop = Math.max(0, top - Math.floor(logEl.clientHeight / 2));
    } else if (align === "top") {
        logEl.scrollTop = top;
    } else if (align === "bottom") {
        logEl.scrollTop = Math.max(0, top - logEl.clientHeight + div.offsetHeight);
    }
}

window.__embedLogEnsureLineVisible = function (paneId, rawIndex, options) {
    ensureLineVisible(paneId, rawIndex, options || {});
};

window.__embedLogFindRawIndexContaining = function (paneId, text) {
    const needle = String(text ?? "");
    const lines = state.rawLines[paneId] || [];
    for (let i = 0; i < lines.length; i++) {
        const line = getLine(paneId, i);
        if (!line) continue;
        const haystack = `${line.rawText || ""} ${line.html || ""} ${line.ts || ""} ${line.pluginFilterText || ""}`;
        if (haystack.includes(needle)) return i;
    }
    return -1;
};

export function rerenderRenderedLines(paneId) {
    const vp = _virtualPanes.get(paneId);
    if (!vp) return;
    const rx = state.filters[paneId];
    vp.rendered.forEach((div, idx) => {
        const line = getLine(paneId, idx);
        if (line) applyLineDom(div, line, paneId, idx, rx);
    });
}

function _renderVirtualWindow(paneId, { targetIdx, forceAtBottom } = {}) {
    const vp = _getVirtual(paneId);
    if (!vp) return;
    const lines = state.rawLines[paneId];
    const logEl = document.getElementById("log-" + paneId);
    if (!logEl) return;

    if (!lines || !lines.length) {
        vp.windowEl.innerHTML = "";
        vp.rendered.clear();
        vp.spacerEl.style.height = "0";
        vp.firstRendered = -1;
        vp.lastRendered = -1;
        vp.firstOrdinal = -1;
        vp.lastOrdinal = -1;
        state.atBottom[paneId] = true;
        updateJumpBtn(paneId);
        return;
    }

    const totalCount = _visibleCount(paneId, vp);
    if (totalCount === 0) {
        vp.windowEl.innerHTML = "";
        vp.rendered.clear();
        vp.spacerEl.style.height = "0";
        vp.firstRendered = -1;
        vp.lastRendered = -1;
        vp.firstOrdinal = -1;
        vp.lastOrdinal = -1;
        state.atBottom[paneId] = true;
        updateJumpBtn(paneId);
        return;
    }

    if (targetIdx === undefined) targetIdx = forceAtBottom ? lines.length - 1 : 0;
    targetIdx = Math.max(0, Math.min(lines.length - 1, targetIdx));

    const rowH = _measureRowHeight(paneId);
    const totalH = totalCount * rowH;
    const viewH = logEl.clientHeight || rowH;
    const viewportCount = Math.max(1, Math.ceil(viewH / rowH));
    const targetOrdinal = forceAtBottom
        ? totalCount - 1
        : _ordinalForRawIndex(paneId, vp, targetIdx);

    // Clear any orphan non-managed children (transition from legacy/non-virtual DOM).
    if (vp.windowEl.children.length !== vp.rendered.size) {
        vp.windowEl.innerHTML = "";
        vp.rendered.clear();
        vp.firstRendered = -1;
        vp.lastRendered = -1;
    }

    let firstOrdinal, lastOrdinal;
    if (forceAtBottom) {
        lastOrdinal = totalCount - 1;
        firstOrdinal = Math.max(0, lastOrdinal - viewportCount - OVERSCAN);
    } else {
        firstOrdinal = Math.max(0, targetOrdinal - Math.floor(viewportCount / 2) - OVERSCAN);
        lastOrdinal = Math.min(totalCount - 1, targetOrdinal + Math.ceil(viewportCount / 2) + OVERSCAN);
    }
    vp.firstOrdinal = firstOrdinal;
    vp.lastOrdinal = lastOrdinal;

    vp.spacerEl.style.height = totalH + "px";

    const rx = state.filters[paneId];
    const keepRaw = new Set();
    for (let ordinal = firstOrdinal; ordinal <= lastOrdinal; ordinal++) {
        const rawIndex = _rawIndexAt(paneId, vp, ordinal);
        if (rawIndex !== undefined) keepRaw.add(rawIndex);
    }

    const wrapped = !!state.wrap[paneId];
    if (!wrapped) {
        for (const [rawIndex, el] of vp.rendered) {
            if (!keepRaw.has(rawIndex)) continue;
            const ordinal = _ordinalForRawIndex(paneId, vp, rawIndex);
            el.style.position = "absolute";
            el.style.top = (ordinal * rowH) + "px";
            el.style.left = "0";
            el.style.right = "0";
        }
    }

    _ensureRange(paneId, firstOrdinal, lastOrdinal, rx, rowH);
    _pruneToRawSet(paneId, keepRaw);

    // Wrapped rows can span multiple visual lines, so the fixed `ordinal * rowH`
    // layout above would overlap the next row. Re-stack the rendered window
    // using each row's actual measured height instead. Rows outside this
    // window still assume rowH for the spacer/scroll math (an approximation,
    // same tradeoff virtualized lists with variable row height typically make).
    if (wrapped) {
        let top = firstOrdinal * rowH;
        for (let ordinal = firstOrdinal; ordinal <= lastOrdinal; ordinal++) {
            const rawIndex = _rawIndexAt(paneId, vp, ordinal);
            const el = rawIndex !== undefined ? vp.rendered.get(rawIndex) : null;
            if (!el) continue;
            el.style.position = "absolute";
            el.style.top = top + "px";
            el.style.left = "0";
            el.style.right = "0";
            top += el.offsetHeight || rowH;
        }
    }

    if (forceAtBottom) {
        logEl.scrollTop = logEl.scrollHeight;
    }

    state.atBottom[paneId] = forceAtBottom || (logEl.scrollHeight - logEl.scrollTop - logEl.clientHeight < 40);
    updateJumpBtn(paneId);
}

// ---------------------------------------------------------------------------
// Stats (line count + UTF-8 byte count per pane; toolbar total)
// ---------------------------------------------------------------------------

const _textEncoder = new TextEncoder();

function _byteSize(text) {
    if (!text) return 0;
    return _textEncoder.encode(text).length;
}

function _formatInt(n) {
    return n.toLocaleString("en-US");
}

function _formatBytes(bytes) {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(bytes < 10 * 1024 ? 1 : 0)} kB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function _formatStatsText(lines, bytes) {
    return `${_formatInt(lines)} lines · ${_formatBytes(bytes)}`;
}

function _updatePaneStats(paneId) {
    const el = document.querySelector(`.pane-stats[data-pane-stats="${paneId}"]`);
    if (!el) return;
    const lines = state.rawLines[paneId] || [];
    el.textContent = lines.length
        ? _formatStatsText(lines.length, _paneBytes.get(paneId) || 0)
        : "";
}

function _updateToolbarStats() {
    const el = document.getElementById("toolbar-stats");
    if (!el) return;
    let totalLines = 0;
    let totalBytes = 0;
    for (const paneId of PANES) {
        const lines = state.rawLines[paneId];
        if (!lines) continue;
        totalLines += lines.length;
        totalBytes += _paneBytes.get(paneId) || 0;
    }
    el.textContent = totalLines
        ? `· ${_formatStatsText(totalLines, totalBytes)}`
        : "";
}

function _recordPaneBytes(paneId, newLines) {
    if (!newLines || newLines.length === 0) return;
    let delta = 0;
    for (const line of newLines) {
        // state.rawLines entries are [ts, rawText, isTx, meta]
        delta += _byteSize(line && line[1]);
    }
    _paneBytes.set(paneId, (_paneBytes.get(paneId) || 0) + delta);
}

function _resetPaneStats(paneId) {
    _paneBytes.set(paneId, 0);
    _updatePaneStats(paneId);
}

export function refreshStatsUi() {
    // Public hook so callers outside this module can re-render (e.g. after
    // importing lines from a file or after pane layout rebuilds).
    for (const paneId of PANES) _updatePaneStats(paneId);
    _updateToolbarStats();
}


export function renderPaneWindow(paneId, { targetIdx }) {
    _renderVirtualWindow(paneId, { targetIdx, forceAtBottom: false });
}
export function appendLine(paneId, ts, rawText, isTx, meta = null) {
    appendLineBatch([{ paneId, ts, rawText, isTx, meta }]);
}

export function appendLineBatch(entries) {
    const touched = new Set();
    const newByPane = new Map();

    entries.forEach(({ paneId, ts, rawText, isTx, meta = null }) => {
        if (!state.rawLines[paneId]) return;
        noteRelativeTimestampCandidate(meta);
        state.rawLines[paneId].push([ts, rawText, isTx, meta]);
        if (!newByPane.has(paneId)) newByPane.set(paneId, []);
        newByPane.get(paneId).push([ts, rawText, isTx, meta]);
        touched.add(paneId);
    });

    for (const [paneId, batch] of newByPane) {
        _recordPaneBytes(paneId, batch);
        _updatePaneStats(paneId);
    }

    touched.forEach(paneId => {
        const lines = state.rawLines[paneId];
        if (!lines || !lines.length) return;
        const vp = _getVirtual(paneId);
        const logEl = document.getElementById("log-" + paneId);
        if (!logEl || !vp) return;

        const rowH = _measureRowHeight(paneId);
        const midOrdinal = Math.max(0, Math.floor((logEl.scrollTop + logEl.clientHeight / 2) / rowH));
        const targetIdx = state.atBottom[paneId]
            ? lines.length - 1
            : (_rawIndexAt(paneId, vp, midOrdinal) ?? Math.min(lines.length - 1, midOrdinal));
        _renderVirtualWindow(paneId, { targetIdx, forceAtBottom: state.atBottom[paneId] });

        updateJumpBtn(paneId);
    });

    if (touched.size > 0) {
        _updateToolbarStats();
        window.__embedLogSchedulePersist?.();
        window.__embedLogUpdateTimestampModeUi?.();
        window.applyMarkers?.();
    }
}

export function rerenderPane(paneId) {
    const logEl = document.getElementById("log-" + paneId);
    if (!logEl) return;

    const vp = _getVirtual(paneId);
    if (!vp) return;

    const lines = state.rawLines[paneId] || [];
    if (!lines.length) {
        _renderVirtualWindow(paneId, { targetIdx: 0 });
        return;
    }

    if (state.filters[paneId]) {
        logEl.scrollTop = 0;
        _renderVirtualWindow(paneId, { targetIdx: 0 });
        return;
    }

    const rowH = _measureRowHeight(paneId);
    const midOrdinal = Math.max(0, Math.floor((logEl.scrollTop + logEl.clientHeight / 2) / rowH));
    const targetIdx = state.atBottom[paneId]
        ? lines.length - 1
        : Math.min(lines.length - 1, midOrdinal);
    _renderVirtualWindow(paneId, { targetIdx, forceAtBottom: state.atBottom[paneId] });
}
window.__embedLogInvalidateVirtualMetrics = function () {
    PANES.forEach(paneId => {
        const vp = _virtualPanes.get(paneId);
        if (!vp) return;
        vp.rowHeight = 0;
        rerenderPane(paneId);
    });
};
export function reanalyzePanePlugins(paneId) {
    const lines = state.rawLines[paneId] || [];
    lines.forEach(entry => {
        if (!_isRawTuple(entry)) analyzeLinePlugins(paneId, entry);
    });
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
        lines.forEach(entry => {
            if (!_isRawTuple(entry)) applyTimestampModeToLine(entry);
        });
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
        for (const entry of lines) {
            if (_isRawTuple(entry)) {
                const meta = entry[3];
                if (meta && typeof meta === "object") {
                    if (mode === "relative" && (meta.relTs || meta.relNum !== undefined)) return true;
                    if (mode === "absolute" && (meta.absTs || meta.absNum !== undefined || meta.timestampIso)) return true;
                }
                if (mode === "relative" && typeof entry[0] === "string" && entry[0].startsWith("T+")) return true;
            } else {
                if (lineHasTimestampMode(entry, mode)) return true;
            }
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
    const lines = state.rawLines[paneId];
    const len = lines ? lines.length : 0;
    if (len) {
        _renderVirtualWindow(paneId, { targetIdx: len - 1, forceAtBottom: true });
    } else {
        logEl.scrollTop = logEl.scrollHeight;
    }
    state.atBottom[paneId] = true;
    updateJumpBtn(paneId);
}
export function _linesSetupPane(id) {
    const logEl = document.getElementById("log-" + id);
    if (window.ResizeObserver && logEl.dataset.resizeObserverBound !== "1") {
        logEl.dataset.resizeObserverBound = "1";
        const resizeObserver = new ResizeObserver(() => _scheduleVirtualResizeRefresh());
        resizeObserver.observe(logEl);
    }
    logEl.addEventListener("scroll", () => {
        state.atBottom[id] = logEl.scrollHeight - logEl.scrollTop - logEl.clientHeight < 40;
        updateJumpBtn(id);

        const vp = _virtualPanes.get(id);
        if (!vp || vp.rendered.size === 0) return;

        const viewMid = logEl.scrollTop + logEl.clientHeight / 2;
        const rowH = vp.rowHeight > 0 ? vp.rowHeight : _measureRowHeight(id);
        const totalCount = _visibleCount(id, vp);
        if (!totalCount) return;
        let midOrdinal = Math.round(viewMid / rowH);
        midOrdinal = Math.max(0, Math.min(totalCount - 1, midOrdinal));

        const firstOrdinal = Number.isFinite(vp.firstOrdinal) ? vp.firstOrdinal : 0;
        const lastOrdinal = Number.isFinite(vp.lastOrdinal) ? vp.lastOrdinal : firstOrdinal;
        const rangeCenter = firstOrdinal + Math.floor((lastOrdinal - firstOrdinal) / 2);
        if (Math.abs(midOrdinal - rangeCenter) > OVERSCAN) {
            if (_pendingRaf.has(id)) return;
            _pendingRaf.set(id, true);
            requestAnimationFrame(() => {
                _pendingRaf.delete(id);
                const targetIdx = _rawIndexAt(id, vp, midOrdinal);
                if (targetIdx !== undefined) _renderVirtualWindow(id, { targetIdx });
            });
        }
    });
    // Event delegation — replaces per-line listeners for better performance
    logEl.addEventListener("click", e => {
        const lineDiv = e.target.closest(".log-line");
        if (!lineDiv) return;
        const idx = parseInt(lineDiv.dataset.idx, 10);
        const line = getLine(id, idx);
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
        const line = getLine(id, idx);
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
            // Rows are absolutely positioned assuming a fixed single-line height;
            // wrapping makes row height variable, so force a relayout.
            rerenderPane(id);
        });
    }

    // Per-pane raw download
    const dlBtn = document.querySelector(`#pane-${id} .pane-download-btn`);
    if (dlBtn) {
        dlBtn.addEventListener("click", () => {
            const lines = state.rawLines[id] || [];
            if (!lines.length) return;
            const text = lines.map(entry => {
                const rawText = _isRawTuple(entry) ? entry[1] : entry.rawText;
                const ts = _isRawTuple(entry) ? entry[0] : entry.ts;
                const clean = (rawText ?? "").replace(/\x1b(?:\[[0-9;]*[A-Za-z]|\][^\x07]*\x07|[^[\]])/g, "").trim();
                return `[${ts}] ${clean}`;
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
    el.title = refs.map(r => r.name).join("\n") + (configurable ? "\n\nHover or click to configure" : "");
    el.classList.toggle("configurable", configurable);
    el.tabIndex = configurable ? 0 : -1;
    el.setAttribute("aria-label", configurable ? "Configure pane plugins" : "Active pane plugins");
    if (el.dataset.hoverBound === "1") return;
    el.dataset.hoverBound = "1";
    el.addEventListener("mouseenter", () => _showPluginInfo(paneId, el));
    el.addEventListener("mouseleave", () => _schedulePluginInfoHide());
    el.addEventListener("focus", () => _showPluginInfo(paneId, el));
    el.addEventListener("blur", () => _schedulePluginInfoHide(0));
    el.addEventListener("click", ev => {
        ev.stopPropagation();
        _cancelPluginInfoHide();
        _showPluginInfo(paneId, el);
        _pluginInfoPinned = !_pluginInfoPinned;
    });
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
    const logEl = document.getElementById("log-" + paneId);
    if (logEl) {
        const windowEl = logEl.querySelector(".log-window");
        if (windowEl) windowEl.innerHTML = "";
        const spacerEl = logEl.querySelector(".log-spacer");
        if (spacerEl) spacerEl.style.height = "0";
    }
    _virtualPanes.delete(paneId);
    highlightLine(paneId, null);
    _resetPaneStats(paneId);
    hidePluginOverlays();
    state.atBottom[paneId] = true;
    updateJumpBtn(paneId);
    document.getElementById("copy-actions-" + paneId)?.classList.remove("visible");
    document.getElementById("more-dropdown-" + paneId)?.classList.remove("open");
    window.__embedLogSchedulePersist?.();
    window.__embedLogUpdateTimestampModeUi?.();
}

document.getElementById("btn-jump-all")?.addEventListener("click", () => {
    PANES.forEach(scrollPaneToBottom);
});

document.getElementById("btn-clear")?.addEventListener("click", () => {
    window.wsSend?.({ cmd: "clear_logs", scope: "all" });
    window.__embedLogDiscardPendingLogMessages?.();
    resetRelativeTimestampBase();
    state.syncTs = null;
    state.syncTabSwitch = false;
    PANES.forEach(clearPane);
    _updateToolbarStats();
});


// Rebuild DOM for a pane from stored state — used after layout rebuild (UNWRAP toggle)
export function repopulatePaneLogs(paneId) {
    const logEl = document.getElementById("log-" + paneId);
    if (!logEl) return;
    const windowEl = logEl.querySelector(".log-window");
    if (windowEl) windowEl.innerHTML = "";
    const spacerEl = logEl.querySelector(".log-spacer");
    if (spacerEl) spacerEl.style.height = "0";
    _virtualPanes.delete(paneId);

    const lines = state.rawLines[paneId] || [];
    if (lines.length) {
        const targetIdx = state.atBottom[paneId] ? lines.length - 1 : 0;
        _renderVirtualWindow(paneId, { targetIdx, forceAtBottom: state.atBottom[paneId] });
    }
    if (state.atBottom[paneId] && logEl) logEl.scrollTop = logEl.scrollHeight;
    updateJumpBtn(paneId);
    _updatePaneStats(paneId);
    _updateToolbarStats();
}
// ---------------------------------------------------------------------------
// Sync
// ---------------------------------------------------------------------------

export function highlightLine(paneId, div) {
    const prev = state.highlighted[paneId];
    if (prev && prev !== div) prev.classList.remove("sync-highlight");

    if (!div) {
        state.highlighted[paneId] = null;
        state.highlightedIdx[paneId] = null;
        return;
    }

    const idx = parseInt(div.dataset.idx, 10);
    state.highlighted[paneId] = div;
    state.highlightedIdx[paneId] = Number.isFinite(idx) ? idx : null;
    div.classList.add("sync-highlight");
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
        if (_getNumTs(lines[mid], paneId, mid) < numTs) lo = mid + 1;
        else hi = mid;
    }
    if (lo > 0 && Math.abs(_getNumTs(lines[lo - 1], paneId, lo - 1) - numTs) < Math.abs(_getNumTs(lines[lo], paneId, lo) - numTs)) lo--;

    _scrollPaneToRawIndex(paneId, lo);
}

function _entryServerLineIdx(entry, paneId, idx) {
    if (_isRawTuple(entry)) {
        const meta = entry[3];
        if (meta && typeof meta === "object" && Number.isFinite(meta.lineIdx)) return meta.lineIdx;
        return null;
    }
    return Number.isFinite(entry?.serverLineIdx) ? entry.serverLineIdx : null;
}

function _scrollPaneToRawIndex(paneId, rawIdx) {
    _renderVirtualWindow(paneId, { targetIdx: rawIdx });
    const div = getRenderedLineElement(paneId, rawIdx);
    if (!div) return;

    const logEl = document.getElementById("log-" + paneId);
    logEl.scrollTop = Math.max(0, div.offsetTop - Math.floor(logEl.clientHeight / 3));
    state.atBottom[paneId] = false;
    updateJumpBtn(paneId);
    highlightLine(paneId, div);
}

export function scrollPaneToLineIdx(paneId, lineIdx, fallbackNumTs = null) {
    const targetLineIdx = Number(lineIdx);
    const lines = state.rawLines[paneId];
    if (!lines?.length || !Number.isFinite(targetLineIdx)) {
        if (fallbackNumTs !== null) scrollPaneToTs(paneId, fallbackNumTs);
        return;
    }

    for (let idx = 0; idx < lines.length; idx++) {
        if (_entryServerLineIdx(lines[idx], paneId, idx) === targetLineIdx) {
            _scrollPaneToRawIndex(paneId, idx);
            return;
        }
    }

    if (fallbackNumTs !== null) scrollPaneToTs(paneId, fallbackNumTs);
}

// Middle-click: always clear the filter for this pane, scroll to the line
// in full context, and sync — the deliberate "zoom out to this moment" gesture.
export function onMiddleClick(paneId, numTs, div) {
    const logEl = document.getElementById("log-" + paneId);
    if (!logEl) return;

    let activeDiv = div;
    const rawIdx = parseInt(div?.dataset?.idx, 10);

    if (state.filters[paneId]) {
        const input = document.querySelector(`.filter-input[data-pane="${paneId}"]`);
        if (input) {
            input.value = "";
            input.classList.remove("invalid");
        }
        state.filters[paneId] = null;
        if (Number.isFinite(rawIdx)) {
            ensureLineVisible(paneId, rawIdx, { align: "top" });
            activeDiv = getRenderedLineElement(paneId, rawIdx) || div;
        } else {
            rerenderPane(paneId);
        }
    }

    logEl.scrollTop = activeDiv.offsetTop - Math.floor(logEl.clientHeight / 3);
    state.atBottom[paneId] = false;
    updateJumpBtn(paneId);

    state.syncTs = numTs;
    state.syncTabSwitch = true;
    highlightLine(paneId, activeDiv);
    syncPanes(paneId, numTs, activeDiv);
}

// Click handler:
//   • filter active  → clear filter, re-render, scroll source to line in context
//   • no filter      → source pane stays exactly where user was (no scroll)
//   • always         → store syncTs, highlight clicked line, sync other panes in active tab
export function onLineClick(paneId, numTs, div) {
    const logEl = document.getElementById("log-" + paneId);
    if (!logEl) return;

    let activeDiv = div;
    const rawIdx = parseInt(div?.dataset?.idx, 10);

    if (state.filters[paneId]) {
        const filterInput = document.querySelector(`.filter-input[data-pane="${paneId}"]`);
        if (filterInput) {
            filterInput.value = "";
            filterInput.classList.remove("invalid");
        }
        state.filters[paneId] = null;
        if (Number.isFinite(rawIdx)) {
            ensureLineVisible(paneId, rawIdx, { align: "top" });
            activeDiv = getRenderedLineElement(paneId, rawIdx) || div;
            logEl.scrollTop = activeDiv.offsetTop - Math.floor(logEl.clientHeight / 3);
        } else {
            rerenderPane(paneId);
        }
    }

    state.atBottom[paneId] = false;
    updateJumpBtn(paneId);

    state.syncTs = numTs;
    state.syncTabSwitch = true;
    highlightLine(paneId, activeDiv);
    syncPanes(paneId, numTs, activeDiv);
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
            if (_getNumTs(lines[mid], toId, mid) < numTs) lo = mid + 1;
            else hi = mid;
        }
        if (lo > 0 && Math.abs(_getNumTs(lines[lo - 1], toId, lo - 1) - numTs) < Math.abs(_getNumTs(lines[lo], toId, lo) - numTs)) {
            lo--;
        }

        _renderVirtualWindow(toId, { targetIdx: lo });
        const targetDiv = getRenderedLineElement(toId, lo);
        if (!targetDiv) return;

        const logEl = document.getElementById("log-" + toId);
        logEl.scrollTop = targetDiv.offsetTop - clickedRelTop;
        state.atBottom[toId] = false;
        updateJumpBtn(toId);
        highlightLine(toId, targetDiv);
    });
}
