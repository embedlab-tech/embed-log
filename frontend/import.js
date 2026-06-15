import { state, PANES } from './state.js';
import { parseLogLine } from './tsparse.js';
import {
    clearPane, appendLineBatch, updateJumpBtn,
} from './lines.js';

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
// Parsed lines are appended through the shared pane model so large imports
// use the same bounded virtual DOM as live and exported logs.
// ---------------------------------------------------------------------------

function _loadTextIntoPane(paneId, text) {
    clearPane(paneId);

    let pendingTs = null;
    let pendingData = null;
    const batch = [];

    function flush() {
        if (pendingTs === null) return;
        batch.push({ paneId, ts: pendingTs, rawText: pendingData, isTx: false, meta: null });
        pendingTs = null;
    }

    for (const raw of text.split("\n")) {
        const parsed = parseLogLine(raw);
        if (parsed) {
            flush();
            pendingTs = parsed.ts;
            pendingData = parsed.data;
        } else if (pendingTs !== null && raw.trim()) {
            pendingData += " " + raw.trim();
        }
    }
    flush();

    appendLineBatch(batch);
    return batch.length;
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
export function _importSetupPane(id) {
    const header = document.querySelector(`#pane-${id} .pane-header`);
    if (!header) return;

    // Hidden file input
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".log,.txt";
    input.style.display = "none";
    input.addEventListener("change", () => {
        if (input.files[0]) _importFile(id, input.files[0]);
        input.value = "";   // reset so the same file can be re-imported
    });

    // Import button
    const btn = document.createElement("button");
    btn.id = "import-btn-" + id;
    btn.className = "import-btn";
    btn.title = "Import a .log file into this pane";
    btn.textContent = "Import";
    btn.addEventListener("click", () => input.click());

    header.appendChild(input);
    header.appendChild(btn);

    // Drag-and-drop onto the pane body
    const body = document.querySelector(`#pane-${id} .pane-body`);
    if (!body) return;
    body.addEventListener("dragover", e => {
        e.preventDefault();
        body.classList.add("dragover");
    });
    body.addEventListener("dragleave", () => body.classList.remove("dragover"));
    body.addEventListener("drop", e => {
        e.preventDefault();
        body.classList.remove("dragover");
        const file = e.dataTransfer?.files?.[0];
        if (file) _importFile(id, file);
    });
}

PANES.forEach(_importSetupPane);
