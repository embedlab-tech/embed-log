import {
    state, TABS, PANES, buildTimestampInfo, applyTimestampModeToLine,
    lineHasTimestampMode,
} from './state.js';
import { parseAnsi } from './ansi.js';

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

export function appendLine(paneId, ts, rawText, isTx, meta = null) {
    appendLineBatch([{ paneId, ts, rawText, isTx, meta }]);
}

export function appendLineBatch(entries) {
    const fragments = new Map();
    const touched = new Set();

    entries.forEach(({ paneId, ts, rawText, isTx, meta = null }) => {
        if (!state.rawLines[paneId]) return;

        const html = parseAnsi(rawText);
        const line = {
            ...buildTimestampInfo(ts, typeof meta === "object" && meta !== null
                ? meta
                : (Number.isFinite(meta) ? { numTs: meta } : {})),
            html,
            rawText,
            isTx,
        };
        state.rawLines[paneId].push(line);

        const logEl = document.getElementById("log-" + paneId);
        if (!logEl) return;

        const idx = state.rawLines[paneId].length - 1;
        const div = document.createElement("div");
        div.dataset.ts = line.ts;
        div.dataset.idx = idx;
        div.className = _lineClass(line, idx, paneId);

        const rx = state.filters[paneId];
        if (!matchesFilter(line, rx)) {
            div.style.display = "none";
        } else {
            div.innerHTML = buildLineHtml(line, state.showTs, rx);
        }

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
        div.className = _lineClass(line, i, paneId);
        if (!matchesFilter(line, rx)) {
            div.style.display = "none";
        } else {
            div.style.display = "";
            div.innerHTML = buildLineHtml(line, state.showTs, rx);
        }
    }
    if (state.atBottom[paneId]) logEl.scrollTop = logEl.scrollHeight;
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
        div.className   = _lineClass(line, idx, paneId);
        if (matchesFilter(line, rx)) {
            div.innerHTML = buildLineHtml(line, state.showTs, rx);
        } else {
            div.style.display = "none";
        }

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
