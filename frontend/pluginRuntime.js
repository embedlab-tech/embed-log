const API_VERSION = 1;
const _registry = new Map();
const _loaded = new Map();

let _frontendPlugins = _normalizeFrontendPlugins(window.__embedLogFrontendPlugins);
let _panePlugins = _normalizePanePlugins(window.__embedLogPanePlugins);
let _pluginScripts = _normalizePluginScripts(window.__embedLogPluginScripts);
let _panePluginUiState = _normalizePanePluginUiState(
    window.__embedLogPanePluginUiState || window.__embedLogInitialPanePluginUiState,
);

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

function _isPlainObject(value) {
    return !!value && typeof value === 'object' && !Array.isArray(value);
}

function _normalizePanePluginUiState(value) {
    if (!_isPlainObject(value)) return {};
    const out = {};
    Object.entries(value).forEach(([paneId, pluginState]) => {
        if (!_isPlainObject(pluginState)) return;
        const paneOut = {};
        Object.entries(pluginState).forEach(([pluginName, settings]) => {
            if (!_isPlainObject(settings)) return;
            const settingsOut = {};
            Object.entries(settings).forEach(([key, rawValue]) => {
                if (!key) return;
                if (typeof rawValue === 'boolean' || typeof rawValue === 'number' || typeof rawValue === 'string') {
                    settingsOut[key] = rawValue;
                }
            });
            if (Object.keys(settingsOut).length) paneOut[pluginName] = settingsOut;
        });
        if (Object.keys(paneOut).length) out[paneId] = paneOut;
    });
    return out;
}

function _clonePanePluginUiState(value = _panePluginUiState) {
    const out = {};
    Object.entries(value || {}).forEach(([paneId, pluginState]) => {
        if (!_isPlainObject(pluginState)) return;
        out[paneId] = {};
        Object.entries(pluginState).forEach(([pluginName, settings]) => {
            if (!_isPlainObject(settings)) return;
            out[paneId][pluginName] = { ...settings };
        });
        if (!Object.keys(out[paneId]).length) delete out[paneId];
    });
    return out;
}

function _syncGlobals() {
    window.__embedLogFrontendPlugins = _frontendPlugins;
    window.__embedLogPanePlugins = _panePlugins;
    window.__embedLogPluginScripts = _pluginScripts;
    window.__embedLogPanePluginUiState = _panePluginUiState;
}

function _escapeHtml(value) {
    return String(value)
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;')
        .replace(/"/g, '&quot;');
}

function _sanitizeSettingDefinitions(value) {
    if (!Array.isArray(value)) return [];
    const out = [];
    value.forEach(raw => {
        if (!_isPlainObject(raw)) return;
        const key = typeof raw.key === 'string' ? raw.key.trim() : '';
        const label = typeof raw.label === 'string' ? raw.label.trim() : '';
        if (!key || !label) return;
        const type = raw.type === 'boolean' ? 'boolean' : '';
        if (!type) return;
        out.push({
            key,
            type,
            label,
            description: typeof raw.description === 'string' ? raw.description.trim() : '',
            defaultValue: raw.defaultValue === true,
        });
    });
    return out;
}

function _coerceSettingValue(setting, value) {
    if (setting.type === 'boolean') {
        if (value === true || value === false) return value;
        if (typeof value === 'string') {
            const normalized = value.trim().toLowerCase();
            if (normalized === 'true' || normalized === '1' || normalized === 'yes' || normalized === 'on') return true;
            if (normalized === 'false' || normalized === '0' || normalized === 'no' || normalized === 'off' || normalized === '') return false;
        }
        return !!value;
    }
    return value;
}

function _pluginSettingsDefs(plugin) {
    return Array.isArray(plugin?.settings) ? plugin.settings : [];
}

function _effectiveOptionsForPanePlugin(paneId, ref, plugin) {
    const merged = {};
    _pluginSettingsDefs(plugin).forEach(setting => {
        merged[setting.key] = setting.defaultValue;
    });

    if (_isPlainObject(ref?.options)) {
        Object.entries(ref.options).forEach(([key, rawValue]) => {
            const setting = _pluginSettingsDefs(plugin).find(item => item.key === key);
            merged[key] = setting ? _coerceSettingValue(setting, rawValue) : rawValue;
        });
    }

    const pluginUiState = _panePluginUiState[paneId]?.[ref?.name];
    if (_isPlainObject(pluginUiState)) {
        Object.entries(pluginUiState).forEach(([key, rawValue]) => {
            const setting = _pluginSettingsDefs(plugin).find(item => item.key === key);
            merged[key] = setting ? _coerceSettingValue(setting, rawValue) : rawValue;
        });
    }

    return merged;
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

    _registry.set(name, {
        ...definition,
        name,
        displayName: typeof definition.displayName === 'string' && definition.displayName.trim()
            ? definition.displayName.trim()
            : name,
        settings: _sanitizeSettingDefinitions(definition.settings),
    });
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
    const inlineText = typeof result.inlineText === 'string' ? result.inlineText.trim() : '';
    const disableTooltip = result.disableTooltip === true;
    if (!label && !summary && details.length === 0 && !filterText && classNames.length === 0 && !inlineText) {
        return null;
    }
    return {
        label,
        summary,
        details,
        filterText,
        classNames,
        inlineText,
        disableTooltip,
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

    if (typeof window.__embedLogRefreshPluginIndicators === 'function') {
        window.__embedLogRefreshPluginIndicators();
    }
}

export function resetPanePlugins() {
    _frontendPlugins = {};
    _panePlugins = {};
    _pluginScripts = {};
    _panePluginUiState = {};
    _syncGlobals();
}

export function replacePanePluginUiState(nextState) {
    _panePluginUiState = _normalizePanePluginUiState(nextState);
    _syncGlobals();
}

export function getPanePluginUiState() {
    return _clonePanePluginUiState();
}

export function getPanePluginSettings(paneId) {
    const refs = _pluginRefsForPane(paneId);
    const out = [];
    refs.forEach(ref => {
        const plugin = _registry.get(ref.name);
        const settings = _pluginSettingsDefs(plugin);
        if (!plugin || !settings.length) return;
        const effective = _effectiveOptionsForPanePlugin(paneId, ref, plugin);
        out.push({
            name: ref.name,
            displayName: plugin.displayName || ref.name,
            settings: settings.map(setting => ({
                key: setting.key,
                type: setting.type,
                label: setting.label,
                description: setting.description,
                value: _coerceSettingValue(setting, effective[setting.key]),
            })),
        });
    });
    return out;
}

export function setPanePluginSetting(paneId, pluginName, key, value) {
    const ref = _pluginRefsForPane(paneId).find(item => item.name === pluginName);
    const plugin = _registry.get(pluginName);
    const setting = _pluginSettingsDefs(plugin).find(item => item.key === key);
    if (!ref || !plugin || !setting) return false;

    const nextValue = _coerceSettingValue(setting, value);
    const nextState = _clonePanePluginUiState();
    if (!nextState[paneId]) nextState[paneId] = {};
    if (!nextState[paneId][pluginName]) nextState[paneId][pluginName] = {};
    nextState[paneId][pluginName][key] = nextValue;
    _panePluginUiState = nextState;
    _syncGlobals();
    return true;
}

export function analyzeLinePlugins(paneId, line) {
    const refs = _pluginRefsForPane(paneId);
    if (!refs.length || !line) {
        line.pluginData = null;
        line.pluginFilterText = '';
        line.pluginClassNames = [];
        line.pluginInlineText = '';
        return;
    }

    const pluginData = {};
    const filterBits = [];
    const classNames = [];
    let inlineText = '';

    refs.forEach(ref => {
        const plugin = _registry.get(ref.name);
        if (!plugin) return;
        let raw;
        try {
            raw = plugin.analyzeLine({
                paneId,
                options: _effectiveOptionsForPanePlugin(paneId, ref, plugin),
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
        } catch (_) {
            // Plugin failure is isolated — raw log rendering continues.
            return;
        }
        const clean = _sanitizeAnalysis(raw);
        if (!clean) return;
        pluginData[ref.name] = clean;
        if (clean.label) filterBits.push(clean.label);
        if (clean.summary) filterBits.push(clean.summary);
        if (clean.filterText) filterBits.push(clean.filterText);
        if (clean.details.length) filterBits.push(clean.details.join(' '));
        if (clean.inlineText) {
            filterBits.push(clean.inlineText);
            if (!inlineText) inlineText = clean.inlineText;
        }
        if (clean.classNames.length) classNames.push(...clean.classNames);
    });

    line.pluginData = Object.keys(pluginData).length ? pluginData : null;
    line.pluginFilterText = filterBits.join(' ').trim();
    line.pluginClassNames = [...new Set(classNames)];
    line.pluginInlineText = inlineText;
}

export function getLinePluginTooltip(line) {
    if (!line?.pluginData || !line.paneId) return '';
    const refs = _pluginRefsForPane(line.paneId);
    const sections = [];
    refs.forEach(ref => {
        const info = line.pluginData[ref.name];
        if (!info || info.disableTooltip) return;
        const headerParts = [];
        const plugin = _registry.get(ref.name);
        const label = info.label || plugin?.displayName || ref.name;
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
