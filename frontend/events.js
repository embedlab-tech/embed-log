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
import { scrollPaneToLineIdx, scrollPaneToTs } from './lines.js';

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
const MIN_LANE_HEIGHT = 18;
const MAX_LANE_HEIGHT = 40;
const LEFT_MARGIN   = 134;
const RIGHT_MARGIN  = 16;
const TOP_MARGIN    = 8;
const BOTTOM_MARGIN = 28;
const SCROLL_LANE_HEIGHT = 28;
const SCROLL_DEFAULT_PX_PER_MS = 0.04; // 40 px/s: readable spacing without huge SVGs.
const SCROLL_MIN_PX_PER_MS = 0.002;
const SCROLL_MAX_PX_PER_MS = 2;
const SCROLL_PAD_MS = 1000;
const SCROLL_EDGE_EPS = 24;

// ── Module state ──────────────────────────────────────────────────────────

let _contentEl  = null;   // #events-tab-content — the full tab panel
let _svgWrapEl  = null;   // div wrapping the <svg>
let _filterEl   = null;   // source/severity filter bar
let _eventsTooltipEl  = null;   // hover tooltip
let _eventsBtn  = null;   // tab-bar button

let _timelineMode = 'scroll'; // 'scroll' = zoomable horizontal scroll, 'zoom' = fit range to viewport
let _scrollPxPerMs = SCROLL_DEFAULT_PX_PER_MS;
let _viewRange = null;        // fit-view mode {start, end} in epoch-ms; null = auto-range
let _panState  = null;        // active drag state
let _renderRaf = null;        // coalesce event bursts into one SVG rebuild
let _hasRenderedEventSnapshot = false;
let _selectedEventKey = null;
let _tooltipPinned = false;
let _tooltipHideTimer = null;
let _pendingScrollAnchorMs = null;
let _forceScrollRight = false;
let _renderedRange = null;
let _renderedInnerW = 0;
let _renderedLaneH = SCROLL_LANE_HEIGHT;

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

    // Re-render when the SVG container resizes (window resize, splitter drag, etc.)
    if (typeof ResizeObserver !== 'undefined' && _svgWrapEl) {
        new ResizeObserver(() => {
            if (state.eventsTabActive) _scheduleTimelineRender();
        }).observe(_svgWrapEl);
    }
}

export function destroyEventsTab() {
    _contentEl?.remove();
    _eventsTooltipEl?.remove();
    _contentEl  = null;
    _svgWrapEl  = null;
    _filterEl   = null;
    _eventsTooltipEl  = null;
    _eventsBtn  = null;
    _timelineMode = 'scroll';
    _scrollPxPerMs = SCROLL_DEFAULT_PX_PER_MS;
    _viewRange  = null;
    _panState   = null;
    _selectedEventKey = null;
    _tooltipPinned = false;
    if (_tooltipHideTimer !== null) clearTimeout(_tooltipHideTimer);
    _tooltipHideTimer = null;
    _pendingScrollAnchorMs = null;
    _forceScrollRight = false;
    _renderedRange = null;
    _renderedInnerW = 0;
    _renderedLaneH = SCROLL_LANE_HEIGHT;
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

    const prevScrollLeft = _svgWrapEl.scrollLeft;
    const wasAtRight = _isScrolledToRight();
    const clientW = _svgWrapEl.clientWidth || 800;
    const laneCount = Math.max(1, lanes.size);
    const availH = _svgWrapEl.clientHeight || 300;

    let range, width, innerW, laneH, height;
    if (_timelineMode === 'scroll') {
        range = _scrollRange();
        const span = range.end - range.start || 1;
        innerW = Math.max(50, span * _scrollPxPerMs);
        width = Math.max(clientW, LEFT_MARGIN + RIGHT_MARGIN + Math.ceil(innerW));
        laneH = SCROLL_LANE_HEIGHT;
        height = Math.max(availH, TOP_MARGIN + laneCount * laneH + BOTTOM_MARGIN);
    } else {
        range = _effectiveRange();
        width = clientW;
        innerW = Math.max(50, width - LEFT_MARGIN - RIGHT_MARGIN);
        laneH = Math.max(MIN_LANE_HEIGHT, Math.min(MAX_LANE_HEIGHT, Math.floor((availH - TOP_MARGIN - BOTTOM_MARGIN) / laneCount)));
        height = Math.max(availH, TOP_MARGIN + laneCount * laneH + BOTTOM_MARGIN);
    }

    _renderedRange = range;
    _renderedInnerW = innerW;
    _renderedLaneH = laneH;
    _svgWrapEl.innerHTML = '';
    _svgWrapEl.appendChild(_buildSvg(events, lanes, range, width, height, innerW, laneH));

    if (_timelineMode === 'scroll') {
        if (_forceScrollRight) {
            _scrollToRight();
            _forceScrollRight = false;
        } else if (Number.isFinite(_pendingScrollAnchorMs)) {
            _centerScrollOnTime(_pendingScrollAnchorMs);
            _pendingScrollAnchorMs = null;
        } else if (wasAtRight || !_hasRenderedEventSnapshot) {
            _scrollToRight();
        } else {
            _svgWrapEl.scrollLeft = prevScrollLeft;
        }
    } else {
        _svgWrapEl.scrollLeft = 0;
    }

    _hasRenderedEventSnapshot = state.events.length > 0;
    _syncModeUi();
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
        '<div class="events-step-controls" title="Select and center events without leaving the timeline">' +
            '<button class="events-btn" data-event-nav="-1" title="Previous event">◀ event</button>' +
            '<button class="events-btn" data-event-nav="1" title="Next event">event ▶</button>' +
            '<button class="events-btn" data-event-nav="latest" title="Jump to latest event">Latest</button>' +
        '</div>' +
        '<div class="events-controls">' +
            '<button class="events-mode-btn" data-events-mode="scroll" title="Zoomable horizontal timeline; follows the right edge while you are at latest">Scroll</button>' +
            '<button class="events-mode-btn" data-events-mode="zoom" title="Fit the selected time range to the available screen">Fit</button>' +
            '<button class="events-btn events-zoom-btn" data-zoom="-1" title="Zoom out">−</button>' +
            '<button class="events-btn events-zoom-btn" data-zoom="1" title="Zoom in">+</button>' +
            '<button class="events-btn events-reset-btn" data-zoom="0" title="Reset view / jump latest">⟳</button>' +
        '</div>';

    _svgWrapEl = document.createElement('div');
    _svgWrapEl.className = 'events-svg-wrap';

    _filterEl = document.createElement('div');
    _filterEl.className = 'events-filter-bar';

    _contentEl.append(header, _svgWrapEl, _filterEl);
    document.getElementById('container').appendChild(_contentEl);

    // Mode + zoom controls.
    header.querySelectorAll('[data-events-mode]').forEach(btn => {
        btn.addEventListener('click', () => _setTimelineMode(btn.dataset.eventsMode));
    });
    header.querySelectorAll('[data-zoom]').forEach(btn => {
        btn.addEventListener('click', () => {
            const z = parseInt(btn.dataset.zoom, 10);
            if (z === 0) _resetTimelineView();
            else _zoom(z);
            renderTimeline();
        });
    });
    header.querySelectorAll('[data-event-nav]').forEach(btn => {
        btn.addEventListener('click', () => _navigateEvent(btn.dataset.eventNav));
    });
    _syncModeUi();

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
    _eventsTooltipEl = document.createElement('div');
    _eventsTooltipEl.id = 'events-tooltip';
    document.body.appendChild(_eventsTooltipEl);
}

// ── SVG rendering ─────────────────────────────────────────────────────────

function _buildSvg(events, lanes, range, width, height, innerW, laneH) {
    const svg = document.createElementNS(SVG_NS, 'svg');
    svg.setAttribute('width', width);
    svg.setAttribute('height', height);
    svg.classList.add('events-timeline-svg');

    const span = range.end - range.start || 1;
    const xScale = t => LEFT_MARGIN + (t - range.start) / span * innerW;

    // Lane stripes + labels
    lanes.forEach((laneIdx, eventId) => {
        const y = TOP_MARGIN + laneIdx * laneH;
        if (laneIdx % 2 === 1) {
            svg.appendChild(_el('rect', {
                x: LEFT_MARGIN, y, width: innerW, height: laneH,
                class: 'events-lane-bg',
            }));
        }
        svg.appendChild(_el('line', {
            x1: LEFT_MARGIN,  y1: y + laneH / 2,
            x2: LEFT_MARGIN + innerW, y2: y + laneH / 2,
            class: 'events-lane-line',
        }));
        const label = _el('text', {
            x: LEFT_MARGIN - 8, y: y + laneH / 2 + 4,
            'text-anchor': 'end', class: 'events-lane-label',
        });
        label.textContent = eventId;
        svg.appendChild(label);
    });

    // Time-axis ticks. In scroll mode ticks are aligned to absolute time
    // intervals so existing grid lines don't shift when new events extend the
    // right edge.
    _timelineTicks(range, innerW).forEach(t => {
        const x = xScale(t);
        if (x < LEFT_MARGIN - 1 || x > LEFT_MARGIN + innerW + 1) return;
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
    });

    // Event dots
    events.forEach((ev, i) => {
        const laneIdx = lanes.get(ev.event_id);
        if (laneIdx === undefined) return;
        const cx = xScale(ev.timestamp_num);
        const cy = TOP_MARGIN + laneIdx * laneH + laneH / 2;
        const selected = _eventKey(ev) === _selectedEventKey;
        if (selected) {
            svg.appendChild(_el('line', {
                x1: cx, y1: TOP_MARGIN, x2: cx, y2: height - BOTTOM_MARGIN + 4,
                class: 'events-selected-line',
            }));
        }
        // Larger invisible hit target. It is appended before the visual dot and
        // the dot has pointer-events disabled, so hover/click stays stable.
        const hit = _el('circle', {
            cx, cy, r: 11,
            fill: 'transparent',
            class: 'events-dot-hit' + (selected ? ' selected' : ''),
            'data-event-idx': i,
            'data-event-id': ev.event_id,
            'data-source-id': ev.source_id,
        });
        const dot = _el('circle', {
            cx, cy, r: selected ? 7 : 5,
            fill: SEVERITY_COLORS[ev.severity] || SEVERITY_COLORS.info,
            class: 'events-dot' + (selected ? ' selected' : ''),
            'data-event-idx': i,
            'data-event-id': ev.event_id,
            'data-source-id': ev.source_id,
            'data-severity': ev.severity || 'info',
        });
        svg.appendChild(hit);
        svg.appendChild(dot);
    });

    return svg;
}

// ── Interaction ───────────────────────────────────────────────────────────

function _initInteraction() {
    _svgWrapEl.addEventListener('wheel', e => {
        if (_timelineMode !== 'scroll') return;
        // Treat a normal vertical wheel as horizontal timeline scroll. Shift+wheel
        // still works naturally because browsers usually put the delta in X.
        const delta = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY;
        if (!delta) return;
        _svgWrapEl.scrollLeft += delta;
        e.preventDefault();
    }, { passive: false });

    _svgWrapEl.addEventListener('pointerdown', e => {
        if (e.target.closest('.events-dot-hit')) return;   // let click fire
        if (_timelineMode === 'scroll') {
            _panState = { mode: 'scroll', startX: e.clientX, scrollLeft: _svgWrapEl.scrollLeft };
        } else {
            const range = _effectiveRange();
            const innerW = Math.max(50, _svgWrapEl.clientWidth - LEFT_MARGIN - RIGHT_MARGIN);
            _panState = { mode: 'zoom', startX: e.clientX, range, innerW };
        }
        try { _svgWrapEl.setPointerCapture(e.pointerId); } catch (_) {}
        _svgWrapEl.style.cursor = 'grabbing';
    });

    _svgWrapEl.addEventListener('pointermove', e => {
        if (_panState?.mode === 'scroll') {
            _svgWrapEl.scrollLeft = _panState.scrollLeft - (e.clientX - _panState.startX);
            return;
        }
        if (_panState?.mode === 'zoom') {
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
            if (ev) {
                // If the user clicked this event and the action popup is open,
                // don't downgrade it back to a fragile hover tooltip just
                // because the pointer moved over the dot again.
                if (_tooltipPinned && _eventKey(ev) === _selectedEventKey) {
                    _cancelTooltipHide();
                } else {
                    _showTooltip(ev, e.clientX, e.clientY);
                }
            }
        } else {
            _scheduleTooltipHide();
        }
    });

    _svgWrapEl.addEventListener('pointerup', e => {
        if (!_panState) return;
        _panState = null;
        _svgWrapEl.style.cursor = '';
        try { _svgWrapEl.releasePointerCapture(e.pointerId); } catch (_) {}
    });

    _svgWrapEl.addEventListener('pointerleave', () => _scheduleTooltipHide());

    _svgWrapEl.addEventListener('click', e => {
        const hit = e.target.closest('.events-dot-hit');
        if (!hit) return;
        const ev = _filteredEvents()[parseInt(hit.getAttribute('data-event-idx'), 10)];
        if (ev) _onEventClick(ev);
    });
}

function _onEventClick(ev) {
    _selectedEventKey = _eventKey(ev);
    renderTimeline();
    _scrollEventIntoView(ev);
    _showTooltipForEvent(ev, { action: true });
}

function _jumpToEventLog(ev) {
    _hideTooltip({ force: true });
    const syncTs = (state.timestampMode === 'relative' && Number.isFinite(ev.rel_num))
        ? ev.rel_num
        : ev.timestamp_num;
    state.syncTs = syncTs;
    state.syncTabSwitch = true;

    // Switch to the tab (or unwrapped pane) that contains the event source.
    if (state.unwrap) {
        const paneIdx = PANES.indexOf(ev.source_id);
        if (paneIdx >= 0) switchTab(paneIdx);
    } else {
        const tabIdx = TABS.findIndex(t => t.panes.includes(ev.source_id));
        if (tabIdx >= 0) switchTab(tabIdx);
    }

    // Prefer exact backend line_idx when available; timestamp is the fallback.
    if (Number.isFinite(ev.line_idx)) scrollPaneToLineIdx(ev.source_id, ev.line_idx, syncTs);
    else scrollPaneToTs(ev.source_id, syncTs);
}

function _setTimelineMode(mode) {
    const next = mode === 'zoom' ? 'zoom' : 'scroll';
    if (_timelineMode === next) return;

    if (next === 'zoom') {
        _viewRange = _visibleScrollRange() || _effectiveRange();
    } else {
        const range = _viewRange || _effectiveRange();
        _pendingScrollAnchorMs = (range.start + range.end) / 2;
        _viewRange = null;
    }
    _timelineMode = next;
    renderTimeline();
}

function _syncModeUi() {
    _contentEl?.querySelectorAll('[data-events-mode]').forEach(btn => {
        btn.classList.toggle('active', btn.dataset.eventsMode === _timelineMode);
    });
    const reset = _contentEl?.querySelector('.events-reset-btn');
    if (reset) reset.title = _timelineMode === 'scroll'
        ? 'Reset zoom scale and jump to latest event'
        : 'Reset fit range to all events';
}

function _navigateEvent(dir) {
    const events = _filteredEvents();
    if (!events.length) return;
    let idx = events.findIndex(ev => _eventKey(ev) === _selectedEventKey);
    if (dir === 'latest') {
        idx = events.length - 1;
    } else {
        const step = Number(dir) || 1;
        idx = idx < 0 ? (step > 0 ? 0 : events.length - 1) : Math.max(0, Math.min(events.length - 1, idx + step));
    }
    const ev = events[idx];
    if (!ev) return;
    _selectedEventKey = _eventKey(ev);
    if (_timelineMode === 'zoom') _ensureTimeVisibleInZoom(ev.timestamp_num);
    renderTimeline();
    _scrollEventIntoView(ev);
    _showTooltipForEvent(ev, { action: true });
}

function _resetTimelineView() {
    if (_timelineMode === 'scroll') {
        _scrollPxPerMs = SCROLL_DEFAULT_PX_PER_MS;
        _pendingScrollAnchorMs = null;
        _forceScrollRight = true;
        return;
    }
    _viewRange = null;
}

function _zoom(direction) {
    if (_timelineMode === 'scroll') {
        const wasAtRight = _isScrolledToRight();
        const visible = _visibleScrollRange();
        const center = visible ? (visible.start + visible.end) / 2 : null;
        const factor = direction > 0 ? 1.7 : 1 / 1.7;
        _scrollPxPerMs = Math.max(SCROLL_MIN_PX_PER_MS, Math.min(SCROLL_MAX_PX_PER_MS, _scrollPxPerMs * factor));
        _pendingScrollAnchorMs = wasAtRight ? null : center;
        return;
    }

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

function _showTooltip(ev, x, y, { action = false } = {}) {
    if (!_eventsTooltipEl) return;
    _cancelTooltipHide();
    _tooltipPinned = action;
    const captures = Array.isArray(ev.captures) && ev.captures.length
        ? `<div class="et-captures">${ev.captures.map(c => `<code>${_esc(c)}</code>`).join(' ')}</div>`
        : '';
    const deltas = _eventDeltas(ev);
    const deltaDetails = [
        deltas.previous && `<div>Δ previous event: <code>${_formatDuration(deltas.previous)}</code></div>`,
        deltas.sameRule && `<div>Δ previous ${_esc(ev.event_id)}: <code>${_formatDuration(deltas.sameRule)}</code></div>`,
    ].filter(Boolean).join('');
    const actions = action
        ? '<div class="et-actions"><button type="button" class="events-jump-log-btn">Jump to log</button></div>'
        : '';
    _eventsTooltipEl.innerHTML =
        `<div class="et-head"><span class="et-sev et-sev-${ev.severity}">${ev.severity}</span>${_esc(ev.event_id)}</div>` +
        `<div class="et-meta">${_esc(paneLabel(ev.source_id))} · ${_esc(ev.timestamp || '')} · line ${ev.line_idx ?? '?'}</div>` +
        `<div class="et-msg">${_esc(ev.message || '')}</div>` +
        (deltaDetails ? `<div class="et-deltas">${deltaDetails}</div>` : '') +
        captures + actions;
    _eventsTooltipEl.classList.toggle('actionable', action);
    if (action) {
        _eventsTooltipEl.querySelector('.events-jump-log-btn')?.addEventListener('click', e => {
            e.stopPropagation();
            _jumpToEventLog(ev);
        });
    }
    const w = _eventsTooltipEl.offsetWidth;
    const h = _eventsTooltipEl.offsetHeight;
    _eventsTooltipEl.style.left = Math.min(Math.max(8, x + 12), Math.max(8, window.innerWidth - w - 8)) + 'px';
    _eventsTooltipEl.style.top = Math.min(Math.max(8, y + 12), Math.max(8, window.innerHeight - h - 8)) + 'px';
    _eventsTooltipEl.classList.add('visible');
}

function _showTooltipForEvent(ev, options = {}) {
    if (!_svgWrapEl || !_eventsTooltipEl) return;
    const x = _timeToRenderedX(ev.timestamp_num);
    const laneIdx = _computeLanes().get(ev.event_id);
    if (x === null || laneIdx === undefined) return;

    const rect = _svgWrapEl.getBoundingClientRect();
    const screenX = rect.left + x - _svgWrapEl.scrollLeft;
    const y = TOP_MARGIN + laneIdx * _renderedLaneH + _renderedLaneH / 2;
    const screenY = rect.top + y - _svgWrapEl.scrollTop;
    _showTooltip(ev, screenX, screenY, options);
}

function _cancelTooltipHide() {
    if (_tooltipHideTimer !== null) {
        clearTimeout(_tooltipHideTimer);
        _tooltipHideTimer = null;
    }
}

function _scheduleTooltipHide(delay = 450) {
    if (_tooltipPinned) return;
    _cancelTooltipHide();
    _tooltipHideTimer = setTimeout(() => {
        _tooltipHideTimer = null;
        _hideTooltip();
    }, delay);
}

function _hideTooltip({ force = false } = {}) {
    _cancelTooltipHide();
    if (_tooltipPinned && !force) return;
    _tooltipPinned = false;
    _eventsTooltipEl?.classList.remove('visible', 'actionable');
}

window.__embedLogHideEventsTooltip = function () {
    _hideTooltip({ force: true });
};

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

function _rawDataRange() {
    const events = _filteredEvents();
    if (events.length === 0) return { start: 0, end: 1 };
    let min = Infinity, max = -Infinity;
    events.forEach(ev => {
        const t = ev.timestamp_num;
        if (Number.isFinite(t)) { if (t < min) min = t; if (t > max) max = t; }
    });
    return Number.isFinite(min) ? { start: min, end: max } : { start: 0, end: 1 };
}

function _dataRange() {
    const range = _rawDataRange();
    const span = range.end - range.start || 1000;
    return { start: range.start - span * 0.05, end: range.end + span * 0.05 };
}

function _scrollRange() {
    const range = _rawDataRange();
    const end = Math.max(range.end, range.start + 1);
    return { start: range.start - SCROLL_PAD_MS, end: end + SCROLL_PAD_MS };
}

function _effectiveRange() {
    return _viewRange || _dataRange();
}

function _timelineTicks(range, innerW) {
    const span = range.end - range.start || 1;
    if (_timelineMode !== 'scroll') {
        const tickCount = Math.min(8, Math.max(3, Math.floor(innerW / 90)));
        return Array.from({ length: tickCount + 1 }, (_, i) => range.start + span * i / tickCount);
    }

    const msPerPx = span / Math.max(1, innerW);
    const step = _niceTickStep(msPerPx * 110);
    const ticks = [];
    const first = Math.ceil(range.start / step) * step;
    for (let t = first; t <= range.end; t += step) ticks.push(t);
    return ticks;
}

function _niceTickStep(targetMs) {
    const steps = [
        100, 250, 500,
        1_000, 2_000, 5_000, 10_000, 15_000, 30_000,
        60_000, 2 * 60_000, 5 * 60_000, 10 * 60_000, 15 * 60_000, 30 * 60_000,
        60 * 60_000,
    ];
    return steps.find(step => step >= targetMs) || steps[steps.length - 1];
}

function _eventKey(ev) {
    return [ev.source_id, ev.line_idx ?? '', ev.event_id, ev.timestamp_num].join('|');
}

function _eventDeltas(ev) {
    if (!Number.isFinite(ev.timestamp_num)) return {};
    const ordered = state.events
        .filter(candidate => Number.isFinite(candidate.timestamp_num))
        .slice()
        .sort((a, b) => a.timestamp_num - b.timestamp_num);
    const index = ordered.findIndex(candidate => _eventKey(candidate) === _eventKey(ev));
    if (index <= 0) return {};
    const previous = ordered[index - 1];
    const sameRule = ordered.slice(0, index).reverse().find(candidate =>
        candidate.source_id === ev.source_id && candidate.event_id === ev.event_id
    );
    return {
        previous: ev.timestamp_num - previous.timestamp_num,
        sameRule: sameRule ? ev.timestamp_num - sameRule.timestamp_num : null,
    };
}

function _formatDuration(ms) {
    if (!Number.isFinite(ms) || ms < 0) return '';
    if (ms < 1_000) return `${Math.round(ms)} ms`;
    if (ms < 60_000) return `${(ms / 1_000).toFixed(ms < 10_000 ? 3 : 1)} s`;
    const minutes = Math.floor(ms / 60_000);
    return `${minutes}m ${((ms % 60_000) / 1_000).toFixed(1)}s`;
}

function _isScrolledToRight() {
    if (!_svgWrapEl) return true;
    return _svgWrapEl.scrollWidth - _svgWrapEl.scrollLeft - _svgWrapEl.clientWidth <= SCROLL_EDGE_EPS;
}

function _scrollToRight() {
    if (!_svgWrapEl) return;
    _svgWrapEl.scrollLeft = Math.max(0, _svgWrapEl.scrollWidth - _svgWrapEl.clientWidth);
}

function _timeToRenderedX(ms) {
    const range = _renderedRange;
    if (!range || !Number.isFinite(ms)) return null;
    const span = range.end - range.start || 1;
    return LEFT_MARGIN + (ms - range.start) / span * _renderedInnerW;
}

function _centerScrollOnTime(ms) {
    const x = _timeToRenderedX(ms);
    if (x === null || !_svgWrapEl) return;
    _svgWrapEl.scrollLeft = Math.max(0, x - _svgWrapEl.clientWidth / 2);
}

function _scrollEventIntoView(ev) {
    if (_timelineMode !== 'scroll') return;
    const x = _timeToRenderedX(ev.timestamp_num);
    if (x === null || !_svgWrapEl) return;
    const left = _svgWrapEl.scrollLeft;
    const right = left + _svgWrapEl.clientWidth;
    if (x < left + LEFT_MARGIN || x > right - RIGHT_MARGIN) _centerScrollOnTime(ev.timestamp_num);
}

function _visibleScrollRange() {
    if (!_svgWrapEl || !_renderedRange || !_renderedInnerW) return null;
    const span = _renderedRange.end - _renderedRange.start || 1;
    const start = _renderedRange.start + Math.max(0, _svgWrapEl.scrollLeft - LEFT_MARGIN) / _renderedInnerW * span;
    const end = _renderedRange.start + Math.max(0, _svgWrapEl.scrollLeft + _svgWrapEl.clientWidth - LEFT_MARGIN) / _renderedInnerW * span;
    return end > start ? { start, end } : null;
}

function _ensureTimeVisibleInZoom(ms) {
    if (!Number.isFinite(ms)) return;
    const range = _viewRange || _effectiveRange();
    if (ms >= range.start && ms <= range.end) return;
    const span = range.end - range.start || 1000;
    _viewRange = { start: ms - span / 2, end: ms + span / 2 };
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

function _focusEventFromMarker(marker) {
    const sourceId = marker?.paneId || marker?.source_id;
    const lineIdx = Number(marker?.lineIdx ?? marker?.line_idx);
    const ev = _filteredEvents().find(candidate =>
        candidate.source_id === sourceId && Number(candidate.line_idx) === lineIdx
    );
    if (!ev) return false;

    _selectedEventKey = _eventKey(ev);
    _activateEventsTab();
    requestAnimationFrame(() => {
        if (_timelineMode === 'zoom') _ensureTimeVisibleInZoom(ev.timestamp_num);
        renderTimeline();
        _scrollEventIntoView(ev);
        _showTooltipForEvent(ev, { action: true });
    });
    return true;
}

function _activateEventsTab() {
    _hideTooltip({ force: true });
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

window.__embedLogJumpToEvent = function (marker) {
    return _focusEventFromMarker(marker || {});
};
