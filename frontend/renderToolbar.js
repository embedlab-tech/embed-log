// Toolbar action registry and renderer.
// Produces toolbar HTML based on the current profile.
// In live mode the toolbar is already in index.html — the IIFE below
// only fills the toolbar when it starts empty (static/exported mode).

const ACTIONS = [
    { id: 'btn-clear',         label: 'Clear',       title: 'Clear all panes',                                              cap: 'clearAll' },
    { id: 'btn-export',        label: 'Export HTML',  title: 'Export current session as a self-contained HTML file',          cap: 'exportHtml' },
    { id: 'btn-new-session',   label: 'New session',  title: 'Save current session and start a new one',                     cap: 'sessionApi' },
    { id: 'btn-unwrap',        label: 'Unwrap',       title: 'Unwrap multi-pane tabs into single-pane tabs',                  cap: 'unwrap' },
    { id: 'btn-timestamp-mode', label: 'Absolute',    title: 'Switch timestamps' },
    '__sep__',
    { id: 'btn-theme',         label: '\u{1F319}',    title: 'Toggle light / dark theme',                                    cap: 'themeToggle' },
];

/**
 * Render the toolbar HTML for a given profile.
 * Returns the full <div id="toolbar">…</div> string.
 */
export function renderToolbar(profile) {
    const caps = profile.capabilities;
    const parts = ['<div id="toolbar">'];
    parts.push('    <span class="app-name">embed-log</span>');

    let pendingSep = false;
    ACTIONS.forEach(item => {
        if (item === '__sep__') {
            pendingSep = true;
            return;
        }
        if (item.cap && caps[item.cap] === false) return;
        if (pendingSep) {
            parts.push('    <div class="sep"></div>');
            pendingSep = false;
        }
        parts.push(`    <button id="${item.id}" title="${_escAttr(item.title)}">${item.label}</button>`);
    });

    parts.push('    <div id="toolbar-stats" class="toolbar-stats"></div>');
    if (caps.wsStatus) {
        parts.push('    <div id="ws-status" class="disconnected">WS: disconnected</div>');
    }
    parts.push('    <div id="marker-nav" class="marker-nav" style="display:none">');
    parts.push('        <button id="marker-nav-prev" title="Previous marker">◀</button>');
    parts.push('        <span id="marker-nav-idx">1</span>/<span id="marker-nav-total">0</span>');
    parts.push('        <button id="marker-nav-next" title="Next marker">▶</button>');
    parts.push('    </div>');

    parts.push('</div>');
    return parts.join('\n');
}

function _escAttr(str) {
    return str.replace(/&/g, '&amp;').replace(/"/g, '&quot;');
}

// ── Auto-fill the toolbar when this module loads ──
// In live mode (toolbar already has buttons in index.html), skip.
// In static/exported mode (empty shell), render the full toolbar.
(function () {
    const toolbar = document.getElementById('toolbar');
    if (!toolbar) return;
    if (toolbar.querySelector('button')) return; // already populated
    toolbar.innerHTML = renderToolbar(window.__embedLogProfile);
})();
