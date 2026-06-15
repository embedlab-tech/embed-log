// Shared pane shell renderer.
// Used by tabcreate.js (live mode) and export.js (runtime export).
// merge_logs.py produces the same structure in Python.

export function _escHtml(str) {
    return str
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;")
        .replace(/"/g, "&quot;");
}

export function renderPaneShell(paneId, label, { showTx = false, paneKind = "" } = {}) {
    const safeLabel = _escHtml(label);
    const filterPlaceholder = paneKind === "network_capture" ? "Filter (BPF)…" : "Filter (regex)…";
    const txRow = [
        '            <div class="input-row"' + (showTx ? '' : ' style="display:none"') + '>',
        '                <input class="serial-input" id="input-' + paneId + '" autocomplete="off"' + (showTx ? ' placeholder="Serial TX — press Enter to send"' : '') + '>',
        '                <button class="send-btn" data-pane="' + paneId + '">Send</button>',
        '            </div>',
    ].join('\n');
    return [
        '        <div class="pane" id="pane-' + paneId + '">',
        '            <div class="pane-header">',
        '                <span class="pane-name">' + safeLabel + '</span>',
        '                <span class="pane-stats" data-pane-stats="' + paneId + '"></span>',
        '',
        '                <button class="pane-wrap-btn" title="Toggle word wrap in this pane">Wrap</button>',
        '                <button class="pane-download-btn" title="Download raw .log for this pane">Download</button>',
        '            </div>',
        '            <div class="filter-bar">',
        '                <input class="filter-input" data-pane="' + paneId + '" placeholder="' + filterPlaceholder + '">',
        '            </div>',
        '            <div class="pane-body">',
'                <div class="log-area" id="log-' + paneId + '"><div class="log-spacer"><div class="log-window"></div></div></div>',
        '                <button class="jump-btn" id="jump-' + paneId + '">jump to bottom</button>',
        '            </div>',
        txRow,
        '        </div>',
    ].join('\n');
}
