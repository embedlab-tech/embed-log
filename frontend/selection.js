import { state, PANES, paneLabel, unwrapPaneLabel } from './state.js';
import { onLineClick } from './lines.js';
import { exportHtmlSnapshot } from './export.js';

// ---------------------------------------------------------------------------
// Line selection + copy / export actions
//
// Two explicit scopes (toggled per-pane overlay):
//   Exact   → only the user-selected lines in the active pane
//   Context → selected lines + synchronized lines from all panes
//
// Primary actions:
//   Copy         (respects current scope)
//   Download raw (respects current scope)
//
// Secondary actions (accessible via More ··· dropdown):
//   Export HTML  (respects current scope)
//   Add to clipboard (respects current scope)
//
// Keyboard Ctrl+C / Cmd+C always copies exact selection (predictable).
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
// Inject per-pane selection actions (scope toggle + actions)
// ---------------------------------------------------------------------------
export function _selectionSetupPane(id) {
    const body = document.querySelector(`#pane-${id} .pane-body`);
    if (!body) return;

    const wrap = document.createElement("div");
    wrap.className = "copy-actions";
    wrap.id = "copy-actions-" + id;

    // Scope toggle
    const scopeRow = document.createElement("div");
    scopeRow.className = "scope-row";

    const scopeExact = document.createElement("button");
    scopeExact.className = "scope-btn active";
    scopeExact.id = "scope-exact-" + id;
    scopeExact.textContent = "Exact";
    scopeExact.title = "Only selected lines in this pane";
    scopeExact.addEventListener("click", e => { e.stopPropagation(); _setScope(id, "exact"); });

    const scopeContext = document.createElement("button");
    scopeContext.className = "scope-btn";
    scopeContext.id = "scope-context-" + id;
    scopeContext.textContent = "All";
    scopeContext.title = "Selected lines + synchronized lines from all panes";
    scopeContext.addEventListener("click", e => { e.stopPropagation(); _setScope(id, "context"); });

    const scopeSel = document.createElement("button");
    scopeSel.className = "scope-btn";
    scopeSel.id = "scope-context-selected-" + id;
    scopeSel.textContent = "Sel…";
    scopeSel.title = "Selected lines + only chosen panes";
    scopeSel.addEventListener("click", e => { e.stopPropagation(); _setScope(id, "context-selected"); });

    scopeRow.appendChild(scopeExact);
    scopeRow.appendChild(scopeContext);
    scopeRow.appendChild(scopeSel);

    // Pane selector (lazily rebuilt when scope becomes context-selected)
    const paneSelector = document.createElement("div");
    paneSelector.className = "pane-selector";
    paneSelector.id = "pane-selector-" + id;
    paneSelector.style.display = "none";


    // Primary action row
    const actionRow = document.createElement("div");
    actionRow.className = "action-row";

    const copyBtn = document.createElement("button");
    copyBtn.className = "copy-btn";
    copyBtn.id = "copy-" + id;
    copyBtn.addEventListener("click", e => { e.stopPropagation(); _copy(id); });

    const rawBtn = document.createElement("button");
    rawBtn.className = "copy-btn";
    rawBtn.id = "download-raw-" + id;
    rawBtn.addEventListener("click", e => { e.stopPropagation(); _downloadRaw(id); });

    // More ··· toggle
    const moreToggle = document.createElement("button");
    moreToggle.className = "copy-btn more-toggle";
    moreToggle.id = "more-toggle-" + id;
    moreToggle.textContent = "\u00B7\u00B7\u00B7";
    moreToggle.title = "More actions";
    moreToggle.addEventListener("click", e => { e.stopPropagation(); _toggleMore(id); });

    // Secondary actions dropdown
    const moreDropdown = document.createElement("div");
    moreDropdown.className = "more-dropdown";
    moreDropdown.id = "more-dropdown-" + id;

    const htmlBtn = document.createElement("button");
    htmlBtn.className = "copy-btn";
    htmlBtn.id = "export-html-" + id;
    htmlBtn.textContent = "Export HTML";
    htmlBtn.title = "Export selection to self-contained HTML file";
    htmlBtn.addEventListener("click", e => { e.stopPropagation(); _exportHtml(id); });

    const clipAddBtn = document.createElement("button");
    clipAddBtn.className = "copy-btn";
    clipAddBtn.id = "clip-add-" + id;
    clipAddBtn.textContent = "Add to clipboard";
    clipAddBtn.title = "Append selected lines to internal clipboard buffer";
    clipAddBtn.addEventListener("click", e => { e.stopPropagation(); _addSelectedToBuffer(id); });

    moreDropdown.appendChild(htmlBtn);
    moreDropdown.appendChild(clipAddBtn);

    actionRow.appendChild(copyBtn);
    actionRow.appendChild(rawBtn);
    actionRow.appendChild(moreToggle);
    actionRow.appendChild(moreDropdown);

    wrap.appendChild(scopeRow);
    wrap.appendChild(paneSelector);
    wrap.appendChild(actionRow);
    body.appendChild(wrap);
}

function _rebuildPaneSelector(paneId) {
    const container = document.getElementById('pane-selector-' + paneId);
    if (!container) return;
    container.innerHTML = '';
    PANES.forEach(id => {
        const label = document.createElement('label');
        label.className = 'pane-checkbox';
        const cb = document.createElement('input');
        cb.type = 'checkbox';
        cb.dataset.pane = id;
        cb.checked = state.contextPanes[id] !== false;
        cb.addEventListener('change', e => {
            e.stopPropagation();
            state.contextPanes[id] = cb.checked;
        });
        label.appendChild(cb);
        label.appendChild(document.createTextNode(' ' + unwrapPaneLabel(id)));
        container.appendChild(label);
    });
}

PANES.forEach(_selectionSetupPane);

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------
function _stripHtml(str) { return str.replace(/<[^>]+>/g, ""); }

function _setScope(paneId, scope) {
    state.selectionScope = scope;
    if (scope === 'context-selected' && Object.keys(state.contextPanes).length === 0) {
        PANES.forEach(id => { state.contextPanes[id] = true; });
    }
    PANES.forEach(id => {
        ['exact', 'context', 'context-selected'].forEach(s => {
            const btn = document.getElementById(`scope-${s}-${id}`);
            if (btn) btn.classList.toggle('active', scope === s);
        });
        const ps = document.getElementById('pane-selector-' + id);
        if (!ps) return;
        if (scope === 'context-selected') {
            _rebuildPaneSelector(id);
            ps.style.display = '';
        } else {
            ps.style.display = 'none';
        }
    });
    _syncSelectionActions(paneId);
}

function _toggleMore(paneId) {
    const dd = document.getElementById("more-dropdown-" + paneId);
    if (!dd) return;
    const isOpen = dd.classList.contains("open");
    PANES.forEach(id => {
        document.getElementById("more-dropdown-" + id)?.classList.remove("open");
    });
    if (!isOpen) dd.classList.add("open");
}

function _closeAllMore() {
    PANES.forEach(id => {
        document.getElementById("more-dropdown-" + id)?.classList.remove("open");
    });
}

function _syncSelectionActions(paneId) {
    const wrap = document.getElementById("copy-actions-" + paneId);
    const copyBtn = document.getElementById("copy-" + paneId);
    const rawBtn = document.getElementById("download-raw-" + paneId);
    if (!wrap || !copyBtn) return;

    const count = state.selected[paneId].size;
    const visible = count > 0;
    wrap.classList.toggle("visible", visible);
    wrap.querySelectorAll(".copy-btn, .scope-btn, .more-toggle").forEach(el =>
        el.classList.toggle("visible", visible)
    );

    if (visible) {
        copyBtn.textContent = `Copy (${count})`;
        rawBtn.textContent = `Download raw`;
    }
}

function _applySelection(paneId) {
    const logEl = document.getElementById("log-" + paneId);
    const sel = state.selected[paneId];
    Array.from(logEl.children).forEach((div, i) =>
        div.classList.toggle("selected", sel.has(i))
    );
    _syncSelectionActions(paneId);
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
        _syncSelectionActions(id);
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
    const targetPanes = state.selectionScope === 'context-selected'
        ? PANES.filter(id => state.contextPanes[id])
        : PANES;
    targetPanes.forEach(id => {
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

function _rangeBoundsLabelExact(paneId) {
    const sel = state.selected[paneId];
    if (!sel?.size) return "snippet";
    const lines = state.rawLines[paneId] || [];
    const indices = Array.from(sel).sort((a, b) => a - b);
    const first = lines[indices[0]]?.ts;
    const last = lines[indices[indices.length - 1]]?.ts;
    if (!first || !last) return "snippet";
    return `${first}_to_${last}`;
}

function _escapeRegExp(str) {
    return String(str).replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function _snippetMessageText(entry) {
    let text = _linePlain(entry.line).trim();
    const sourcePrefix = new RegExp(`^\\[${_escapeRegExp(entry.paneId)}\\]\\s*`);
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
            text: `[${e.paneId}] ${_snippetMessageText(e)}`,
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
        .map(line => `${line.ts}  [${paneId}] ${_linePlain(line)}`)
        .join("\n");
}

// ---------------------------------------------------------------------------
// Clipboard accumulation buffer
// ---------------------------------------------------------------------------
let _clipBuffer = "";
let _clipLineCount = 0;

function _clipIndicatorEl() { return document.getElementById("clip-indicator"); }

function _updateClipIndicator() {
    const el = _clipIndicatorEl();
    if (!el) return;
    el.style.display = _clipLineCount > 0 ? "" : "none";
    const span = el.querySelector(".clip-count");
    if (span) span.textContent = `\uD83D\uDCCB ${_clipLineCount} lines`;
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
// Scope-aware action handlers
// ---------------------------------------------------------------------------

function _addToBuffer(paneId, text, count) {
    const isFirst = _clipBuffer === "";
    _clipBuffer += (isFirst ? "" : "\n\n\n\n") + text;
    _clipLineCount += count;
    _updateClipIndicator();
    _renderClipPeek();
}

function _flashButton(id, text, restoreMs = 900) {
    const btn = document.getElementById(id);
    if (!btn) return;
    const prev = btn.textContent;
    btn.textContent = text;
    setTimeout(() => { btn.textContent = prev; _syncSelectionActions(btn.id.replace(/^.+-(?=\w+$)/, "")); }, restoreMs);
}

// ---------------------------------------------------------------------------
// Copy — scope-aware
// ---------------------------------------------------------------------------
function _copy(paneId) {
    if (state.selectionScope !== "exact") return _copyContext(paneId);
    return _copyExact(paneId);
}

function _copyExact(paneId) {
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
        setTimeout(() => { btn.textContent = prev; _syncSelectionActions(paneId); }, 900);
    }).catch(() => {});
}

function _copyContext(paneId) {
    const entries = _collectRangeEntries(paneId);
    const text = _formatRangeRaw(entries);
    if (!text) return;
    _copyText(text).then(() => {
        const btn = document.getElementById("copy-" + paneId);
        if (!btn) return;
        const prev = btn.textContent;
        btn.textContent = "Copied";
        setTimeout(() => { btn.textContent = prev; _syncSelectionActions(paneId); }, 900);
    }).catch(() => {});
}

// ---------------------------------------------------------------------------
// Download raw — scope-aware
// ---------------------------------------------------------------------------
function _downloadRaw(paneId) {
    if (state.selectionScope !== "exact") return _downloadRawContext(paneId);
    return _downloadRawExact(paneId);
}

function _downloadRawExact(paneId) {
    const sel = state.selected[paneId];
    if (!sel.size) return;
    const indices = Array.from(sel).sort((a, b) => a - b);
    const text = _formatSelectionBlock(paneId, indices);
    if (!text) return;
    const label = _rangeBoundsLabelExact(paneId);
    _downloadText(`embed-log-exact-${_safeFilePart(label)}.log`, text + "\n", "text/plain");
}

function _downloadRawContext(paneId) {
    const entries = _collectRangeEntries(paneId);
    const text = _formatRangeRaw(entries);
    if (!text) return;
    const label = _rangeBoundsLabel(entries);
    _downloadText(`embed-log-snippet-${_safeFilePart(label)}.log`, text + "\n", "text/plain");
}

// ---------------------------------------------------------------------------
// Export HTML — scope-aware (secondary action)
// ---------------------------------------------------------------------------
function _exportHtml(paneId) {
    const sel = state.selected[paneId];
    if (!sel.size) return;
    const btn = document.getElementById("export-html-" + paneId);

    if (state.selectionScope !== "exact") {
        const entries = _collectRangeEntries(paneId);
        if (!entries.length) return;
        const label = _rangeBoundsLabel(entries);
        exportHtmlSnapshot({
            button: btn,
            logData: _buildRangeLogData(entries),
            filenamePrefix: `embed-log-snippet-${_safeFilePart(label)}`,
            title: `snippet ${label}`,
            activeTab: state.activeTab,
        });
    } else {
        const indices = Array.from(sel).sort((a, b) => a - b);
        const entries = indices.map(i => ({
            paneId,
            idx: i,
            line: state.rawLines[paneId][i],
        }));
        const label = _rangeBoundsLabelExact(paneId);
        exportHtmlSnapshot({
            button: btn,
            logData: _buildRangeLogData(entries),
            filenamePrefix: `embed-log-exact-${_safeFilePart(label)}`,
            title: `exact ${label}`,
            activeTab: state.activeTab,
        });
    }
}

// ---------------------------------------------------------------------------
// Add to clipboard — scope-aware (secondary action)
// ---------------------------------------------------------------------------
function _addSelectedToBuffer(paneId) {
    const sel = state.selected[paneId];
    if (!sel.size) return;

    let text, count;
    if (state.selectionScope !== "exact") {
        const entries = _collectRangeEntries(paneId);
        text = _formatRangeRaw(entries);
        count = entries.length;
    } else {
        const indices = Array.from(sel).sort((a, b) => a - b);
        text = _formatSelectionBlock(paneId, indices);
        count = sel.size;
    }
    if (!text) return;

    _addToBuffer(paneId, text, count);

    const btn = document.getElementById("clip-add-" + paneId);
    if (!btn) return;
    const prev = btn.textContent;
    btn.textContent = `Added (${_clipLineCount})`;
    setTimeout(() => { btn.textContent = prev; _syncSelectionActions(paneId); }, 900);
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

    // Close More dropdowns on click outside
    if (!inSelectionUi) _closeAllMore();

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
        if (pane) { _copyExact(pane); e.preventDefault(); }
        return;
    }
    if (e.key === "Escape") {
        if (_isClipPeekOpen()) {
            _closeClipPeek();
            return;
        }
        _closeAllMore();
        _clearAllSelections();
    }
});
