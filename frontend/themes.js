// ---------------------------------------------------------------------------
// Theme manager
// - Quick toolbar toggle: light/dark
// - Detailed palette choice (dark + light) in settings panel
// Defaults:
//   light mode -> Whitesand
//   dark mode  -> One Dark
// ---------------------------------------------------------------------------

const DARK_PALETTES = [
    { key: "dracula",          label: "Dracula",
      vars: { "--bg":"#282a36","--panel":"#21222c","--header":"#21222c","--text":"#f8f8f2","--accent":"#bd93f9","--border":"#44475a","--selection":"#44475a","--input-bg":"#21222c","--ansi-0":"#6272a4","--ansi-1":"#ff5555","--ansi-2":"#50fa7b","--ansi-3":"#f1fa8c","--ansi-4":"#6272a4","--ansi-5":"#bd93f9","--ansi-6":"#8be9fd","--ansi-7":"#f8f8f2" } },
    { key: "one-dark",         label: "One Dark",
      vars: { "--bg":"#282c34","--panel":"#21252b","--header":"#21252b","--text":"#abb2bf","--accent":"#61afef","--border":"#3e4451","--selection":"#3e4451","--input-bg":"#21252b","--ansi-0":"#5c6370","--ansi-1":"#e06c75","--ansi-2":"#98c379","--ansi-3":"#e5c07b","--ansi-4":"#61afef","--ansi-5":"#c678dd","--ansi-6":"#56b6c2","--ansi-7":"#abb2bf" } },
    { key: "nord",             label: "Nord",
      vars: { "--bg":"#2e3440","--panel":"#3b4252","--header":"#3b4252","--text":"#eceff4","--accent":"#88c0d0","--border":"#434c5e","--selection":"#434c5e","--input-bg":"#3b4252","--ansi-0":"#616e88","--ansi-1":"#bf616a","--ansi-2":"#a3be8c","--ansi-3":"#ebcb8b","--ansi-4":"#81a1c1","--ansi-5":"#b48ead","--ansi-6":"#88c0d0","--ansi-7":"#eceff4" } },
    { key: "monokai",          label: "Monokai",
      vars: { "--bg":"#272822","--panel":"#1e1f1c","--header":"#1e1f1c","--text":"#f8f8f2","--accent":"#a6e22e","--border":"#3e3d32","--selection":"#49483e","--input-bg":"#1e1f1c","--ansi-0":"#75715e","--ansi-1":"#f92672","--ansi-2":"#a6e22e","--ansi-3":"#f4bf75","--ansi-4":"#66d9e8","--ansi-5":"#ae81ff","--ansi-6":"#66d9e8","--ansi-7":"#f8f8f2" } },
    { key: "tokyo-night",      label: "Tokyo Night",
      vars: { "--bg":"#1a1b26","--panel":"#16161e","--header":"#16161e","--text":"#c0caf5","--accent":"#7aa2f7","--border":"#292e42","--selection":"#292e42","--input-bg":"#16161e","--ansi-0":"#565f89","--ansi-1":"#f7768e","--ansi-2":"#9ece6a","--ansi-3":"#e0af68","--ansi-4":"#7aa2f7","--ansi-5":"#bb9af7","--ansi-6":"#7dcfff","--ansi-7":"#c0caf5" } },
    { key: "gruvbox-dark",     label: "Gruvbox Dark",
      vars: { "--bg":"#282828","--panel":"#1d2021","--header":"#1d2021","--text":"#ebdbb2","--accent":"#fabd2f","--border":"#3c3836","--selection":"#504945","--input-bg":"#1d2021","--ansi-0":"#928374","--ansi-1":"#fb4934","--ansi-2":"#b8bb26","--ansi-3":"#fabd2f","--ansi-4":"#83a598","--ansi-5":"#d3869b","--ansi-6":"#8ec07c","--ansi-7":"#ebdbb2" } },
    { key: "catppuccin-mocha", label: "Catppuccin Mocha",
      vars: { "--bg":"#1e1e2e","--panel":"#181825","--header":"#181825","--text":"#cdd6f4","--accent":"#cba6f7","--border":"#313244","--selection":"#45475a","--input-bg":"#181825","--ansi-0":"#6c7086","--ansi-1":"#f38ba8","--ansi-2":"#a6e3a1","--ansi-3":"#f9e2af","--ansi-4":"#89b4fa","--ansi-5":"#cba6f7","--ansi-6":"#89dceb","--ansi-7":"#cdd6f4" } },
];

const LIGHT_PALETTES = [
    { key: "whitesand",        label: "Whitesand",
      vars: { "--bg":"#fbf6ee","--panel":"#f4ede1","--header":"#efe6d8","--text":"#3f3a33","--accent":"#2f7bbd","--border":"#d7cbb8","--selection":"#e6dac6","--input-bg":"#efe6d8","--ansi-0":"#6b6257","--ansi-1":"#b6404b","--ansi-2":"#2f7f4f","--ansi-3":"#a06a00","--ansi-4":"#2f7bbd","--ansi-5":"#7a4db3","--ansi-6":"#0f7c78","--ansi-7":"#3f3a33" } },
    { key: "github-light",     label: "GitHub Light",
      vars: { "--bg":"#ffffff","--panel":"#f6f8fa","--header":"#f6f8fa","--text":"#24292f","--accent":"#0969da","--border":"#d0d7de","--selection":"#ddf4ff","--input-bg":"#f6f8fa","--ansi-0":"#6e7781","--ansi-1":"#cf222e","--ansi-2":"#116329","--ansi-3":"#9a6700","--ansi-4":"#0969da","--ansi-5":"#8250df","--ansi-6":"#0969da","--ansi-7":"#24292f" } },
    { key: "solarized-light",  label: "Solarized Light",
      vars: { "--bg":"#fdf6e3","--panel":"#eee8d5","--header":"#eee8d5","--text":"#657b83","--accent":"#268bd2","--border":"#93a1a1","--selection":"#eee8d5","--input-bg":"#eee8d5","--ansi-0":"#93a1a1","--ansi-1":"#dc322f","--ansi-2":"#859900","--ansi-3":"#b58900","--ansi-4":"#268bd2","--ansi-5":"#6c71c4","--ansi-6":"#2aa198","--ansi-7":"#657b83" } },
    { key: "catppuccin-latte", label: "Catppuccin Latte",
      vars: { "--bg":"#eff1f5","--panel":"#e6e9ef","--header":"#e6e9ef","--text":"#4c4f69","--accent":"#1e66f5","--border":"#ccd0da","--selection":"#acb0be","--input-bg":"#e6e9ef","--ansi-0":"#8c8fa1","--ansi-1":"#d20f39","--ansi-2":"#40a02b","--ansi-3":"#df8e1d","--ansi-4":"#1e66f5","--ansi-5":"#8839ef","--ansi-6":"#04a5e5","--ansi-7":"#4c4f69" } },
    { key: "gruvbox-light",    label: "Gruvbox Light",
      vars: { "--bg":"#fbf1c7","--panel":"#f2e5bc","--header":"#ebdbb2","--text":"#3c3836","--accent":"#b57614","--border":"#d5c4a1","--selection":"#ebdbb2","--input-bg":"#ebdbb2","--ansi-0":"#928374","--ansi-1":"#9d0006","--ansi-2":"#79740e","--ansi-3":"#b57614","--ansi-4":"#076678","--ansi-5":"#8f3f71","--ansi-6":"#427b58","--ansi-7":"#3c3836" } },
];

const DEFAULT_LIGHT = "whitesand";
const DEFAULT_DARK = "one-dark";

let _mode = document.documentElement.getAttribute("data-theme") === "whitesand" ? "light" : "dark";
let _lightKey = DEFAULT_LIGHT;
let _darkKey = DEFAULT_DARK;
const _listeners = new Set();

const _initial = window.__embedLogInitialThemeState;
if (_initial && typeof _initial === "object") {
    if (_initial.mode === "light" || _initial.mode === "dark") _mode = _initial.mode;
    if (typeof _initial.lightKey === "string") _lightKey = _initial.lightKey;
    if (typeof _initial.darkKey === "string") _darkKey = _initial.darkKey;
}

function _find(list, key) {
    return list.find(p => p.key === key) || list[0];
}

function _applyVars(vars) {
    const root = document.documentElement;
    for (const [k, v] of Object.entries(vars)) {
        root.style.setProperty(k, v);
    }
}

function _emit() {
    _listeners.forEach(fn => {
        try { fn(); } catch (_) {}
    });
}

function _applyCurrent() {
    const palette = _mode === "dark"
        ? _find(DARK_PALETTES, _darkKey)
        : _find(LIGHT_PALETTES, _lightKey);
    _applyVars(palette.vars);
    document.documentElement.setAttribute("data-theme", _mode === "light" ? "whitesand" : "");
    _emit();
}

function _setMode(mode) {
    _mode = mode === "dark" ? "dark" : "light";
    _applyCurrent();
}

function _setDarkPalette(key) {
    _darkKey = _find(DARK_PALETTES, key).key;
    if (_mode === "dark") _applyCurrent();
}

function _setLightPalette(key) {
    _lightKey = _find(LIGHT_PALETTES, key).key;
    if (_mode === "light") _applyCurrent();
}

function _toggle() {
    _setMode(_mode === "dark" ? "light" : "dark");
}

window.__embedLogTheme = {
    toggle: _toggle,
    isDark: () => _mode === "dark",
    mode: () => _mode,
    getState: () => ({ mode: _mode, lightKey: _lightKey, darkKey: _darkKey }),
    onChange: fn => {
        if (typeof fn !== "function") return () => {};
        _listeners.add(fn);
        return () => _listeners.delete(fn);
    },
    setDarkPalette: _setDarkPalette,
    setLightPalette: _setLightPalette,
};

function _makeSelect(items, selectedKey) {
    const sel = document.createElement("select");
    items.forEach(item => {
        const opt = document.createElement("option");
        opt.value = item.key;
        opt.textContent = item.label;
        sel.appendChild(opt);
    });
    sel.value = selectedKey;
    return sel;
}

function _mountSettingsControls() {
    const panel = document.getElementById("settings-panel");
    if (!panel) return false;
    if (document.getElementById("set-theme-light")) return true;

    const sep1 = document.createElement("span");
    sep1.className = "set-sep";
    sep1.textContent = "|";
    panel.appendChild(sep1);

    const lblLight = document.createElement("span");
    lblLight.className = "set-label";
    lblLight.textContent = "Light theme:";
    panel.appendChild(lblLight);

    const selLight = _makeSelect(LIGHT_PALETTES, _lightKey);
    selLight.id = "set-theme-light";
    selLight.addEventListener("change", () => _setLightPalette(selLight.value));
    panel.appendChild(selLight);

    const sep2 = document.createElement("span");
    sep2.className = "set-sep";
    sep2.textContent = "|";
    panel.appendChild(sep2);

    const lblDark = document.createElement("span");
    lblDark.className = "set-label";
    lblDark.textContent = "Dark theme:";
    panel.appendChild(lblDark);

    const selDark = _makeSelect(DARK_PALETTES, _darkKey);
    selDark.id = "set-theme-dark";
    selDark.addEventListener("change", () => _setDarkPalette(selDark.value));
    panel.appendChild(selDark);

    return true;
}

(function _deferMount() {
    if (_mountSettingsControls()) return;
    setTimeout(_deferMount, 50);
})();

_applyCurrent();
window.dispatchEvent(new Event("embedlog-theme-ready"));
