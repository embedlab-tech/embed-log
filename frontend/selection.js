import { state, PANES } from './state.js';
import { onLineClick } from './lines.js';
import { exportHtmlSnapshot } from './export.js';

// ---------------------------------------------------------------------------
// Line selection + copy
//
// Copy       -> copies only current selection to system clipboard.
// Clipboard add -> appends selection to internal buffer (peek/copy-all via 📋).
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Inject clipboard indicator into toolbar (before ws-status)
// ---------------------------------------------------------------------------
(function () {
    const wsStatus = document.getElementById("ws-status");
    if (!wsStatus) return;

    const ind = document.createElement("div");
    ind.id = "clip-indicator";
    ind.style.display = "none";

    const span = document.createElement("span");
    span.className = "clip-count";
    span.title = "Peek clipboard buffer";
    span.addEventListener("click", e => { e.stopPropagation(); _toggleClipPeek(); });
    ind.appendChild(span);

    const sep = document.createElement("span");
    sep.className = "clip-sep";
    sep.textContent = "·";
    ind.appendChild(sep);

    const peekBtn = document.createElement("button");
    peekBtn.id = "clip-peek-btn";
    peekBtn.className = "clip-peek";
    peekBtn.textContent = "Peek";
    peekBtn.title = "Show clipboard buffer";
    peekBtn.addEventListener("click", e => { e.stopPropagation(); _toggleClipPeek(); });
    ind.appendChild(peekBtn);

    const clearBtn = document.createElement("button");
    clearBtn.className = "clip-clear";
    clearBtn.textContent = "Clear";
    clearBtn.title = "Clear clipboard buffer";
    clearBtn.addEventListener("click", _clearClipBuffer);
    ind.appendChild(clearBtn);

    const menu = document.createElement("div");
    menu.id = "clip-peek-menu";
    menu.innerHTML = `
        <div class="clip-peek-head">
            <span>Clipboard buffer</span>
            <button type="button" class="clip-peek-copyall" title="Copy full buffered clipboard content">Copy all</button>
        </div>
        <pre class="clip-peek-body"></pre>
    `;
    menu.querySelector(".clip-peek-copyall")?.addEventListener("click", e => {
        e.stopPropagation();
        _copyClipBuffer();
    });
    document.body.appendChild(menu);

    wsStatus.before(ind);
})();

// ---------------------------------------------------------------------------
// Inject per-pane copy actions
// ---------------------------------------------------------------------------
export function _selectionSetupPane(id) {
    const body = document.querySelector(`#pane-${id} .pane-body`);
    if (!body) return;

    const wrap = document.createElement("div");
    wrap.className = "copy-actions";
    wrap.id = "copy-actions-" + id;

    const addBtn = document.createElement("button");
    addBtn.className = "copy-btn";
    addBtn.id = "copy-add-" + id;
    addBtn.textContent = "Clipboard add";
    addBtn.title = "Append selected lines from this pane to internal clipboard buffer";
    addBtn.addEventListener("click", e => { e.stopPropagation(); _addSelectedToBuffer(id); });

    const copyBtn = document.createElement("button");
    copyBtn.className = "copy-btn";
    copyBtn.id = "copy-" + id;
    copyBtn.textContent = "Copy";
    copyBtn.title = "Copy selected lines from this pane to system clipboard";
    copyBtn.addEventListener("click", e => { e.stopPropagation(); _copySelectedDirect(id); });

    const rangeCopyBtn = document.createElement("button");
    rangeCopyBtn.className = "copy-btn";
    rangeCopyBtn.id = "copy-range-" + id;
    rangeCopyBtn.textContent = "Copy range";
    rangeCopyBtn.title = "Copy synchronized raw snippet from all panes in this selected time range";
    rangeCopyBtn.addEventListener("click", e => { e.stopPropagation(); _copyRangeRaw(id); });

    const rangeRawBtn = document.createElement("button");
    rangeRawBtn.className = "copy-btn";
    rangeRawBtn.id = "download-range-raw-" + id;
    rangeRawBtn.textContent = "Raw file";
    rangeRawBtn.title = "Download synchronized raw snippet from all panes in this selected time range";
    rangeRawBtn.addEventListener("click", e => { e.stopPropagation(); _downloadRangeRaw(id); });

    const rangeHtmlBtn = document.createElement("button");
    rangeHtmlBtn.className = "copy-btn";
    rangeHtmlBtn.id = "download-range-html-" + id;
    rangeHtmlBtn.textContent = "HTML snippet";
    rangeHtmlBtn.title = "Download a self-contained HTML snippet for this selected time range";
    rangeHtmlBtn.addEventListener("click", e => { e.stopPropagation(); _downloadRangeHtml(id); });

    wrap.appendChild(addBtn);
    wrap.appendChild(copyBtn);
    wrap.appendChild(rangeCopyBtn);
    wrap.appendChild(rangeRawBtn);
    wrap.appendChild(rangeHtmlBtn);
    body.appendChild(wrap);
}
PANES.forEach(_selectionSetupPane);

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------
function _stripHtml(str) { return str.replace(/<[^>]+>/g, ""); }

function _syncCopyBtn(paneId) {
    const wrap = document.getElementById("copy-actions-" + paneId);
    const addBtn = document.getElementById("copy-add-" + paneId);
    const copyBtn = document.getElementById("copy-" + paneId);
    const rangeCopyBtn = document.getElementById("copy-range-" + paneId);
    const rangeRawBtn = document.getElementById("download-range-raw-" + paneId);
    const rangeHtmlBtn = document.getElementById("download-range-html-" + paneId);
    if (!wrap || !addBtn || !copyBtn) return;

    const count = state.selected[paneId].size;
    const visible = count > 0;
    wrap.classList.toggle("visible", visible);
    wrap.querySelectorAll(".copy-btn").forEach(btn => btn.classList.toggle("visible", visible));

    if (visible) {
        addBtn.textContent = `Clipboard add (${count})`;
        copyBtn.textContent = `Copy (${count})`;
        if (rangeCopyBtn) rangeCopyBtn.textContent = "Copy range";
        if (rangeRawBtn) rangeRawBtn.textContent = "Raw file";
        if (rangeHtmlBtn) rangeHtmlBtn.textContent = "HTML snippet";
    }
}

function _applySelection(paneId) {
    const logEl = document.getElementById("log-" + paneId);
    const sel = state.selected[paneId];
    Array.from(logEl.children).forEach((div, i) =>
        div.classList.toggle("selected", sel.has(i))
    );
    _syncCopyBtn(paneId);
}

function _selectIndexRange(paneId, startIdx, endIdx) {
    if (!Number.isFinite(startIdx) || !Number.isFinite(endIdx)) return;
    _clearOtherSelections(paneId);
    const lines = state.rawLines[paneId] || [];
    const lo = Math.max(0, Math.min(startIdx, endIdx));
    const hi = Math.min(lines.length - 1, Math.max(startIdx, endIdx));
    const sel = new Set();
    for (let i = lo; i <= hi; i++) sel.add(i);
    state.selected[paneId] = sel;
    _applySelection(paneId);
}

function _clearOtherSelections(keepPane) {
    PANES.forEach(id => {
        if (id === keepPane || !state.selected[id].size) return;
        state.selected[id] = new Set();
        _applySelection(id);
    });
}

function _clearAllSelections() {
    PANES.forEach(id => {
        if (state.selected[id]?.size) {
            state.selected[id] = new Set();
            _applySelection(id);
        }
        _syncCopyBtn(id);
    });
}

function _decodeEntities(text) {
    const ta = document.createElement("textarea");
    ta.innerHTML = text;
    return ta.value;
}

function _linePlain(line) {
    return _decodeEntities(_stripHtml(line?.html || "")).replace(/\s+/g, " ").trim();
}

function _lineRaw(line) {
    return `${line.ts}  ${_linePlain(line)}`;
}

function _safeFilePart(str) {
    return String(str || "snippet").replace(/[^0-9A-Za-z_.-]+/g, "-").replace(/^-+|-+$/g, "") || "snippet";
}

function _downloadText(filename, text, type = "text/plain") {
    const blob = new Blob([text], { type });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = filename;
    a.click();
    URL.revokeObjectURL(url);
}

function _copyText(text) {
    if (navigator.clipboard?.writeText) return navigator.clipboard.writeText(text);
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.style.position = "fixed";
    ta.style.left = "-9999px";
    document.body.appendChild(ta);
    ta.focus();
    ta.select();
    try { document.execCommand("copy"); }
    finally { ta.remove(); }
    return Promise.resolve();
}

function _selectionRange(paneId) {
    const sel = state.selected[paneId];
    if (!sel?.size) return null;
    const lines = state.rawLines[paneId] || [];
    const nums = Array.from(sel)
        .map(i => lines[i]?.numTs)
        .filter(n => Number.isFinite(n) && n > 0);
    if (!nums.length) return null;
    return { from: Math.min(...nums), to: Math.max(...nums) };
}

function _collectRangeEntries(paneId) {
    const range = _selectionRange(paneId);
    if (!range) return [];
    const entries = [];
    PANES.forEach(id => {
        (state.rawLines[id] || []).forEach((line, idx) => {
            const n = line?.numTs;
            if (Number.isFinite(n) && n >= range.from && n <= range.to) {
                entries.push({ paneId: id, idx, line });
            }
        });
    });
    entries.sort((a, b) =>
        (a.line.numTs - b.line.numTs) || a.paneId.localeCompare(b.paneId) || (a.idx - b.idx)
    );
    return entries;
}

function _rangeBoundsLabel(entries) {
    if (!entries.length) return "snippet";
    const first = entries[0].line.ts;
    const last = entries[entries.length - 1].line.ts;
    return `${first}_to_${last}`;
}

function _escapeRegExp(str) {
    return String(str).replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function _snippetMessageText(entry) {
    let text = _linePlain(entry.line).trim();
    const sourcePrefix = new RegExp(`^\\[${_escapeRegExp(entry.paneId)}\\]\\s*`);
    // Demo/UDP logs can already contain their own timestamp/source prefix.
    // The merged snippet adds one normalized prefix, so strip duplicated ones
    // only when they are unambiguous.
    for (let i = 0; i < 4; i++) {
        const before = text;
        text = text
            .replace(/^\[\d{4}-\d{2}-\d{2}T[^\]]+\]\s*/, "")
            .replace(sourcePrefix, "")
            .trim();
        if (text === before) break;
    }
    return text;
}

function _formatRangeRaw(entries) {
    return entries.map(e => `[${e.line.ts}] [${e.paneId}] ${_snippetMessageText(e)}`).join("\n");
}

function _buildRangeLogData(entries) {
    const logData = {};
    PANES.forEach(id => { logData[id] = []; });
    entries.forEach(e => {
        if (!logData[e.paneId]) logData[e.paneId] = [];
        logData[e.paneId].push({
            ts: e.line.ts,
            text: _snippetMessageText(e),
            isTx: e.line.isTx,
        });
    });
    return logData;
}

function _formatSelectionBlock(paneId, indices) {
    const lines = state.rawLines[paneId];
    return indices
        .map(i => lines[i])
        .filter(Boolean)
        .map(_lineRaw)
        .join("\n");
}

// ---------------------------------------------------------------------------
// Clipboard accumulation buffer (only via "Clipboard add")
// ---------------------------------------------------------------------------
let _clipBuffer = "";
let _clipLineCount = 0;

function _clipIndicatorEl() { return document.getElementById("clip-indicator"); }

function _updateClipIndicator() {
    const el = _clipIndicatorEl();
    if (!el) return;
    el.style.display = _clipLineCount > 0 ? "" : "none";
    const span = el.querySelector(".clip-count");
    if (span) span.textContent = `📋 ${_clipLineCount} lines`;
    const peekBtn = document.getElementById("clip-peek-btn");
    if (peekBtn) peekBtn.disabled = _clipLineCount <= 0;
}

function _clearClipBuffer() {
    _clipBuffer = "";
    _clipLineCount = 0;
    _updateClipIndicator();
    _renderClipPeek();
    _closeClipPeek();
}

function _clipPeekMenuEl() { return document.getElementById("clip-peek-menu"); }

function _isClipPeekOpen() {
    return _clipPeekMenuEl()?.classList.contains("open") ?? false;
}

function _renderClipPeek() {
    const menu = _clipPeekMenuEl();
    if (!menu) return;
    const body = menu.querySelector(".clip-peek-body");
    if (!body) return;
    body.textContent = _clipBuffer || "(Clipboard buffer is empty)";
    const copyAllBtn = menu.querySelector(".clip-peek-copyall");
    if (copyAllBtn) copyAllBtn.disabled = _clipLineCount <= 0;
}

function _copyClipBuffer() {
    if (!_clipBuffer) return;
    const menu = _clipPeekMenuEl();
    const btn = menu?.querySelector(".clip-peek-copyall");
    navigator.clipboard.writeText(_clipBuffer).then(() => {
        if (!btn) return;
        const prev = btn.textContent;
        btn.textContent = "Copied";
        btn.disabled = true;
        setTimeout(() => {
            btn.textContent = prev;
            btn.disabled = _clipLineCount <= 0;
        }, 900);
    }).catch(() => {});
}

function _openClipPeek() {
    if (_clipLineCount <= 0) return;
    const menu = _clipPeekMenuEl();
    const ind = document.getElementById("clip-indicator");
    if (!menu || !ind) return;
    _renderClipPeek();
    const rect = ind.getBoundingClientRect();
    menu.style.left = `${Math.max(8, rect.left)}px`;
    menu.style.top = `${rect.bottom + 6}px`;
    menu.classList.add("open");
}

function _closeClipPeek() {
    _clipPeekMenuEl()?.classList.remove("open");
}

function _toggleClipPeek() {
    if (_isClipPeekOpen()) _closeClipPeek();
    else _openClipPeek();
}

// ---------------------------------------------------------------------------
// Copy actions
// ---------------------------------------------------------------------------
function _addSelectedToBuffer(paneId) {
    const sel = state.selected[paneId];
    if (!sel.size) return;

    const indices = Array.from(sel).sort((a, b) => a - b);
    const text = _formatSelectionBlock(paneId, indices);
    if (!text) return;

    const isFirst = _clipBuffer === "";
    _clipBuffer += (isFirst ? "" : "\n\n\n\n") + text;
    _clipLineCount += sel.size;

    _updateClipIndicator();
    _renderClipPeek();

    const btn = document.getElementById("copy-add-" + paneId);
    if (!btn) return;
    const prev = btn.textContent;
    btn.textContent = `Added (${_clipLineCount})`;
    setTimeout(() => { btn.textContent = prev; _syncCopyBtn(paneId); }, 900);
}

function _copySelectedDirect(paneId) {
    const sel = state.selected[paneId];
    if (!sel.size) return;

    const indices = Array.from(sel).sort((a, b) => a - b);
    const text = _formatSelectionBlock(paneId, indices);
    if (!text) return;

    _copyText(text).then(() => {
        const btn = document.getElementById("copy-" + paneId);
        if (!btn) return;
        const prev = btn.textContent;
        btn.textContent = "Copied";
        setTimeout(() => { btn.textContent = prev; _syncCopyBtn(paneId); }, 900);
    }).catch(() => {});
}

function _copyRangeRaw(paneId) {
    const entries = _collectRangeEntries(paneId);
    const text = _formatRangeRaw(entries);
    if (!text) return;
    _copyText(text).then(() => {
        const btn = document.getElementById("copy-range-" + paneId);
        if (!btn) return;
        const prev = btn.textContent;
        btn.textContent = "Copied";
        setTimeout(() => { btn.textContent = prev; _syncCopyBtn(paneId); }, 900);
    }).catch(() => {});
}

function _downloadRangeRaw(paneId) {
    const entries = _collectRangeEntries(paneId);
    const text = _formatRangeRaw(entries);
    if (!text) return;
    const name = `embed-log-snippet-${_safeFilePart(_rangeBoundsLabel(entries))}.log`;
    _downloadText(name, text + "\n", "text/plain");
}

function _downloadRangeHtml(paneId) {
    const entries = _collectRangeEntries(paneId);
    if (!entries.length) return;
    const btn = document.getElementById("download-range-html-" + paneId);
    exportHtmlSnapshot({
        button: btn,
        logData: _buildRangeLogData(entries),
        filenamePrefix: `embed-log-snippet-${_safeFilePart(_rangeBoundsLabel(entries))}`,
        title: `snippet ${_rangeBoundsLabel(entries)}`,
        activeTab: state.activeTab,
    });
}

// ---------------------------------------------------------------------------
// Pointer drag — selection
// ---------------------------------------------------------------------------
let _drag = null;          // { paneId, startIdx, startY, lineEl, active }
let _suppressClick = false;
let _rangeAnchor = null;   // { paneId, idx } set by a normal line click for Shift+Click range selection

document.addEventListener("pointerdown", e => {
    if (e.button !== 0) return;
    const line = e.target.closest(".log-line");
    if (!line) return;
    const logArea = line.closest(".log-area");
    if (!logArea) return;

    _drag = {
        paneId: logArea.id.slice(4),
        startIdx: parseInt(line.dataset.idx, 10),
        startY: e.clientY,
        lineEl: line,
        active: false,
    };
    _suppressClick = false;
});

document.addEventListener("pointermove", e => {
    if (!_drag) return;
    if (Math.abs(e.clientY - _drag.startY) < 6) return;

    if (!_drag.active) {
        _drag.active = true;
        _suppressClick = true;

        _clearOtherSelections(_drag.paneId);

        const raw = state.rawLines[_drag.paneId][_drag.startIdx];
        if (raw) onLineClick(_drag.paneId, raw.numTs, _drag.lineEl);

        try { _drag.lineEl.setPointerCapture(e.pointerId); } catch (_) {}
    }

    const el = document.elementFromPoint(e.clientX, e.clientY);
    if (!el) return;
    const line = el.closest(".log-line");
    if (!line) return;
    const logArea = line.closest(".log-area");
    if (!logArea || logArea.id.slice(4) !== _drag.paneId) return;

    const endIdx = parseInt(line.dataset.idx, 10);
    const lo = Math.min(_drag.startIdx, endIdx);
    const hi = Math.max(_drag.startIdx, endIdx);
    const sel = new Set();
    for (let i = lo; i <= hi; i++) sel.add(i);
    state.selected[_drag.paneId] = sel;
    _applySelection(_drag.paneId);
});

document.addEventListener("pointerup", () => { _drag = null; });

document.addEventListener("click", e => {
    if (_suppressClick) {
        if (e.target.closest(".log-line")) {
            _suppressClick = false;
            e.stopPropagation();
            return;
        }
        _suppressClick = false;
    }

    const clickedLine = e.target.closest(".log-line");
    const clickedLogArea = clickedLine?.closest(".log-area");
    if (clickedLine && clickedLogArea) {
        const paneId = clickedLogArea.id.slice(4);
        const idx = parseInt(clickedLine.dataset.idx, 10);

        if (e.shiftKey && _rangeAnchor?.paneId === paneId && Number.isFinite(idx)) {
            e.preventDefault();
            e.stopPropagation();
            _selectIndexRange(paneId, _rangeAnchor.idx, idx);
            return;
        }

        if (!e.shiftKey && Number.isFinite(idx)) {
            _rangeAnchor = { paneId, idx };
        }
    }

    const inClipUi = e.target.closest("#clip-indicator") || e.target.closest("#clip-peek-menu");
    const inSelectionUi = e.target.closest(".copy-actions");
    if (_isClipPeekOpen() && !inClipUi) _closeClipPeek();

    if (!PANES.some(id => state.selected[id]?.size > 0)) return;
    if (inClipUi || inSelectionUi) return;
    _clearAllSelections();
}, true);

// ---------------------------------------------------------------------------
// Keyboard
// ---------------------------------------------------------------------------
document.addEventListener("keydown", e => {
    if ((e.ctrlKey || e.metaKey) && e.key === "c") {
        const pane = PANES.find(id => state.selected[id].size > 0);
        if (pane) { _copySelectedDirect(pane); e.preventDefault(); }
        return;
    }
    if (e.key === "Escape") {
        if (_isClipPeekOpen()) {
            _closeClipPeek();
            return;
        }
        _clearAllSelections();
    }
});
