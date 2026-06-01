const API_VERSION = 1;
const _registry = new Map();
const _loaded = new Map();

let _frontendPlugins = _normalizeFrontendPlugins(window.__embedLogFrontendPlugins);
let _panePlugins = _normalizePanePlugins(window.__embedLogPanePlugins);
let _pluginScripts = _normalizePluginScripts(window.__embedLogPluginScripts);

function _normalizeFrontendPlugins(value) {
    if (!value || typeof value !== 'object') return {};
    const out = {};
    Object.entries(value).forEach(([name, meta]) => {
        if (!name || !meta || typeof meta !== 'object') return;
        out[name] = {
            kind: meta.kind === 'line' ? 'line' : 'line',
            sha256: typeof meta.sha256 === 'string' ? meta.sha256 : '',
            builtin: typeof meta.builtin === 'string' ? meta.builtin : undefined,
            path: typeof meta.path === 'string' ? meta.path : undefined,
        };
    });
    return out;
}

function _normalizePanePlugins(value) {
    if (!value || typeof value !== 'object') return {};
    const out = {};
    Object.entries(value).forEach(([paneId, refs]) => {
        if (!Array.isArray(refs)) return;
        out[paneId] = refs
            .map(ref => {
                if (typeof ref === 'string' && ref.trim()) {
                    return { name: ref.trim(), options: {} };
                }
                if (!ref || typeof ref !== 'object') return null;
                const name = typeof ref.name === 'string' ? ref.name.trim() : '';
                if (!name) return null;
                const options = ref.options && typeof ref.options === 'object' && !Array.isArray(ref.options)
                    ? ref.options
                    : {};
                return { name, options };
            })
            .filter(Boolean);
    });
    return out;
}

function _normalizePluginScripts(value) {
    if (!value || typeof value !== 'object') return {};
    const out = {};
    Object.entries(value).forEach(([name, script]) => {
        if (typeof script === 'string' && script.trim()) out[name] = script;
    });
    return out;
}

function _syncGlobals() {
    window.__embedLogFrontendPlugins = _frontendPlugins;
    window.__embedLogPanePlugins = _panePlugins;
    window.__embedLogPluginScripts = _pluginScripts;
}

function _escapeHtml(value) {
    return String(value)
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;')
        .replace(/"/g, '&quot;');
}


function _register(definition) {
    if (!definition || typeof definition !== 'object') {
        throw new Error('plugin definition must be an object');
    }
    if (definition.apiVersion !== API_VERSION) {
        throw new Error(`plugin ${definition.name || '<unnamed>'} has unsupported apiVersion ${definition.apiVersion}`);
    }
    const name = typeof definition.name === 'string' ? definition.name.trim() : '';
    if (!name) {
        throw new Error('plugin definition name must be a non-empty string');
    }
    if (definition.kind !== 'line') {
        throw new Error(`plugin ${name} kind must be 'line'`);
    }
    if (typeof definition.analyzeLine !== 'function') {
        throw new Error(`plugin ${name} must define analyzeLine(ctx)`);
    }
    _registry.set(name, definition);
}

window.EmbedLogPlugins = window.EmbedLogPlugins || {};
window.EmbedLogPlugins.register = _register;

function _scriptSourceUrl(name, sha256) {
    const safeName = String(name || 'plugin').replace(/[^A-Za-z0-9_.-]+/g, '-');
    const safeSha = String(sha256 || 'dev').replace(/[^A-Za-z0-9]+/g, '').slice(0, 12) || 'dev';
    return `embed-log-plugin:${safeName}:${safeSha}.js`;
}

function _executePluginScript(name, script, sha256) {
    const source = `${script}\n//# sourceURL=${_scriptSourceUrl(name, sha256)}`;
    const el = document.createElement('script');
    el.text = source;
    document.head.appendChild(el);
    el.remove();
}

function _pluginMeta(name) {
    return _frontendPlugins[name] || null;
}

function _pluginRefsForPane(paneId) {
    return _panePlugins[paneId] || [];
}

function _sanitizeStringArray(value) {
    if (!Array.isArray(value)) return [];
    return value
        .map(item => typeof item === 'string' ? item.trim() : '')
        .filter(Boolean);
}

function _sanitizeClassNames(value) {
    return _sanitizeStringArray(value)
        .filter(cls => /^[A-Za-z0-9_-]+$/.test(cls));
}

function _sanitizeAnalysis(result) {
    if (!result || typeof result !== 'object') return null;
    const label = typeof result.label === 'string' ? result.label.trim() : '';
    const summary = typeof result.summary === 'string' ? result.summary.trim() : '';
    const details = _sanitizeStringArray(result.details);
    const filterText = typeof result.filterText === 'string' ? result.filterText.trim() : '';
    const classNames = _sanitizeClassNames(result.classNames);
    if (!label && !summary && details.length === 0 && !filterText && classNames.length === 0) {
        return null;
    }
    return {
        label,
        summary,
        details,
        filterText,
        classNames,
    };
}

export async function configurePanePlugins(frontendPlugins, panePlugins, pluginScripts) {
    _frontendPlugins = _normalizeFrontendPlugins(frontendPlugins);
    _panePlugins = _normalizePanePlugins(panePlugins);
    _pluginScripts = _normalizePluginScripts(pluginScripts);
    _syncGlobals();

    const needed = new Set();
    Object.values(_panePlugins).forEach(refs => {
        refs.forEach(ref => needed.add(ref.name));
    });

    needed.forEach(name => {
        const meta = _pluginMeta(name);
        if (!meta) {
            throw new Error(`plugin ${name} is configured on a pane but missing from frontend_plugins`);
        }
        const script = _pluginScripts[name];
        if (!script) {
            throw new Error(`plugin ${name} is configured on a pane but missing from plugin_scripts`);
        }
        if (_loaded.get(name) === meta.sha256 && _registry.has(name)) {
            return;
        }
        _executePluginScript(name, script, meta.sha256);
        if (!_registry.has(name)) {
            throw new Error(`plugin ${name} did not register itself`);
        }
        _loaded.set(name, meta.sha256);
    });
}

    // Refresh per-pane plugin indicators in the header
    if (typeof window.__embedLogRefreshPluginIndicators === 'function') {
        window.__embedLogRefreshPluginIndicators();
    }

export function resetPanePlugins() {
    _frontendPlugins = {};
    _panePlugins = {};
    _pluginScripts = {};
    _syncGlobals();
}

export function analyzeLinePlugins(paneId, line) {
    const refs = _pluginRefsForPane(paneId);
    if (!refs.length || !line) {
        line.pluginData = null;
        line.pluginFilterText = '';
        line.pluginClassNames = [];
        return;
    }

    const pluginData = {};
    const filterBits = [];
    const classNames = [];

    refs.forEach(ref => {
        const plugin = _registry.get(ref.name);
        if (!plugin) return;
        const raw = plugin.analyzeLine({
            paneId,
            options: ref.options || {},
            rawText: line.rawText ?? '',
            html: line.html ?? '',
            isTx: !!line.isTx,
            timestamp: line.ts ?? '',
            absTs: line.absTs ?? null,
            absNum: line.absNum ?? null,
            relTs: line.relTs ?? null,
            relNum: line.relNum ?? null,
            utils: {
                escapeHtml: _escapeHtml,
            },
        });
        const clean = _sanitizeAnalysis(raw);
        if (!clean) return;
        pluginData[ref.name] = clean;
        if (clean.label) filterBits.push(clean.label);
        if (clean.summary) filterBits.push(clean.summary);
        if (clean.filterText) filterBits.push(clean.filterText);
        if (clean.details.length) filterBits.push(clean.details.join(' '));
        if (clean.classNames.length) classNames.push(...clean.classNames);
    });

    line.pluginData = Object.keys(pluginData).length ? pluginData : null;
    line.pluginFilterText = filterBits.join(' ').trim();
    line.pluginClassNames = [...new Set(classNames)];
}

export function getLinePluginTooltip(line) {
    if (!line?.pluginData || !line.paneId) return '';
    const refs = _pluginRefsForPane(line.paneId);
    const sections = [];
    refs.forEach(ref => {
        const info = line.pluginData[ref.name];
        if (!info) return;
        const headerParts = [];
        const label = info.label || ref.name;
        headerParts.push(label);
        if (info.summary) headerParts.push(info.summary);
        const section = [headerParts.join(' — ')];
        if (info.details.length) section.push(...info.details);
        sections.push(section.join('\n'));
    });
    return sections.join('\n\n');
}

export function getConfiguredFrontendPlugins() {
    return _frontendPlugins;
}

export function getConfiguredPanePlugins() {
    return _panePlugins;
}

export function getConfiguredPluginScripts() {
    return _pluginScripts;
}

_syncGlobals();
