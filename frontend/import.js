"use strict";

// ---------------------------------------------------------------------------
// File import — load .log files into any pane
//
// • "Import" button in each pane header (opens a file picker)
// • Drag-and-drop a .log file onto any pane body
//
// Expected log format (same as what the server writes):
//   [2026-03-25T11:50:09.900+01:00] message text
//
// Continuation lines (no leading timestamp) are appended to the preceding
// timestamped line so multi-line stack traces stay together.
//
// All lines are bulk-inserted via DocumentFragment — one DOM write per file.
// ---------------------------------------------------------------------------

// Timestamp parsing is provided by tsparse.js (loaded before this file).

function _loadTextIntoPane(paneId, text) {
    clearPane(paneId);
    const logEl = document.getElementById("log-" + paneId);
    const rx    = state.filters[paneId];
    const frag  = document.createDocumentFragment();

    let pendingTs   = null;
    let pendingData = null;
    let count = 0;

    function flush() {
        if (pendingTs === null) return;
        const html  = parseAnsi(pendingData);
        const numTs = tsToNum(pendingTs);
        const line  = { ts: pendingTs, numTs, html, rawText: pendingData, isTx: false };
        const idx   = state.rawLines[paneId].length;
        state.rawLines[paneId].push(line);

        const div = document.createElement("div");
        div.dataset.ts  = pendingTs;
        div.dataset.idx = idx;
        div.className   = _lineClass(line, idx, paneId);

        if (!matchesFilter(line, rx)) {
            div.style.display = "none";
        } else {
            div.innerHTML = buildLineHtml(line, state.showTs, rx);
        }
        div.addEventListener("click",     () => onLineClick(paneId, numTs, div));
        div.addEventListener("mousedown", e  => { if (e.button === 1) e.preventDefault(); });
        div.addEventListener("auxclick",  e  => { if (e.button === 1) onMiddleClick(paneId, numTs, div); });
        frag.appendChild(div);
        count++;
        pendingTs = null;
    }

    for (const raw of text.split("\n")) {
        const parsed = parseLogLine(raw);
        if (parsed) {
            flush();
            pendingTs   = parsed.ts;
            pendingData = parsed.data;
        } else if (pendingTs !== null && raw.trim()) {
            // Continuation line — append to current entry (stack traces, etc.)
            pendingData += " " + raw.trim();
        }
    }
    flush();

    const wasAtBottom = state.atBottom[paneId];
    logEl.appendChild(frag);
    if (wasAtBottom) {
        logEl.scrollTop = logEl.scrollHeight;
    }
    updateJumpBtn(paneId);
    return count;
}

function _importFile(paneId, file) {
    const reader = new FileReader();
    reader.onload = e => {
        const count = _loadTextIntoPane(paneId, e.target.result);
        const btn = document.getElementById("import-btn-" + paneId);
        if (!btn) return;
        const prev = btn.textContent;
        btn.textContent = `✓ ${count} lines`;
        setTimeout(() => { btn.textContent = prev; }, 2000);
    };
    reader.readAsText(file);
}

// ---------------------------------------------------------------------------
// Per-pane: import button + drag-and-drop
// ---------------------------------------------------------------------------
function _importSetupPane(id) {
    const header = document.querySelector(`#pane-${id} .pane-header`);
    if (!header) return;

    // Hidden file input
    const input    = document.createElement("input");
    input.type     = "file";
    input.accept   = ".log,.txt";
    input.style.display = "none";
    input.addEventListener("change", () => {
        if (input.files[0]) _importFile(id, input.files[0]);
        input.value = "";   // reset so the same file can be re-imported
    });

    // Import button
    const btn       = document.createElement("button");
    btn.id          = "import-btn-" + id;
    btn.className   = "import-btn";
    btn.title       = "Import a .log file into this pane";
    btn.textContent = "Import";
    btn.addEventListener("click", () => input.click());

    header.appendChild(input);
    header.appendChild(btn);

    // Drag-and-drop onto the pane body
    const body = document.querySelector(`#pane-${id} .pane-body`);
    if (!body) return;

    body.addEventListener("dragover", e => {
        e.preventDefault();
        body.classList.add("drag-over");
    });
    body.addEventListener("dragleave", e => {
        if (!body.contains(e.relatedTarget)) body.classList.remove("drag-over");
    });
    body.addEventListener("drop", e => {
        e.preventDefault();
        body.classList.remove("drag-over");
        const file = e.dataTransfer.files[0];
        if (file) _importFile(id, file);
    });
}
PANES.forEach(_importSetupPane);
