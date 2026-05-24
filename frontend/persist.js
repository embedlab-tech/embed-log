import { state, TABS, PANES } from './state.js';
import { appendLine, updateJumpBtn } from './lines.js';
import { createTabWithPanes } from './tabcreate.js';
import { switchTab } from './tabs.js';

const STORAGE_KEY_PREFIX = 'embed-log:session:';
const STORAGE_KEY_SUFFIX = ':v1';
const LAST_SESSION_KEY = 'embed-log:last-session-id';
const MAX_LINES_PER_PANE = 1500;
const SAVE_DEBOUNCE_MS = 500;

let _storageKey = `${STORAGE_KEY_PREFIX}default${STORAGE_KEY_SUFFIX}`;
let _hasSessionInfo = false;
let _currentSessionId = null;
let _restoreDone = false;
let _restoring = false;
let _saveTimer = null;

function _safeJsonParse(text) {
    try { return JSON.parse(text); } catch { return null; }
}

function _snapshot() {
    const lines = {};
    PANES.forEach(paneId => {
        const src = state.rawLines[paneId] || [];
        const sliced = src.slice(-MAX_LINES_PER_PANE).map(line => ({
            ts: line.ts,
            text: line.rawText ?? '',
            isTx: !!line.isTx,
        }));
        if (sliced.length) lines[paneId] = sliced;
    });

    return {
        tabs: TABS.map(t => ({ label: t.label, panes: [...t.panes] })),
        activeTab: state.activeTab,
        fontSize: state.fontSize,
        lines,
        savedAt: Date.now(),
    };
}

function _saveNow() {
    if (_restoring) return;
    try {
        localStorage.setItem(_storageKey, JSON.stringify(_snapshot()));
    } catch {
        // Ignore quota / private mode errors.
    }
}

function _restoreIfPossible() {
    if (_restoreDone) return;

    const raw = localStorage.getItem(_storageKey);
    if (!raw) {
        _restoreDone = true;
        return;
    }
    const snap = _safeJsonParse(raw);
    if (!snap) {
        _restoreDone = true;
        return;
    }

    _restoring = true;

    // If there is no live layout yet, recreate last known layout first.
    if (TABS.length === 0 && Array.isArray(snap.tabs) && snap.tabs.length) {
        snap.tabs.forEach(tab => {
            if (!tab || !Array.isArray(tab.panes) || !tab.panes.length) return;
            createTabWithPanes(tab.label || 'Tab', tab.panes, { switchTo: false });
        });
        if (TABS.length) {
            const idx = Math.max(0, Math.min(Number(snap.activeTab) || 0, TABS.length - 1));
            switchTab(idx);
        }
    }

    // Refill panes that are currently empty.
    const byPane = snap.lines && typeof snap.lines === 'object' ? snap.lines : {};
    Object.entries(byPane).forEach(([paneId, entries]) => {
        if (!PANES.includes(paneId)) return;
        if (!Array.isArray(entries) || entries.length === 0) return;
        if ((state.rawLines[paneId] || []).length > 0) return; // avoid duplicates

        state.atBottom[paneId] = false;
        entries.forEach(e => {
            appendLine(paneId, e.ts || '', e.text || '', !!e.isTx);
        });
        const logEl = document.getElementById('log-' + paneId);
        if (logEl) {
            logEl.scrollTop = logEl.scrollHeight;
            state.atBottom[paneId] = true;
            updateJumpBtn(paneId);
        }
    });

    // Restore font-size from cache
    if (typeof snap.fontSize === 'number' && snap.fontSize > 0) {
        state.fontSize = snap.fontSize;
    }
    
    _restoring = false;
    _restoreDone = true;
}

window.__embedLogSchedulePersist = function __embedLogSchedulePersist() {
    clearTimeout(_saveTimer);
    _saveTimer = setTimeout(_saveNow, SAVE_DEBOUNCE_MS);
};

window.__embedLogSetSession = function __embedLogSetSession(session) {
    _hasSessionInfo = true;
    const sessionId = session && session.id ? String(session.id) : 'default';

    // Persisted comparison across page reloads/browser reopen.
    let lastSessionId = null;
    try { lastSessionId = localStorage.getItem(LAST_SESSION_KEY); } catch {}

    if ((lastSessionId && lastSessionId !== sessionId) || (_currentSessionId && _currentSessionId !== sessionId)) {
        // Policy: new backend session means fresh UI cache.
        try {
            const keys = [];
            for (let i = 0; i < localStorage.length; i++) {
                const key = localStorage.key(i);
                if (key && key.startsWith(STORAGE_KEY_PREFIX)) keys.push(key);
            }
            keys.forEach(key => localStorage.removeItem(key));
            localStorage.removeItem('embed-log:session:v1'); // legacy key
        } catch {}
    }

    _currentSessionId = sessionId;
    _storageKey = `${STORAGE_KEY_PREFIX}${sessionId}${STORAGE_KEY_SUFFIX}`;
    _restoreDone = false;

    try { localStorage.setItem(LAST_SESSION_KEY, sessionId); } catch {}
};

window.__embedLogAfterConfig = function __embedLogAfterConfig() {
    _restoreIfPossible();
};

window.__embedLogClearCache = function __embedLogClearCache() {
    clearTimeout(_saveTimer);
    try {
        const keys = [];
        for (let i = 0; i < localStorage.length; i++) {
            const key = localStorage.key(i);
            if (key && key.startsWith(STORAGE_KEY_PREFIX)) keys.push(key);
        }
        keys.forEach(key => localStorage.removeItem(key));
        localStorage.removeItem('embed-log:session:v1'); // legacy key
        localStorage.removeItem(LAST_SESSION_KEY);
    } catch {}
};

// Note: restore is intentionally session-bound and starts after WS config.

window.addEventListener('beforeunload', _saveNow);
