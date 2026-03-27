"use strict";

let ws = null;
let wsRetryDelay = 1000;
const WS_MAX_DELAY = 16000;
const wsStatus = document.getElementById("ws-status");

function wsSetStatus(cls, text) {
    wsStatus.className    = cls;
    wsStatus.textContent  = "WS: " + text;
}

function wsSend(obj) {
    if (ws && ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify(obj));
    }
}

function wsConnect() {
    wsSetStatus("connecting", "connecting…");
    ws = new WebSocket("ws://" + window.location.host + "/ws");

    ws.addEventListener("open", () => {
        wsSetStatus("connected", "connected");
        wsRetryDelay = 1000;
    });

    ws.addEventListener("message", e => {
        let msg;
        try { msg = JSON.parse(e.data); } catch { return; }

        // Config message — server tells us the tab/pane layout upfront.
        // Create all tabs before any log data arrives.
        if (msg.type === "config") {
            if (msg.tabs && msg.tabs.length > 0) {
                msg.tabs.forEach(tab =>
                    createTabWithPanes(tab.label, tab.panes, { switchTo: false })
                );
                switchTab(0);
            }
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
