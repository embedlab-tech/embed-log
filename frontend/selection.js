import { state, TABS, PANES, paneLabel, unwrapPaneLabel } from './state.js';
import { onLineClick, hidePluginOverlays } from './lines.js';
import { exportHtmlSnapshot } from './export.js';
import { can } from './profile.js';
import { switchTab } from './tabs.js';
import { _escHtml } from './renderPane.js';
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

//
// Keyboard Ctrl+C / Cmd+C always copies exact selection (predictable).
// ---------------------------------------------------------------------------



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
    moreDropdown.appendChild(htmlBtn);

    const rawBtn = document.createElement("button");
    rawBtn.className = "copy-btn";
    rawBtn.id = "download-raw-" + id;
    rawBtn.textContent = "Download raw";
    rawBtn.title = "Download selected lines as raw .log file";
    rawBtn.addEventListener("click", e => { e.stopPropagation(); _downloadRaw(id); });
    moreDropdown.appendChild(rawBtn);
    actionRow.appendChild(copyBtn);
    // Marker toggle (runtime only) — in the main action row
    if (can('markers')) {
        const markerBtn = document.createElement("button");
        markerBtn.className = "copy-btn";
        markerBtn.id = "marker-toggle-" + id;
        markerBtn.textContent = "Add Note";
        markerBtn.title = "Add a note to the selected/sync-highlighted line(s)";
        markerBtn.addEventListener("click", e => { e.stopPropagation(); _toggleMarker(id); });
        actionRow.appendChild(markerBtn);
    }
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
            _syncSelectionActions(paneId);
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
    if (!wrap || !copyBtn) return;

    const selectedCount = state.selected[paneId].size;
    const visible = selectedCount > 0;
    const displayCount = state.selectionScope === "exact"
        ? selectedCount
        : _countRangeEntries(paneId);
    wrap.classList.toggle("visible", visible);
    wrap.querySelectorAll(".copy-btn, .scope-btn, .more-toggle").forEach(el =>
        el.classList.toggle("visible", visible)
    );

    if (visible) {
        copyBtn.textContent = `Copy (${displayCount})`;
    }
    // Scope-gate secondary actions: marker only in Exact, Export HTML only outside Exact
    const htmlBtn = document.getElementById("export-html-" + paneId);
    if (htmlBtn) {
        htmlBtn.style.display = state.selectionScope === "exact" ? "none" : "";
    }
    const markerBtn = document.getElementById("marker-toggle-" + paneId);
    if (markerBtn) {
        markerBtn.style.display = state.selectionScope === "exact" ? "" : "none";
    }
}
function _flatMarkerList() {
    const all = [];
    Object.keys(state.markers).forEach(paneId => {
        (state.markers[paneId] || []).forEach(m => {
            all.push({ paneId, ...m });
        });
    });
    all.sort((a, b) => (a.numTs ?? 0) - (b.numTs ?? 0));
    return all;
}

function _markerLineIdx(paneId) {
    const div = state.highlighted[paneId];
    if (div) return parseInt(div.dataset.idx, 10);
    const sel = state.selected[paneId];
    if (sel?.size === 1) return [...sel][0];
    return -1;
}

function _toggleMarker(paneId) {
    // Get indices to mark: selected lines, or fall back to highlighted line
    const sel = state.selected[paneId];
    const indices = sel?.size > 0
        ? Array.from(sel)
        : (() => {
            const idx = _markerLineIdx(paneId);
            return idx >= 0 ? [idx] : [];
          })();
    if (!indices.length) return;

    const markers = state.markers[paneId] = state.markers[paneId] || [];
    const lines = state.rawLines[paneId] || [];

    // If any indices are not yet marked, show the input to add/overwrite
    _showMarkerInput(paneId, indices, lines);
    // We don't handle "remove existing" here anymore — the save/commit
    // in _showMarkerInput replaces any overlapping markers.
}


function _showMarkerInput(paneId, indices, lines) {
    const body = document.querySelector(`#pane-${paneId} .pane-body`);
    if (!body) return;

    // Remove any existing marker input overlay
    document.querySelectorAll(".marker-input-overlay").forEach(el => el.remove());

    const overlay = document.createElement("div");
    overlay.className = "marker-input-overlay";
    overlay.innerHTML =
        '<span class="marker-input-label">Marker:</span>' +
        '<input class="marker-input" type="text" placeholder="Describe this marker…" autofocus>' +
        '<button class="marker-input-save">Save</button>' +
        '<button class="marker-input-cancel">✕</button>';

    const input = overlay.querySelector(".marker-input");
    const saveBtn = overlay.querySelector(".marker-input-save");
    const cancelBtn = overlay.querySelector(".marker-input-cancel");

    const candidates = indices.map(i => ({ lineIdx: i, numTs: lines[i]?.numTs ?? 0 }));
    // Single marker for the entire range (first → last)
    const rangeStart = Math.min(...indices);
    const rangeEnd = Math.max(...indices);
    let inputActive = true;

    function commit() {
        if (!inputActive) return;
        inputActive = false;
        const desc = (input.value || "").trim() || "(no description)";
        const markers = state.markers[paneId] = state.markers[paneId] || [];
        const keep = markers.filter(m => m.lineIdx < rangeStart || m.lineIdx > rangeEnd);
        state.markers[paneId] = keep;
        keep.push({
            lineIdx: rangeStart,
            endIdx: rangeEnd,
            numTs: lines[rangeStart]?.numTs ?? 0,
            description: desc,
            createdAt: new Date().toISOString(),
        });
        overlay.remove();
        applyMarkers();
        _updateMarkerNav();
        wsSend({ cmd: "save_markers", markers: _flatMarkerList() });
    }

    function cancel() {
        if (!inputActive) return;
        inputActive = false;
        overlay.remove();
    }

    saveBtn.addEventListener("click", e => { e.stopPropagation(); commit(); });
    cancelBtn.addEventListener("click", e => { e.stopPropagation(); cancel(); });
    input.addEventListener("keydown", e => {
        if (e.key === "Enter") commit();
        if (e.key === "Escape") cancel();
    });

    // Show which lines are being marked
    const label = overlay.querySelector(".marker-input-label");
    const count = rangeEnd - rangeStart + 1;
    if (count > 1) label.textContent = `Marker (${count} lines):`;

    // Position overlay near the copy-actions
    const actions = body.querySelector(".copy-actions");
    if (actions) {
        const rect = actions.getBoundingClientRect();
        overlay.style.position = "fixed";
        overlay.style.left = rect.left + "px";
        overlay.style.top = (rect.bottom + 4) + "px";
        // Clamp so overlay doesn't extend past right edge
        requestAnimationFrame(() => {
            const ow = overlay.offsetWidth;
            if (rect.left + ow > window.innerWidth) {
                overlay.style.left = Math.max(8, window.innerWidth - ow - 8) + "px";
                }
            });
        }
    body.appendChild(overlay);
    setTimeout(() => input.focus(), 50);
}


export function applyMarkers() {
    const byPane = state.markers;
    PANES.forEach(paneId => {
        const logEl = document.getElementById("log-" + paneId);
        if (!logEl) return;
        const paneMarkers = byPane[paneId] || [];
        // Build a lookup: lineIdx → description (mark every line in a range)
        const byLine = {};
        paneMarkers.forEach(m => {
            const end = m.endIdx ?? m.lineIdx;
            for (let i = m.lineIdx; i <= end; i++) {
                if (byLine[i] === undefined) byLine[i] = m.description;
            }
        });
        Array.from(logEl.children).forEach((div, i) => {
            const hasMarker = byLine[i] !== undefined;
            div.classList.toggle("has-marker", hasMarker);
            if (hasMarker) {
                div.dataset.markerTooltip = byLine[i];
            } else {
                delete div.dataset.markerTooltip;
            }
        });
    });
}
window.applyMarkers = applyMarkers;

// ── Marker tooltip ──
const _tooltipEl = document.createElement("div");
_tooltipEl.id = "marker-tooltip";
document.body.appendChild(_tooltipEl);

document.addEventListener("mouseover", e => {
    const line = e.target.closest(".log-line.has-marker");
    if (!line) { _tooltipEl.classList.remove("visible"); return; }
    const desc = line.dataset.markerTooltip || "";
    if (!desc) return;
    const rect = line.getBoundingClientRect();
    _tooltipEl.innerHTML = '<span class="mt-label">Marker</span>' + _escHtml(desc);
    // Position above the line so it doesn't cover log text below
    _tooltipEl.style.left = Math.max(4, rect.left) + "px";
    _tooltipEl.style.bottom = (window.innerHeight - rect.top + 4) + "px";
    _tooltipEl.classList.add("visible");
});


function _updateMarkerNav() {
    const flat = _flatMarkerList();
    const navEl = document.getElementById("marker-nav");
    if (!navEl) return;
    if (flat.length === 0) {
        navEl.style.display = "none";
        state.markerNavIdx = -1;
        return;
    }
    navEl.style.display = "";
    const total = document.getElementById("marker-nav-total");
    if (total) total.textContent = String(flat.length);
    // Clamp nav index
    if (state.markerNavIdx < 0 || state.markerNavIdx >= flat.length) {
        state.markerNavIdx = 0;
    }
    _updateMarkerNavBtn();
}

function _updateMarkerNavBtn() {
    const el = document.getElementById("marker-nav-idx");
    if (el) el.textContent = String(state.markerNavIdx + 1);
}

document.getElementById("marker-nav-prev")?.addEventListener("click", () => {
    const flat = _flatMarkerList();
    if (!flat.length) return;
    state.markerNavIdx = (state.markerNavIdx - 1 + flat.length) % flat.length;
    _jumpMarker(flat[state.markerNavIdx]);
});

document.getElementById("marker-nav-next")?.addEventListener("click", () => {
    const flat = _flatMarkerList();
    if (!flat.length) return;
    state.markerNavIdx = (state.markerNavIdx + 1) % flat.length;
    _jumpMarker(flat[state.markerNavIdx]);
});

function _jumpMarker(m) {
    const paneId = m.paneId;
    // Switch to the tab containing this pane
    const tabIdx = TABS.findIndex(t => t.panes.includes(paneId));
    if (tabIdx >= 0) switchTab(tabIdx);
    const div = document.querySelector(`#log-${paneId} [data-idx="${m.lineIdx}"]`);
    if (!div) return;
    const logEl = document.getElementById("log-" + paneId);
    if (!logEl) return;
    logEl.scrollTop = div.offsetTop - Math.floor(logEl.clientHeight / 3);
    state.atBottom[paneId] = false;
    onLineClick(paneId, m.numTs, div);
    _updateMarkerNavBtn();
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
function _lineRenderedPlain(line) {
    const inlineText = typeof line?.pluginInlineText === "string"
        ? line.pluginInlineText.replace(/\s+/g, " ").trim()
        : "";
    return inlineText || _linePlain(line);
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
const RANGE_MARGIN_MS = 10;

function _selectionRange(paneId) {
    const sel = state.selected[paneId];
    if (!sel?.size) return null;
    const lines = state.rawLines[paneId] || [];
    const nums = Array.from(sel)
        .map(i => lines[i]?.numTs)
        .filter(n => Number.isFinite(n) && n >= 0);
    if (!nums.length) return null;
    const from = Math.min(...nums);
    const to = Math.max(...nums);
    if (state.selectionScope === 'exact') return { from, to };
    // In context or context-selected mode, pad range slightly to capture
    // sibling pane data whose timestamps may be offset by a few ms due to
    // source-level message delivery timing differences.
    return { from: from - RANGE_MARGIN_MS, to: to + RANGE_MARGIN_MS };
}

function _rangeTargetPanes() {
    return state.selectionScope === 'context-selected'
        ? PANES.filter(id => state.contextPanes[id])
        : PANES;
}

function _countRangeEntries(paneId) {
    const range = _selectionRange(paneId);
    if (!range) return 0;
    let count = 0;
    _rangeTargetPanes().forEach(id => {
        (state.rawLines[id] || []).forEach(line => {
            const n = line?.numTs;
            if (Number.isFinite(n) && n >= range.from && n <= range.to) count++;
        });
    });
    return count;
}

function _collectRangeEntries(paneId) {
    const range = _selectionRange(paneId);
    if (!range) return [];
    const entries = [];
    _rangeTargetPanes().forEach(id => {
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

function _snippetMessageText(entry, useRendered = false) {
    let text = (useRendered ? _lineRenderedPlain(entry.line) : _linePlain(entry.line)).trim();
    const sourcePrefix = new RegExp(`^\\[${_escapeRegExp(entry.paneId)}\\]\\s*`);
    for (let i = 0; i < 4; i++) {
        const before = text;
        text = text
            .replace(/^\[\d{4}-\d{2}-\d{2}T[^\]]+\]\s*/, "")
            .replace(/^\[T\+\d+:\d{2}:\d{2}\.\d{3}\]\s*/, "")
            .replace(/^T\+\d+:\d{2}:\d{2}\.\d{3}\s*/, "")
            .replace(sourcePrefix, "")
            .trim();
        if (text === before) break;
    }
    return text;
}

function _applyMarkerAnnotations(paneId, items) {
    const paneMarkers = state.markers[paneId] || [];
    if (!paneMarkers.length) return items.map(i => i.text).join("\n");
    const out = [];
    for (const item of items) {
        for (const m of paneMarkers) {
            if (m.lineIdx === item.idx) {
                out.push(`USER_MARKER_START = ${m.description}`);
            }
        }
        out.push(item.text);
        for (const m of paneMarkers) {
            const end = m.endIdx ?? m.lineIdx;
            if (end === item.idx) {
                out.push("USER_MARKER_END");
            }
        }
    }
    return out.join("\n");
}


function _formatRangeRaw(entries, useRendered = false) {
    const parts = [];
    let currentPane = null;
    let paneItems = [];
    function flushPane() {
        if (paneItems.length) {
            parts.push(_applyMarkerAnnotations(currentPane, paneItems));
            paneItems = [];
        }
    }
    entries.forEach(e => {
        if (e.paneId !== currentPane) flushPane();
        currentPane = e.paneId;
        paneItems.push({
            idx: e.idx,
            text: `[${e.line.ts}] [${e.paneId}] ${_snippetMessageText(e, useRendered)}`,
        });
    });
    flushPane();
    return parts.join("\n");
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
            absTs: e.line.absTs ?? null,
            absNum: Number.isFinite(e.line.absNum) ? e.line.absNum : null,
            relTs: e.line.relTs ?? null,
            relNum: Number.isFinite(e.line.relNum) ? e.line.relNum : null,
        });
    });
    return logData;
}

function _formatSelectionBlock(paneId, indices, useRendered = false) {
    const lines = state.rawLines[paneId];
    const items = indices
        .map(idx => lines[idx]
            ? { idx, text: `${lines[idx].ts}  [${paneId}] ${(useRendered ? _lineRenderedPlain(lines[idx]) : _linePlain(lines[idx]))}` }
            : null)
        .filter(Boolean);
    return _applyMarkerAnnotations(paneId, items);
}























// ---------------------------------------------------------------------------
// Scope-aware action handlers
// ---------------------------------------------------------------------------



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
    const text = _formatSelectionBlock(paneId, indices, true);
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
    const text = _formatRangeRaw(entries, true);
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
    _saveSnippetToServer(text, [paneId], 'exact', label);
}

function _downloadRawContext(paneId) {
    const entries = _collectRangeEntries(paneId);
    const text = _formatRangeRaw(entries);
    if (!text) return;
    const label = _rangeBoundsLabel(entries);
    _downloadText(`embed-log-snippet-${_safeFilePart(label)}.log`, text + "\n", "text/plain");
    const panes = [...new Set(entries.map(e => e.paneId))];
    _saveSnippetToServer(text, panes, state.selectionScope, label);
}

function _saveSnippetToServer(text, panes, scope, label) {
    if (!window.__embedLogProfile?.capabilities?.sessionApi) return;
    fetch('/api/session/snippet', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ text, panes, scope, label }),
    }).catch(() => {});
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


// ---------------------------------------------------------------------------
// Pointer drag — selection
// ---------------------------------------------------------------------------
let _drag = null;          // { paneId, startIdx, startY, lineEl, active }
let _suppressClick = false;
let _rangeAnchor = null;   // { paneId, idx } set by a normal line click for Shift+Click range selection
let _altSelection = false; // true during an Alt-native-text-select drag


document.addEventListener("pointerdown", e => {
    if (e.button !== 0) return;
    const line = e.target.closest(".log-line");
    if (!line) return;
    const logArea = line.closest(".log-area");
    if (!logArea) return;
    if (e.altKey) { _altSelection = true; return; }
    hidePluginOverlays();


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

document.addEventListener("pointerup", () => {
    if (_altSelection) {
        _altSelection = false;
        const text = window.getSelection()?.toString();
        if (text) navigator.clipboard.writeText(text).catch(() => {});
    }
    _drag = null;
});

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
    if (clickedLine && clickedLogArea && !e.altKey) {
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

            // Single-click: select this line (replaces any previous selection)
            const add = e.ctrlKey || e.metaKey;
            if (!add) _clearOtherSelections(paneId);
            const sel = new Set(add ? (state.selected[paneId] || []) : []);
            if (add && sel.has(idx)) sel.delete(idx);
            else sel.add(idx);
            state.selected[paneId] = sel;
            _applySelection(paneId);
            _closeAllMore();
            return;
        }
    }

    const inSelectionUi = e.target.closest(".copy-actions");

    // Close More dropdowns on click outside
    if (!inSelectionUi) _closeAllMore();

    if (!PANES.some(id => state.selected[id]?.size > 0)) return;
    if (inSelectionUi) return;
    _clearAllSelections();
}, true);

// ---------------------------------------------------------------------------
// Keyboard
// ---------------------------------------------------------------------------
document.addEventListener("keydown", e => {
    if ((e.ctrlKey || e.metaKey) && e.key === "c") {
        const pane = PANES.find(id => state.selected[id].size > 0);
        if (pane) { _copyExact(pane); e.preventDefault(); return; }
        // No explicit selection — fall back to the sync-highlighted line.
        const hlPane = PANES.find(id => state.highlighted[id]);
        if (hlPane) {
            const div = state.highlighted[hlPane];
            const idx = parseInt(div?.dataset.idx, 10);
            if (Number.isFinite(idx)) {
                const text = _formatSelectionBlock(hlPane, [idx], true);
                if (text) { _copyText(text); e.preventDefault(); return; }
            }
        }
        return;
    }
    if (e.key === "Escape") {
        _closeAllMore();
        _clearAllSelections();
    }
});
// ── Alt key: hold to enable native text selection on log lines ──
document.addEventListener("keydown", e => {
    if (e.key === "Alt" && !e.ctrlKey && !e.metaKey) {
        document.body.classList.add("alt-held");
    }
});
document.addEventListener("keyup", e => {
    if (e.key === "Alt") document.body.classList.remove("alt-held");
});
window.addEventListener("blur", () => document.body.classList.remove("alt-held"));
// Update marker nav when markers arrive from server
window.__embedLogOnMarkers = () => {
    _updateMarkerNav();
};
