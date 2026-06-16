// Event detection timeline — SVG swimlane visualization.
//
// One horizontal lane per unique event_id, events plotted as circles coloured
// by severity.  Supports zoom, drag-to-pan, filtering by source/severity,
// hover tooltips, and click-to-sync log panes.
//
// The Events "tab" is managed here independently of the pane-based tab system.
// tabs.js calls __embedLogRenderEventsTab to let us append our button, and
// switchTab hides our content when a regular tab is activated.

import { state, TABS, PANES, paneLabel, formatRelativeTimestamp } from './state.js';
import { switchTab } from './tabs.js';
import { scrollPaneToTs } from './lines.js';

// ── Constants ─────────────────────────────────────────────────────────────

const SVG_NS = 'http://www.w3.org/2000/svg';

const SEVERITY_COLORS = {
    info:  'var(--event-info, #3b82f6)',
    warn:  'var(--event-warn, #eab308)',
    error: 'var(--event-error, #ef4444)',
    fatal: 'var(--event-fatal, #991b1b)',
};
const SEVERITY_ORDER = ['fatal', 'error', 'warn', 'info'];

const LANE_HEIGHT   = 22;
const LEFT_MARGIN   = 134;
const RIGHT_MARGIN  = 16;
const TOP_MARGIN    = 8;
const BOTTOM_MARGIN = 28;

// ── Module state ──────────────────────────────────────────────────────────

let _contentEl  = null;   // #events-tab-content — the full tab panel
let _svgWrapEl  = null;   // div wrapping the <svg>
let _filterEl   = null;   // source/severity filter bar
let _tooltipEl  = null;   // hover tooltip
let _eventsBtn  = null;   // tab-bar button

let _viewRange = null;     // {start, end} in epoch-ms; null = auto-range
let _panState  = null;     // active drag state
let _renderRaf = null;     // coalesce event bursts into one SVG rebuild
let _hasRenderedEventSnapshot = false;

let _hiddenSources    = new Set();
let _hiddenSeverities = new Set();

// ── Public API ────────────────────────────────────────────────────────────

export function initEventsTab() {
    if (!state.eventsEnabled) return;
    if (_contentEl) return;          // already built
    _buildDom();
    _buildTooltip();
    _renderFilters();
    renderTimeline();
}

export function destroyEventsTab() {
    _contentEl?.remove();
    _tooltipEl?.remove();
    _contentEl  = null;
    _svgWrapEl  = null;
    _filterEl   = null;
    _tooltipEl  = null;
    _eventsBtn  = null;
    _viewRange  = null;
    _panState   = null;
    if (_renderRaf !== null) cancelAnimationFrame(_renderRaf);
    _renderRaf = null;
    _hasRenderedEventSnapshot = false;
    _hiddenSources.clear();
    _hiddenSeverities.clear();
    state.eventsTabActive = false;
}

export function addEvent(ev) {
    state.events.push(ev);
    _updateCount();
    // Avoid rebuilding a large hidden SVG for every incoming event. Render the
    // first event so tests/DOM consumers can observe dots, then keep the hidden
    // tab cheap until the user opens it.
    if (_contentEl && (state.eventsTabActive || !_hasRenderedEventSnapshot)) {
        _scheduleTimelineRender();
    }
}

export function renderTimeline() {
    _renderRaf = null;
    if (!_contentEl || !_svgWrapEl) return;
    _updateCount();
    const lanes  = _computeLanes();
    const events = _filteredEvents();

    const range  = _effectiveRange();
    const width  = _svgWrapEl.clientWidth || 800;
    const innerW = Math.max(50, width - LEFT_MARGIN - RIGHT_MARGIN);
    const height = TOP_MARGIN + Math.max(1, lanes.size) * LANE_HEIGHT + BOTTOM_MARGIN;

    _svgWrapEl.innerHTML = '';
    _svgWrapEl.appendChild(_buildSvg(events, lanes, range, width, height, innerW));
    _hasRenderedEventSnapshot = state.events.length > 0;
}

function _scheduleTimelineRender() {
    if (_renderRaf !== null) return;
    _renderRaf = requestAnimationFrame(renderTimeline);
}

function _updateCount() {
    const countEl = _contentEl?.querySelector('.events-count');
    if (countEl) countEl.textContent = state.events.length ? `${state.events.length} events` : 'Waiting for events…';
}

// ── DOM construction ──────────────────────────────────────────────────────

function _buildDom() {
    _contentEl = document.createElement('div');
    _contentEl.id = 'events-tab-content';
    _contentEl.className = 'tab-content events-content';
    _contentEl.style.display = 'none';

    const header = document.createElement('div');
    header.className = 'events-header';
    header.innerHTML =
        '<span class="events-title">⚡ Event Timeline</span>' +
        '<span class="events-count"></span>' +
        '<button class="events-nav-toggle" id="events-nav-toggle" title="Include event markers in marker navigation">⚡ in nav</button>' +
        '<div class="events-controls">' +
            '<button class="events-btn" data-zoom="-1" title="Zoom out">−</button>' +
            '<button class="events-btn" data-zoom="1" title="Zoom in">+</button>' +
            '<button class="events-btn" data-zoom="0" title="Reset view">⟳</button>' +
        '</div>';

    _svgWrapEl = document.createElement('div');
    _svgWrapEl.className = 'events-svg-wrap';

    _filterEl = document.createElement('div');
    _filterEl.className = 'events-filter-bar';

    _contentEl.append(header, _svgWrapEl, _filterEl);
    document.getElementById('container').appendChild(_contentEl);

    // Wire zoom buttons
    header.querySelectorAll('.events-btn').forEach(btn => {
        btn.addEventListener('click', () => {
            const z = parseInt(btn.dataset.zoom, 10);
            if (z === 0) _viewRange = null;
            else _zoom(z);
            renderTimeline();
        });
    });

    // Toggle event markers in main marker navigation
    const navToggle = document.getElementById('events-nav-toggle');
    if (navToggle) {
        navToggle.addEventListener('click', () => {
            window.__embedLogToggleEventMarkers?.();
            const active = navToggle.classList.toggle('active');
            // Sync visual state with the global flag
            if (active !== state.includeEventMarkers) {
                state.includeEventMarkers = active;
            }
        });
    }

    _initInteraction();

    // Events tab button (appended to tab-bar via hook)
    _eventsBtn = document.createElement('button');
    _eventsBtn.className = 'tab-btn events-tab-btn';
    _eventsBtn.textContent = '⚡ Events';
    _eventsBtn.addEventListener('click', _activateEventsTab);
}

function _buildTooltip() {
    _tooltipEl = document.createElement('div');
    _tooltipEl.id = 'events-tooltip';
    document.body.appendChild(_tooltipEl);
}

// ── SVG rendering ─────────────────────────────────────────────────────────

function _buildSvg(events, lanes, range, width, height, innerW) {
    const svg = document.createElementNS(SVG_NS, 'svg');
    svg.setAttribute('width', width);
    svg.setAttribute('height', height);
    svg.classList.add('events-timeline-svg');

    const span = range.end - range.start || 1;
    const xScale = t => LEFT_MARGIN + (t - range.start) / span * innerW;

    // Lane stripes + labels
    lanes.forEach((laneIdx, eventId) => {
        const y = TOP_MARGIN + laneIdx * LANE_HEIGHT;
        if (laneIdx % 2 === 1) {
            svg.appendChild(_el('rect', {
                x: LEFT_MARGIN, y, width: innerW, height: LANE_HEIGHT,
                class: 'events-lane-bg',
            }));
        }
        svg.appendChild(_el('line', {
            x1: LEFT_MARGIN,  y1: y + LANE_HEIGHT / 2,
            x2: LEFT_MARGIN + innerW, y2: y + LANE_HEIGHT / 2,
            class: 'events-lane-line',
        }));
        const label = _el('text', {
            x: LEFT_MARGIN - 8, y: y + LANE_HEIGHT / 2 + 4,
            'text-anchor': 'end', class: 'events-lane-label',
        });
        label.textContent = eventId;
        svg.appendChild(label);
    });

    // Time-axis ticks
    const tickCount = Math.min(8, Math.max(3, Math.floor(innerW / 90)));
    for (let i = 0; i <= tickCount; i++) {
        const t = range.start + span * i / tickCount;
        const x = xScale(t);
        svg.appendChild(_el('line', {
            x1: x, y1: TOP_MARGIN, x2: x,
            y2: height - BOTTOM_MARGIN + 4,
            class: 'events-tick',
        }));
        const tl = _el('text', {
            x, y: height - BOTTOM_MARGIN + 18,
            'text-anchor': 'middle', class: 'events-tick-label',
        });
        tl.textContent = _formatTime(t);
        svg.appendChild(tl);
    }

    // Event dots
    events.forEach((ev, i) => {
        const laneIdx = lanes.get(ev.event_id);
        if (laneIdx === undefined) return;
        const cx = xScale(ev.timestamp_num);
        const cy = TOP_MARGIN + laneIdx * LANE_HEIGHT + LANE_HEIGHT / 2;
        const dot = _el('circle', {
            cx, cy, r: 5,
            fill: SEVERITY_COLORS[ev.severity] || SEVERITY_COLORS.info,
            class: 'events-dot',
            'data-event-idx': i,
        });
        // Larger invisible hit target
        const hit = _el('circle', {
            cx, cy, r: 10,
            fill: 'transparent',
            class: 'events-dot-hit',
            'data-event-idx': i,
        });
        svg.appendChild(dot);
        svg.appendChild(hit);
    });

    return svg;
}

// ── Interaction ───────────────────────────────────────────────────────────

function _initInteraction() {
    _svgWrapEl.addEventListener('pointerdown', e => {
        if (e.target.closest('.events-dot-hit')) return;   // let click fire
        const range = _effectiveRange();
        const innerW = Math.max(50, _svgWrapEl.clientWidth - LEFT_MARGIN - RIGHT_MARGIN);
        _panState = { startX: e.clientX, range, innerW };
        try { _svgWrapEl.setPointerCapture(e.pointerId); } catch (_) {}
        _svgWrapEl.style.cursor = 'grabbing';
    });

    _svgWrapEl.addEventListener('pointermove', e => {
        if (_panState) {
            const dx = e.clientX - _panState.startX;
            const span = _panState.range.end - _panState.range.start || 1;
            const deltaMs = -dx / _panState.innerW * span;
            _viewRange = {
                start: _panState.range.start + deltaMs,
                end:   _panState.range.end + deltaMs,
            };
            renderTimeline();
            return;
        }
        const hit = e.target.closest('.events-dot-hit');
        if (hit) {
            const ev = _filteredEvents()[parseInt(hit.getAttribute('data-event-idx'), 10)];
            if (ev) _showTooltip(ev, e.clientX, e.clientY);
        } else {
            _hideTooltip();
        }
    });

    _svgWrapEl.addEventListener('pointerup', e => {
        if (!_panState) return;
        _panState = null;
        _svgWrapEl.style.cursor = '';
        try { _svgWrapEl.releasePointerCapture(e.pointerId); } catch (_) {}
    });

    _svgWrapEl.addEventListener('pointerleave', _hideTooltip);

    _svgWrapEl.addEventListener('click', e => {
        const hit = e.target.closest('.events-dot-hit');
        if (!hit) return;
        const ev = _filteredEvents()[parseInt(hit.getAttribute('data-event-idx'), 10)];
        if (ev) _onEventClick(ev);
    });
}

function _onEventClick(ev) {
    const syncTs = (state.timestampMode === 'relative' && Number.isFinite(ev.rel_num))
        ? ev.rel_num
        : ev.timestamp_num;
    state.syncTs = syncTs;
    state.syncTabSwitch = true;

    // Switch to the tab (or unwrapped pane) that contains the event source
    if (state.unwrap) {
        const paneIdx = PANES.indexOf(ev.source_id);
        if (paneIdx >= 0) switchTab(paneIdx);
    } else {
        const tabIdx = TABS.findIndex(t => t.panes.includes(ev.source_id));
        if (tabIdx >= 0) switchTab(tabIdx);
    }

    // Scroll the source pane to the event's timestamp in the active timestamp mode.
    scrollPaneToTs(ev.source_id, syncTs);
}

function _zoom(direction) {
    const dataRange = _dataRange();
    const range = _viewRange || _effectiveRange();
    const center = (range.start + range.end) / 2;
    const span = range.end - range.start || 1;
    const newSpan = span * (direction > 0 ? 0.6 : 1.7);

    let start = center - newSpan / 2;
    let end = center + newSpan / 2;
    // Clamp to data range when zooming out
    if (direction < 0) {
        if (start < dataRange.start) { start = dataRange.start; end = start + newSpan; }
        if (end > dataRange.end) { end = dataRange.end; start = end - newSpan; }
    }
    _viewRange = { start, end };
}

// ── Tooltip ───────────────────────────────────────────────────────────────

function _showTooltip(ev, x, y) {
    if (!_tooltipEl) return;
    const captures = Array.isArray(ev.captures) && ev.captures.length
        ? `<div class="et-captures">${ev.captures.map(c => `<code>${_esc(c)}</code>`).join(' ')}</div>`
        : '';
    _tooltipEl.innerHTML =
        `<div class="et-head"><span class="et-sev et-sev-${ev.severity}">${ev.severity}</span>${_esc(ev.event_id)}</div>` +
        `<div class="et-meta">${_esc(paneLabel(ev.source_id))} · ${_esc(ev.timestamp || '')} · line ${ev.line_idx ?? '?'}</div>` +
        `<div class="et-msg">${_esc(ev.message || '')}</div>` +
        captures;
    const w = _tooltipEl.offsetWidth;
    _tooltipEl.style.left = Math.min(x + 12, window.innerWidth - w - 8) + 'px';
    _tooltipEl.style.top = (y + 12) + 'px';
    _tooltipEl.classList.add('visible');
}

function _hideTooltip() {
    _tooltipEl?.classList.remove('visible');
}

// ── Filtering ─────────────────────────────────────────────────────────────

function _filteredEvents() {
    return state.events.filter(ev =>
        !_hiddenSources.has(ev.source_id) &&
        !_hiddenSeverities.has(ev.severity)
    );
}

function _renderFilters() {
    if (!_filterEl) return;
    const sources = Object.keys(state.eventRules);
    if (sources.length === 0) { _filterEl.innerHTML = ''; return; }

    const usedSeverities = new Set();
    Object.values(state.eventRules).forEach(rules =>
        rules.forEach(r => usedSeverities.add(r.severity))
    );

    let html = '<div class="events-filter-group"><span class="events-filter-label">Source</span>';
    sources.forEach(src => {
        const checked = !_hiddenSources.has(src) ? 'checked' : '';
        html += `<label class="events-chip"><input type="checkbox" data-fsrc="${_esc(src)}" ${checked}>${_esc(paneLabel(src))}</label>`;
    });
    html += '</div>';

    html += '<div class="events-filter-group"><span class="events-filter-label">Severity</span>';
    SEVERITY_ORDER.forEach(sev => {
        if (!usedSeverities.has(sev)) return;
        const checked = !_hiddenSeverities.has(sev) ? 'checked' : '';
        html += `<label class="events-chip"><input type="checkbox" data-fsev="${sev}" ${checked}><span class="events-sev-dot" style="background:${SEVERITY_COLORS[sev]}"></span>${sev}</label>`;
    });
    html += '</div>';

    _filterEl.innerHTML = html;

    _filterEl.querySelectorAll('[data-fsrc]').forEach(cb =>
        cb.addEventListener('change', () => {
            cb.checked ? _hiddenSources.delete(cb.dataset.fsrc) : _hiddenSources.add(cb.dataset.fsrc);
            renderTimeline();
        })
    );
    _filterEl.querySelectorAll('[data-fsev]').forEach(cb =>
        cb.addEventListener('change', () => {
            cb.checked ? _hiddenSeverities.delete(cb.dataset.fsev) : _hiddenSeverities.add(cb.dataset.fsev);
            renderTimeline();
        })
    );
}

// ── Helpers ───────────────────────────────────────────────────────────────

function _el(tag, attrs = {}) {
    const e = document.createElementNS(SVG_NS, tag);
    for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, v);
    return e;
}

function _computeLanes() {
    const lanes = new Map();
    state.events.forEach(ev => {
        if (!_hiddenSources.has(ev.source_id) && !_hiddenSeverities.has(ev.severity)) {
            if (!lanes.has(ev.event_id)) lanes.set(ev.event_id, lanes.size);
        }
    });
    // Before the first live event arrives, still render an SVG with lanes from
    // configured rules so the Events tab has stable structure instead of a div
    // that is later replaced by an SVG.
    if (lanes.size === 0) {
        Object.entries(state.eventRules || {}).forEach(([src, rules]) => {
            if (_hiddenSources.has(src) || !Array.isArray(rules)) return;
            rules.forEach(rule => {
                if (_hiddenSeverities.has(rule.severity)) return;
                if (rule?.name && !lanes.has(rule.name)) lanes.set(rule.name, lanes.size);
            });
        });
    }
    return lanes;
}

function _dataRange() {
    const events = _filteredEvents();
    if (events.length === 0) return { start: 0, end: 1 };
    let min = Infinity, max = -Infinity;
    events.forEach(ev => {
        const t = ev.timestamp_num;
        if (Number.isFinite(t)) { if (t < min) min = t; if (t > max) max = t; }
    });
    if (!Number.isFinite(min)) return { start: 0, end: 1 };
    const span = max - min || 1000;
    return { start: min - span * 0.05, end: max + span * 0.05 };
}

function _effectiveRange() {
    return _viewRange || _dataRange();
}

function _formatTime(epochMs) {
    if (state.timestampMode === 'relative' && Number.isFinite(state.firstLogAtMs)) {
        return formatRelativeTimestamp(Math.max(0, epochMs - state.firstLogAtMs));
    }
    if (!Number.isFinite(epochMs)) return '';
    const d = new Date(epochMs);
    const pad = n => String(n).padStart(2, '0');
    const ms = String(d.getMilliseconds()).padStart(3, '0');
    return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}.${ms}`;
}

function _esc(s) {
    return String(s ?? '').replace(/[&<>"']/g, c =>
        ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c])
    );
}

// ── Tab activation ────────────────────────────────────────────────────────

function _activateEventsTab() {
    document.querySelectorAll('[id^="tab-content-"], [id^="u-tab-content-"]').forEach(el => {
        if (el !== _contentEl) el.style.display = 'none';
    });
    if (_contentEl) _contentEl.style.display = 'flex';
    document.querySelectorAll('#tab-bar .tab-btn[data-tab-idx]').forEach(b => b.classList.remove('active'));
    _eventsBtn?.classList.add('active');
    state.eventsTabActive = true;
    requestAnimationFrame(renderTimeline);   // dimensions may have changed
}

// ── Window hooks ──────────────────────────────────────────────────────────

// Called by tabs.js renderTabBar so the Events button survives tab-bar rebuilds.
// Hidden in UNWRAP mode where each pane is its own tab.
window.__embedLogRenderEventsTab = function (bar) {
    if (!state.eventsEnabled || !_eventsBtn) return;
    if (state.unwrap) { _eventsBtn.style.display = 'none'; return; }
    _eventsBtn.style.removeProperty('display');
    bar.appendChild(_eventsBtn);
};
