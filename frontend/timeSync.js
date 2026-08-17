// Clock-domain-aware synchronization helpers.

export const SYNC_MATCH_TOLERANCE_MS = 5000;

export function findNearestCandidate(lines, target, domain = "system") {
    let best = null;
    for (let idx = 0; idx < (lines || []).length; idx++) {
        const candidate = (lines[idx]?.timeCandidates || []).find(item => item.domain === domain);
        if (!Number.isFinite(candidate?.num)) continue;
        const distance = Math.abs(candidate.num - target);
        if (!best || distance < best.distance) best = { idx, num: candidate.num, distance };
    }
    return best;
}

/**
 * Resolve a clicked line to the clock used by the rest of the viewer.
 * A system candidate wins. A device-only line is first matched against
 * device candidates in other panes; the matched line's system time becomes
 * the cross-pane anchor.
 */
export function resolveSyncAnchor(sourceLine, otherPaneLines) {
    const system = sourceLine?.timeCandidates?.find(item => item.domain === "system");
    if (system) return { numTs: system.num, domain: "system", deviceNum: null };

    const device = sourceLine?.timeCandidates?.find(item => item.domain === "device");
    if (!device) return null;
    let best = null;
    for (const lines of otherPaneLines || []) {
        const match = findNearestCandidate(lines, device.num, "device");
        if (match && (!best || match.distance < best.match.distance)) {
            const matchedLine = lines[match.idx];
            const matchedSystem = matchedLine?.timeCandidates?.find(item => item.domain === "system");
            best = { match, matchedSystem };
        }
    }
    if (best?.matchedSystem && best.match.distance <= SYNC_MATCH_TOLERANCE_MS) {
        return { numTs: best.matchedSystem.num, domain: "system", deviceNum: device.num };
    }
    return { numTs: device.num, domain: "device", deviceNum: device.num };
}
