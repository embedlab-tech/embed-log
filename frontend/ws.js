import { state, TABS, PANES } from './state.js';
import { appendLine, clearPane } from './lines.js';
import { createTabWithPanes, createDynamicTab } from './tabcreate.js';
import { switchTab } from './tabs.js';

let ws = null;
let wsRetryDelay = 1000;
const WS_MAX_DELAY = 16000;
const wsStatus = document.getElementById("ws-status");
let currentSessionId = null;

function resetLayoutForNewSession() {
    const container = document.getElementById("container");
    if (container) container.innerHTML = "";

    TABS.length = 0;
    PANES.length = 0;

    state.activeTab = 0;
    state.syncTs = null;
    state.filters = {};
    state.rawLines = {};
    state.atBottom = {};
    state.highlighted = {};
    state.selected = {};
}

function wsSetStatus(cls, text) {
    wsStatus.className    = cls;
    wsStatus.textContent  = "WS: " + text;
}

export function wsSend(obj) {
    if (ws && ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify(obj));
    }
}

// Expose wsSend globally so ui.js can call it without a circular import.
// In static exports this is stubbed to a no-op by the bootstrap script.
window.wsSend = wsSend;

function clearAllPaneContents() {
    PANES.forEach(paneId => {
        const logEl = document.getElementById("log-" + paneId);
        if (!logEl) return;
        clearPane(paneId);
    });
}

function wsConnect() {
    wsSetStatus("connecting", "connecting…");
    ws = new WebSocket("ws://" + window.location.host + "/ws");

    ws.addEventListener("open", () => {
        // UX policy: never show stale in-memory lines after reconnect/open.
        clearAllPaneContents();
        wsSetStatus("connected", "connected");
        wsRetryDelay = 1000;
    });

    ws.addEventListener("message", e => {
        let msg;
        try { msg = JSON.parse(e.data); } catch { return; }

        // Config message — server tells us the tab/pane layout upfront.
        // Create all tabs before any log data arrives.
        if (msg.type === "config") {
            if (typeof msg.app_name === "string" && msg.app_name.trim()) {
                const appNameEl = document.querySelector("#toolbar .app-name");
                if (appNameEl) appNameEl.textContent = msg.app_name.trim();
            }

            const sessionId = msg.session?.id || null;
            const isSessionChange = currentSessionId && sessionId && currentSessionId !== sessionId;
            if (isSessionChange) {
                resetLayoutForNewSession();
            }
            currentSessionId = sessionId || currentSessionId;

            window.__embedLogSetSession?.(msg.session || null);
            window.__embedLogOnSessionHtmlStatus?.({
                ...msg.session,
                type: "session_html_status",
            });
            if (TABS.length === 0 && msg.tabs && msg.tabs.length > 0) {
                msg.tabs.forEach(tab =>
                    createTabWithPanes(tab.label, tab.panes, { switchTo: false })
                );
                switchTab(0);
            }
            window.__embedLogAfterConfig?.(msg.tabs || []);
            return;
        }

        if (msg.type === "session_html_status") {
            window.__embedLogOnSessionHtmlStatus?.(msg);
            return;
        }

        if (msg.type !== "rx" && msg.type !== "tx") return;

        const { type, data, timestamp, source_id } = msg;
        if (!source_id) return;

        // Unknown source_id — server has no --tab for it; create a tab on the fly.
        if (!PANES.includes(source_id)) {
            createDynamicTab(source_id, source_id);
        }
        appendLine(source_id, timestamp || "", data || "", type === "tx");
    });

    ws.addEventListener("close", () => {
        wsSetStatus("disconnected", `reconnecting in ${wsRetryDelay / 1000}s…`);
        setTimeout(() => {
            wsRetryDelay = Math.min(wsRetryDelay * 2, WS_MAX_DELAY);
            wsConnect();
        }, wsRetryDelay);
    });

    ws.addEventListener("error", () => ws.close());
}

wsConnect();
