import { state, TABS, PANES } from './state.js';
import { parseAnsi, tsToNum } from './ansi.js';

// ---------------------------------------------------------------------------
// Line rendering
// ---------------------------------------------------------------------------


// parseAnsi HTML-escapes < and >, so <wrn> becomes &lt;wrn&gt; in stored HTML.
// <inf> is intentionally excluded — it stays unstyled.
const _LINE_TAG_RE = /&lt;(wrn|warn|dbg|debug|err|error)&gt;/i;
function _lineTagClass(html) {
    const m = _LINE_TAG_RE.exec(html);
    if (!m) return "";
    switch (m[1].toLowerCase()) {
        case "wrn":  case "warn":  return " line-wrn";
        case "dbg":  case "debug": return " line-dbg";
        case "err":  case "error": return " line-err";
        default: return "";
    }
}

export function buildLineHtml(line, showTs, filterRx) {
    const tsClass = "ts" + (showTs ? "" : " hidden");
    let content = line.html;
    if (filterRx) {
        content = content.replace(filterRx, m => `<mark class="hl">${m}</mark>`);
    }
    return `<span class="${tsClass}">${line.ts}</span>${content}`;
}

// Build the full className string for a log-line div, preserving selection state.
export function _lineClass(line, idx, paneId) {
    return "log-line"
        + (line.isTx ? " tx-line" : "")
        + _lineTagClass(line.html)
        + (state.selected[paneId].has(idx) ? " selected" : "");
}

export function matchesFilter(line, rx) {
    if (!rx) return true;
    const plain = line.html.replace(/<[^>]+>/g, "") + " " + line.ts;
    return rx.test(plain);
}

export function appendLine(paneId, ts, rawText, isTx) {
    const html  = parseAnsi(rawText);
    const numTs = tsToNum(ts);
    const line  = { ts, numTs, html, rawText, isTx };
    state.rawLines[paneId].push(line);

    const logEl = document.getElementById("log-" + paneId);
    const idx   = state.rawLines[paneId].length - 1;
    const div   = document.createElement("div");
    div.dataset.ts  = ts;
    div.dataset.idx = idx;
    div.className   = _lineClass(line, idx, paneId);

    const rx = state.filters[paneId];
    if (!matchesFilter(line, rx)) {
        div.style.display = "none";
    } else {
        div.innerHTML = buildLineHtml(line, state.showTs, rx);
    }

    div.addEventListener("click",     () => onLineClick(paneId, numTs, div));
    div.addEventListener("mousedown", e  => { if (e.button === 1) e.preventDefault(); });
    div.addEventListener("auxclick",  e  => { if (e.button === 1) onMiddleClick(paneId, numTs, div); });
    logEl.appendChild(div);

    if (state.atBottom[paneId]) logEl.scrollTop = logEl.scrollHeight;
    updateJumpBtn(paneId);
    window.__embedLogSchedulePersist?.();
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
        if (!matchesFilter(line, rx)) {
            div.style.display = "none";
        } else {
            div.style.display = "";
            div.className = _lineClass(line, i, paneId);
            div.innerHTML = buildLineHtml(line, state.showTs, rx);
        }
    }
    if (state.atBottom[paneId]) logEl.scrollTop = logEl.scrollHeight;
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
    document.getElementById("jump-" + id).addEventListener("click", () => {
        state.syncTabSwitch = false;
        scrollPaneToBottom(id);
    });
    document.querySelector(`.pane-clear-btn[data-pane="${id}"]`)
        ?.addEventListener("click", () => {
            window.wsSend?.({ cmd: "clear_logs", scope: "pane", id });
            clearPane(id);
        });
}
PANES.forEach(_linesSetupPane);

// ---------------------------------------------------------------------------
// Clear
// ---------------------------------------------------------------------------

export function clearPane(paneId) {
    state.rawLines[paneId] = [];
    state.selected[paneId] = new Set();
    document.getElementById("log-" + paneId).innerHTML = "";
    highlightLine(paneId, null);
    state.atBottom[paneId] = true;
    updateJumpBtn(paneId);
    // Hide copy-selection actions if selection.js has added them
    document.getElementById("copy-" + paneId)?.classList.remove("visible");
    document.getElementById("copy-add-" + paneId)?.classList.remove("visible");
    document.getElementById("copy-actions-" + paneId)?.classList.remove("visible");
    window.__embedLogSchedulePersist?.();
}

document.getElementById("btn-clear").addEventListener("click", () => {
    window.wsSend?.({ cmd: "clear_logs", scope: "all" });
    PANES.forEach(clearPane);
});

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
    const activePanes = TABS[state.activeTab].panes;
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
