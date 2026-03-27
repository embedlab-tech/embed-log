"use strict";

// ---------------------------------------------------------------------------
// Line selection + copy
//
// Plain click  → sync works exactly as before (no change).
// Click + drag → once the pointer moves >6 px vertically the drag activates:
//                • syncs the line you started dragging from
//                • highlights the range as you drag
//                • the click that would fire on pointerup is suppressed so
//                  onLineClick isn't called a second time
//
// Copy via the floating "Copy N lines" button, Ctrl/Cmd+C, or Escape to clear.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Inject clipboard indicator into toolbar (before ws-status)
// ---------------------------------------------------------------------------
(function () {
    const wsStatus = document.getElementById("ws-status");
    if (!wsStatus) return;

    const ind = document.createElement("div");
    ind.id            = "clip-indicator";
    ind.style.display = "none";

    const span = document.createElement("span");
    span.className = "clip-count";
    ind.appendChild(span);

    const sep = document.createElement("span");
    sep.className   = "clip-sep";
    sep.textContent = "·";
    ind.appendChild(sep);

    const clearBtn = document.createElement("button");
    clearBtn.className   = "clip-clear";
    clearBtn.textContent = "Clear";
    clearBtn.title       = "Clear clipboard buffer";
    clearBtn.addEventListener("click", _clearClipBuffer);
    ind.appendChild(clearBtn);

    wsStatus.before(ind);
})();

// ---------------------------------------------------------------------------
// Inject per-pane copy button
// ---------------------------------------------------------------------------
function _selectionSetupPane(id) {
    const body = document.querySelector(`#pane-${id} .pane-body`);
    if (!body) return;
    const btn = document.createElement("button");
    btn.className = "copy-btn";
    btn.id        = "copy-" + id;
    btn.addEventListener("click", e => { e.stopPropagation(); _copySelected(id); });
    body.appendChild(btn);
}
PANES.forEach(_selectionSetupPane);

// ---------------------------------------------------------------------------
// Patch clearPane (lines.js) so the copy button hides after a clear
// ---------------------------------------------------------------------------
const _clearPaneBase = clearPane;
clearPane = function (paneId) {
    _clearPaneBase(paneId);               // also resets state.selected[paneId]
    _applySelection(paneId);              // removes .selected classes + hides btn
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------
function _stripHtml(str) { return str.replace(/<[^>]+>/g, ""); }

function _syncCopyBtn(paneId) {
    const btn   = document.getElementById("copy-" + paneId);
    if (!btn) return;
    const count = state.selected[paneId].size;
    btn.classList.toggle("visible", count > 0);
    if (count > 0)
        btn.textContent = `Copy ${count} line${count === 1 ? "" : "s"}`;
}

function _applySelection(paneId) {
    const logEl = document.getElementById("log-" + paneId);
    const sel   = state.selected[paneId];
    Array.from(logEl.children).forEach((div, i) =>
        div.classList.toggle("selected", sel.has(i))
    );
    _syncCopyBtn(paneId);
}

function _clearOtherSelections(keepPane) {
    PANES.forEach(id => {
        if (id === keepPane || !state.selected[id].size) return;
        state.selected[id] = new Set();
        _applySelection(id);
    });
}

// ---------------------------------------------------------------------------
// Clipboard accumulation buffer
// Each copy appends to the buffer (3 blank lines between groups).
// The full buffer is written to the system clipboard every time.
// ---------------------------------------------------------------------------
let _clipBuffer    = "";
let _clipLineCount = 0;

function _clipIndicatorEl() { return document.getElementById("clip-indicator"); }

function _updateClipIndicator() {
    const el = _clipIndicatorEl();
    if (!el) return;
    el.style.display = _clipLineCount > 0 ? "" : "none";
    const span = el.querySelector(".clip-count");
    if (span) span.textContent = `📋 ${_clipLineCount} lines`;
}

function _clearClipBuffer() {
    _clipBuffer    = "";
    _clipLineCount = 0;
    _updateClipIndicator();
}

// ---------------------------------------------------------------------------
// Copy
// ---------------------------------------------------------------------------
function _copySelected(paneId) {
    const sel = state.selected[paneId];
    if (!sel.size) return;
    const lines = state.rawLines[paneId];
    const text  = Array.from(sel)
        .sort((a, b) => a - b)
        .map(i => lines[i] ? `${lines[i].ts}  ${_stripHtml(lines[i].html)}` : null)
        .filter(Boolean)
        .join("\n");
    if (!text) return;

    const isFirst  = _clipBuffer === "";
    _clipBuffer    += (isFirst ? "" : "\n\n\n\n") + text;
    _clipLineCount += sel.size;

    navigator.clipboard.writeText(_clipBuffer).then(() => {
        const btn = document.getElementById("copy-" + paneId);
        if (!btn) return;
        const prev = btn.textContent;
        btn.textContent = isFirst
            ? `Copied! (${_clipLineCount})`
            : `Appended! (${_clipLineCount} total)`;
        setTimeout(() => { btn.textContent = prev; }, 1400);
        _updateClipIndicator();
    }).catch(() => {});
}

// ---------------------------------------------------------------------------
// Pointer drag — selection
// ---------------------------------------------------------------------------
let _drag          = null;   // { paneId, startIdx, startY, lineEl, active }
let _suppressClick = false;  // true after a drag completes; cleared by click handler

document.addEventListener("pointerdown", e => {
    if (e.button !== 0) return;                  // left-click only
    const line    = e.target.closest(".log-line");
    if (!line) return;
    const logArea = line.closest(".log-area");
    if (!logArea) return;

    _drag = {
        paneId:   logArea.id.slice(4),          // "log-X" → "X"
        startIdx: parseInt(line.dataset.idx, 10),
        startY:   e.clientY,
        lineEl:   line,
        active:   false,
    };
    _suppressClick = false;
});

document.addEventListener("pointermove", e => {
    if (!_drag) return;
    if (Math.abs(e.clientY - _drag.startY) < 6) return;   // not enough movement yet

    if (!_drag.active) {
        // Drag just crossed the threshold — activate
        _drag.active   = true;
        _suppressClick = true;

        _clearOtherSelections(_drag.paneId);

        // Sync the starting line now (click will be suppressed, so do it here)
        const raw = state.rawLines[_drag.paneId][_drag.startIdx];
        if (raw) onLineClick(_drag.paneId, raw.numTs, _drag.lineEl);

        try { _drag.lineEl.setPointerCapture(e.pointerId); } catch (_) {}
    }

    // Extend selection to the line currently under the pointer
    const el = document.elementFromPoint(e.clientX, e.clientY);
    if (!el) return;
    const line    = el.closest(".log-line");
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

// After a drag the browser still fires a click event on the log-line.
// Intercept it in the capture phase so onLineClick isn't called a second time.
document.addEventListener("click", e => {
    if (!_suppressClick) return;
    if (!e.target.closest(".log-line")) return;
    _suppressClick = false;
    e.stopPropagation();
}, true);

// ---------------------------------------------------------------------------
// Keyboard
// ---------------------------------------------------------------------------
document.addEventListener("keydown", e => {
    if ((e.ctrlKey || e.metaKey) && e.key === "c") {
        const pane = PANES.find(id => state.selected[id].size > 0);
        if (pane) { _copySelected(pane); e.preventDefault(); }
        return;
    }
    if (e.key === "Escape") {
        PANES.forEach(id => {
            if (!state.selected[id].size) return;
            state.selected[id] = new Set();
            _applySelection(id);
        });
    }
});
