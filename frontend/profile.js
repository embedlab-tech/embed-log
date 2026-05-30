// Profile: tells the viewer what mode we're in and what features are enabled.
// Modules that need capability gating read window.__embedLogProfile directly.
// This is done so it works in both live mode (ES module) and static mode
// (module syntax stripped, window.__embedLogProfile set by bootstrap config).
//
// In live mode: import this module to trigger the side effect that sets
// window.__embedLogProfile. The default profile is LIVE.
// In static mode: bootstrap/config sets window.__embedLogProfile before
// the bundled classic scripts run.

export const STATIC_PROFILE = {
    kind: "static",
    capabilities: {
        clearAll: false,
        downloadRaw: true,
        exportHtml: false,
        fontSize: true,
        paneSwap: true,
        persistCache: false,
        selectionExportHtml: true,
        sessionApi: false,
        themeToggle: true,
        tx: false,
        unwrap: true,
        wsStatus: false,
        dynamicTabs: false,
        markers: false,
    },
};

const LIVE_PROFILE = {
    kind: "live",
    capabilities: {
        clearAll: true,
        downloadRaw: true,
        exportHtml: true,
        fontSize: true,
        paneSwap: true,
        persistCache: true,
        selectionExportHtml: true,
        sessionApi: true,
        themeToggle: true,
        tx: true,
        unwrap: true,
        wsStatus: true,
        dynamicTabs: false,
        markers: true,
    },
};

const profile = window.__embedLogProfile || LIVE_PROFILE;
window.__embedLogProfile = profile;

export const PROFILE = profile;
export function can(feature) {
    return profile.capabilities[feature] === true;
}
