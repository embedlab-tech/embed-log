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

export function renderPaneShell(paneId, label, { showTx = false } = {}) {
    const safeLabel = _escHtml(label);
    const txInputAttrs = ' placeholder="Serial TX — press Enter to send"' + (showTx ? '' : ' style="display:none"');
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
        '',
        '                <button class="pane-wrap-btn" title="Toggle word wrap in this pane">Wrap</button>',
        '',
        '            </div>',
        '            <div class="filter-bar">',
        '                <input class="filter-input" data-pane="' + paneId + '" placeholder="Filter (regex)…">',
        '            </div>',
        '            <div class="pane-body">',
        '                <div class="log-area" id="log-' + paneId + '"></div>',
        '                <button class="jump-btn" id="jump-' + paneId + '">jump to bottom</button>',
        '            </div>',
        txRow,
        '        </div>',
    ].join('\n');
}
