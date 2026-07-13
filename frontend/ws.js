import { state, TABS, PANES, PANE_LABELS, setTimestampContext, resetRelativeTimestampBase } from './state.js';
import { appendLineBatch, clearPane, rerenderPane, setTimestampMode, refreshStatsUi } from './lines.js';
import { createTabWithPanes } from './tabcreate.js';
import { configurePanePlugins, resetPanePlugins } from './pluginRuntime.js';

import { switchTab } from './tabs.js';
import { initEventsTab, addEvent, destroyEventsTab, renderTimeline } from './events.js';

let ws = null;
let wsRetryDelay = 1000;
const WS_MAX_DELAY = 16000;
const wsStatus = document.getElementById("ws-status");
let currentSessionId = null;
let pendingLogMessages = [];
let pendingLogFlush = false;
let configReady = false;
const LOG_FLUSH_MAX_LINES = 1000;

function resetLayoutForNewSession() {
    const container = document.getElementById("container");
    if (container) container.innerHTML = "";

    TABS.length = 0;
    PANES.length = 0;

    state.activeTab = 0;
    state.activePaneTab = 0;
    state.syncTs = null;
    state.syncTabSwitch = false;
    state.filters = {};
    state.rawLines = {};
    state.atBottom = {};
    state.highlighted = {};
    state.highlightedIdx = {};
    state.selected = {};
    Object.keys(PANE_LABELS).forEach(key => delete PANE_LABELS[key]);
    resetPanePlugins();

    // Tear down events timeline — recreated if the new config has rules.
    destroyEventsTab();
    state.events = [];
    state.eventsEnabled = false;
    state.eventRules = {};
}

function wsSetStatus(cls, text) {
    wsStatus.className = cls;
    wsStatus.textContent = "WS: " + text;
}

export function wsSend(obj) {
    if (ws && ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify(obj));
    }
}

// Expose wsSend globally so ui.js can call it without a circular import.
// In static exports this is stubbed to a no-op by the bootstrap script.
window.wsSend = wsSend;

function clearAllPaneContents({ resetRelative = false } = {}) {
    discardPendingLogMessages();
    if (resetRelative) {
        resetRelativeTimestampBase();
        state.syncTs = null;
        state.syncTabSwitch = false;
    }
    PANES.forEach(paneId => {
        const logEl = document.getElementById("log-" + paneId);
        if (!logEl) return;
        clearPane(paneId);
    });
}

function discardPendingLogMessages() {
    pendingLogMessages = [];
    pendingLogFlush = false;
}

window.__embedLogDiscardPendingLogMessages = discardPendingLogMessages;

function enqueueLogMessage(entry) {
    pendingLogMessages.push(entry);
    if (pendingLogFlush || !configReady) return;
    pendingLogFlush = true;
    requestAnimationFrame(flushLogMessages);
}

function flushLogMessages() {
    if (!configReady) {
        pendingLogFlush = false;
        return;
    }
    const batch = pendingLogMessages.splice(0, LOG_FLUSH_MAX_LINES);
    if (batch.length > 0) appendLineBatch(batch);

    if (pendingLogMessages.length > 0) {
        requestAnimationFrame(flushLogMessages);
    } else {
        pendingLogFlush = false;
    }
}

async function _handleConfigMessage(msg) {
    if (typeof msg.app_name === "string" && msg.app_name.trim()) {
        const appNameEl = document.querySelector("#toolbar .app-name");
        if (appNameEl) appNameEl.textContent = msg.app_name.trim();
    }
    window.__embedLogTheme?.applyDefaults?.(msg.theme_defaults);

    const sessionId = msg.session?.id || null;
    const isSessionChange = currentSessionId && sessionId && currentSessionId !== sessionId;
    if (isSessionChange) {
        resetLayoutForNewSession();
    }
    currentSessionId = sessionId || currentSessionId;

    // Create tabs and set pane metadata BEFORE loading plugins so log
    // lines render immediately.  Plugins load asynchronously and catch up.
    setTimestampContext({
        mode: msg.session?.timestamp_mode || "absolute",
        firstLogAt: msg.session?.first_log_at,
        resetMode: isSessionChange || (TABS.length === 0 && PANES.length === 0),
    });
    if (isSessionChange || (TABS.length === 0 && PANES.length === 0)) {
        setTimestampMode(state.timestampMode);
    }
    window.__embedLogUpdateTimestampModeUi?.();

    window.__embedLogSetSession?.(msg.session || null);
    window.__embedLogOnSessionHtmlStatus?.({
        ...msg.session,
        type: "session_html_status",
    });
    const paneLabels = msg.pane_labels && typeof msg.pane_labels === "object" ? msg.pane_labels : {};
    window.__embedLogPaneKinds = msg.pane_kinds && typeof msg.pane_kinds === "object" ? msg.pane_kinds : {};
    window.__embedLogPaneCommands = msg.pane_commands && typeof msg.pane_commands === "object" ? msg.pane_commands : {};
    Object.keys(PANE_LABELS).forEach(key => delete PANE_LABELS[key]);
    Object.assign(PANE_LABELS, paneLabels);

    // Event detection — initialize before tab creation so the Events button
    // is appended on the first renderTabBar() pass. Pane labels are already
    // assigned so source filters use user-facing names.
    const eventRules = msg.event_rules && typeof msg.event_rules === "object" ? msg.event_rules : {};
    state.eventRules = eventRules;
    state.eventsEnabled = Object.values(eventRules).some(rules => Array.isArray(rules) && rules.length > 0);
    if (state.eventsEnabled) {
        state.events = [];
        initEventsTab();
    }
    if (TABS.length === 0 && msg.tabs && msg.tabs.length > 0) {
        msg.tabs.forEach(tab =>
            createTabWithPanes(tab.label, tab.panes, { switchTo: false, paneLabels: tab.pane_labels || paneLabels })
        );
        switchTab(0);
    }
    // Apply markers from config if present
    if (msg.markers && Array.isArray(msg.markers)) {
        state.markers = {};
        msg.markers.forEach(m => {
            if (!m.paneId) return;
            state.markers[m.paneId] = state.markers[m.paneId] || [];
            state.markers[m.paneId].push(m);
        });
        window.applyMarkers?.();
        window.__embedLogOnMarkers?.();
    }
    // Allow rendering immediately — plugins will catch up asynchronously.
    configReady = true;
    if (pendingLogMessages.length > 0 && !pendingLogFlush) {
        pendingLogFlush = true;
        requestAnimationFrame(flushLogMessages);
    }

    // Load plugins in the background.  Lines rendered before plugins are
    // ready won't have plugin data, but a re-render is triggered below.
    try {
        await configurePanePlugins(
            msg.frontend_plugins && typeof msg.frontend_plugins === "object" ? msg.frontend_plugins : {},
            msg.pane_plugins && typeof msg.pane_plugins === "object" ? msg.pane_plugins : {},
            msg.plugin_scripts && typeof msg.plugin_scripts === "object" ? msg.plugin_scripts : {},
        );
    } catch (err) {
        console.error("embed-log: failed to configure pane plugins", err);
        alert(`Failed to load pane plugins: ${err.message}`);
        resetPanePlugins();
    }

    // Re-render all panes so plugin data attaches to already-visible lines.
    PANES.forEach(id => {
        try { rerenderPane(id); } catch (_) {}
    });
    window.__embedLogAfterConfig?.(msg.tabs || []);
}

function wsConnect() {
    wsSetStatus("connecting", "connecting…");
    const wsScheme = window.location.protocol === "https:" ? "wss://" : "ws://";
    ws = new WebSocket(wsScheme + window.location.host + "/ws");

    ws.addEventListener("open", () => {
        // UX policy: never show stale in-memory lines after reconnect/open.
        clearAllPaneContents();
        wsSetStatus("connected", "connected");
        wsRetryDelay = 1000;
    });

    ws.addEventListener("message", async e => {
        let msg;
        try { msg = JSON.parse(e.data); } catch { return; }

        // Config message — server tells us the tab/pane layout upfront.
        // Create all tabs before any log data arrives.
        if (msg.type === "config") {
            await _handleConfigMessage(msg);
            return;
        }

        if (msg.type === "session_info") {
            const enriched = setTimestampContext({
                mode: msg.session?.timestamp_mode || state.sessionTimestampMode,
                firstLogAt: msg.session?.first_log_at,
                resetMode: false,
            });
            if (enriched) PANES.forEach(rerenderPane);
            window.__embedLogUpdateTimestampModeUi?.();
            return;
        }
        if (msg.type === "session_html_status") {
            window.__embedLogOnSessionHtmlStatus?.(msg);
            return;
        }

        if (msg.type === "session_rotated") {
            currentSessionId = msg.session?.id || currentSessionId;
            state.syncTs = null;
            state.syncTabSwitch = false;
            state.events = [];
            renderTimeline();
            clearAllPaneContents();
            setTimestampContext({
                mode: msg.session?.timestamp_mode || "absolute",
                firstLogAt: msg.session?.first_log_at,
                resetMode: true,
            });
            setTimestampMode(state.timestampMode);
            window.__embedLogUpdateTimestampModeUi?.();
            window.__embedLogSetSession?.(msg.session || null);
            window.__embedLogOnSessionHtmlStatus?.({
                ...msg.session,
                type: "session_html_status",
            });
            window.__embedLogSchedulePersist?.();
            return;
        }

        if (msg.type === "markers_update") {
            state.markers = {};
            (msg.markers || []).forEach(m => {
                if (!m.paneId) return;
                state.markers[m.paneId] = state.markers[m.paneId] || [];
                state.markers[m.paneId].push(m);
            });
            window.applyMarkers?.();
            window.__embedLogOnMarkers?.();
            return;
        }

        if (msg.type === "filter_result") {
            const input = document.querySelector(`.filter-input[data-pane="${msg.id}"]`);
            if (input && msg.error) {
                input.classList.add("invalid");
                input.title = msg.error;
            } else if (input) {
                input.classList.remove("invalid");
                input.title = "";
            }
            return;
        }

        if (msg.type === "clear_logs") {
            const pane = typeof msg.pane === "string" && msg.pane ? msg.pane : null;
            if (pane && pane !== "all") {
                clearPane(pane);
            } else {
                clearAllPaneContents({ resetRelative: true });
            }
            refreshStatsUi();
            window.__embedLogSchedulePersist?.();
            return;
        }

        if (typeof msg.type === "string" && msg.type.startsWith("event_rule.")) {
            window.dispatchEvent(new CustomEvent("embed-log-event-rule", { detail: msg }));
            return;
        }

        if (msg.type === "event") {
            addEvent(msg);
            return;
        }

        if (msg.type !== "rx" && msg.type !== "tx") return;

        const { type, data, timestamp, timestamp_iso, timestamp_num, source_id,
                absTs, absNum, relTs, relNum, line_idx } = msg;
        if (!source_id) return;

        // Unknown source_id — server has no --tab for it; ignore with a warning.
        if (!PANES.includes(source_id)) {
            console.warn("embed-log: dropping message for unknown source_id:", source_id);
            return;
        }
        enqueueLogMessage({
            paneId: source_id,
            ts: timestamp || "",
            rawText: data || "",
            isTx: type === "tx",
            meta: {
                timestampIso: timestamp_iso,
                numTs: timestamp_num,
                absTs: absTs,
                absNum: absNum,
                relTs: relTs,
                relNum: relNum,
                lineIdx: line_idx,
            },
        });
    });

    ws.addEventListener("close", () => {
        configReady = false;
        wsSetStatus("disconnected", `reconnecting in ${wsRetryDelay / 1000}s…`);
        setTimeout(() => {
            wsRetryDelay = Math.min(wsRetryDelay * 2, WS_MAX_DELAY);
            wsConnect();
        }, wsRetryDelay);
    });

    ws.addEventListener("error", () => ws.close());
}

wsConnect();
