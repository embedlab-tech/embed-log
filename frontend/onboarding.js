(function () {
    function invoke(command, args) {
        const fn = window.__TAURI__?.core?.invoke;
        if (fn) return fn(command, args || {});

        if (command === "list_serial_ports") {
            return fetch("/api/serial_ports").then(r => {
                if (!r.ok) throw new Error(`serial ports API failed: ${r.status}`);
                return r.json();
            });
        }
        if (command === "save_quick_config") {
            return fetch("/api/save_config", {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify(args || {})
            }).then(async r => {
                const text = await r.text();
                if (!r.ok) throw new Error(text || `save config API failed: ${r.status}`);
                return JSON.parse(text);
            });
        }

        if (command === "get_server_status") {
            return fetch("/api/server_status").then(r => {
                if (!r.ok) throw new Error(`server status API failed: ${r.status}`);
                return r.json();
            });
        }

        return Promise.reject(new Error("Tauri API not ready"));
    }

    const state = {
        appName: "embed-log",
        wsPort: 8080,
        logsDir: "logs/",
        baudrate: 115200,
        sources: [],
        tabs: [],
        configPath: ""
    };
    let serialPorts = [];
    let selectedTab = 0;

    const presets = [
        { id: "single-uart", title: "Single serial log", desc: "One tab with one UART/serial source.", sources: [{ type: "uart", name: "device", label: "Device" }], tabs: [{ label: "Device", panes: ["device"] }] },
        { id: "dual-uart", title: "Two serial panes", desc: "One tab split into left/right UART panes.", sources: [{ type: "uart", name: "device_a", label: "Device A" }, { type: "uart", name: "device_b", label: "Device B" }], tabs: [{ label: "Devices", panes: ["device_a", "device_b"] }] },
        { id: "uart-udp-file", title: "Serial + UDP + file", desc: "A practical multi-tab setup with serial, UDP, and file watch.", sources: [{ type: "uart", name: "device", label: "Device" }, { type: "udp", name: "udp", label: "UDP", port: "9000" }, { type: "file", name: "file", label: "File" }], tabs: [{ label: "Device", panes: ["device"] }, { label: "UDP", panes: ["udp"] }, { label: "File", panes: ["file"] }] },
        { id: "blank", title: "Custom setup", desc: "Start empty and add exactly the sources/tabs you need.", sources: [], tabs: [] }
    ];

    function slugify(value) {
        const slug = String(value || "source").trim().toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_+|_+$/g, "");
        return slug || "source";
    }

    function uniqueName(base, currentId) {
        const root = slugify(base);
        const used = new Set(state.sources.filter(s => s.id !== currentId).map(s => s.name));
        if (!used.has(root)) return root;
        for (let i = 2; ; i++) {
            const candidate = `${root}_${i}`;
            if (!used.has(candidate)) return candidate;
        }
    }

    function newSource(type = "uart", template = {}) {
        const id = crypto.randomUUID ? crypto.randomUUID() : String(Date.now() + Math.random());
        const base = template.name || type;
        return {
            id,
            name: uniqueName(base),
            label: template.label || template.name || (type === "uart" ? "Serial" : type.toUpperCase()),
            type,
            port: template.port || "",
            parser: template.parser || "text",
            baudrate: template.baudrate || state.baudrate
        };
    }

    function sourceByName(name) {
        return state.sources.find(s => s.name === name);
    }

    function applyPreset(preset) {
        state.sources = [];
        for (const src of preset.sources) state.sources.push(newSource(src.type, src));
        state.tabs = preset.tabs.map(tab => ({ label: tab.label, panes: tab.panes.slice(0, 2) }));
        if (!state.tabs.length) state.tabs.push({ label: "Logs", panes: [] });
        selectedTab = 0;
        render();
    }

    function addSource(type = "uart") {
        const src = newSource(type);
        state.sources.push(src);
        if (!state.tabs.length) state.tabs.push({ label: "Logs", panes: [src.name] });
        render();
    }

    function removeSource(id) {
        const src = state.sources.find(s => s.id === id);
        state.sources = state.sources.filter(s => s.id !== id);
        if (src) {
            for (const tab of state.tabs) tab.panes = tab.panes.filter(p => p !== src.name);
        }
        render();
    }

    function addTab() {
        state.tabs.push({ label: `Tab ${state.tabs.length + 1}`, panes: state.sources[0] ? [state.sources[0].name] : [] });
        selectedTab = state.tabs.length - 1;
        render();
    }

    function removeTab(index) {
        state.tabs.splice(index, 1);
        if (!state.tabs.length) state.tabs.push({ label: "Logs", panes: [] });
        selectedTab = Math.max(0, Math.min(selectedTab, state.tabs.length - 1));
        render();
    }

    function validate() {
        const errors = [];
        if (!String(state.logsDir || "").trim()) errors.push("Enter a session logs directory.");
        if (!state.sources.length) errors.push("Add at least one source.");
        const names = new Set();
        for (const src of state.sources) {
            if (!src.name.trim()) errors.push("Every source needs a name.");
            if (names.has(src.name)) errors.push(`Duplicate source name: ${src.name}`);
            names.add(src.name);
            if ((src.type === "uart" || src.type === "file") && !src.port.trim()) errors.push(`${src.label || src.name}: enter a ${src.type === "uart" ? "serial port" : "file path"}.`);
            if (src.type === "udp" && (!src.port || Number(src.port) < 1 || Number(src.port) > 65535)) errors.push(`${src.label || src.name}: enter a valid UDP port.`);
            if (!src.parser) errors.push(`${src.label || src.name}: choose a parser.`);
            if (src.parser === "cbor-datagram" && src.type !== "udp") errors.push(`${src.label || src.name}: CBOR parser is only supported for UDP sources.`);
        }
        if (!state.tabs.length) errors.push("Add at least one tab.");
        for (const tab of state.tabs) {
            if (!tab.label.trim()) errors.push("Every tab needs a label.");
            if (!tab.panes.length) errors.push(`${tab.label || "Tab"}: select at least one pane.`);
            if (tab.panes.length > 2) errors.push(`${tab.label || "Tab"}: at most two panes are supported.`);
            for (const pane of tab.panes) if (!names.has(pane)) errors.push(`${tab.label}: unknown source ${pane}.`);
        }
        return [...new Set(errors)];
    }

    function draft() {
        return {
            app_name: state.appName,
            ws_port: Number(state.wsPort) || 8080,
            logs_dir: state.logsDir,
            baudrate: Number(state.baudrate) || 115200,
            sources: state.sources.map(s => ({
                name: s.name,
                label: s.label,
                source_type: s.type,
                port: s.port,
                parser: s.parser,
                baudrate: Number(s.baudrate) || Number(state.baudrate) || 115200
            })),
            tabs: state.tabs.map(t => ({ label: t.label, panes: t.panes.slice(0, 2) }))
        };
    }

    function configPreview() {
        const d = draft();
        const lines = ["version: 1", `baudrate: ${d.baudrate}`, "", "sources:"];
        for (const s of d.sources) {
            lines.push(`  - name: ${s.name}`);
            lines.push(`    type: ${s.source_type}`);
            lines.push(`    port: ${s.source_type === "udp" ? Number(s.port) : s.port || "<choose>"}`);
            if (s.source_type === "uart") lines.push(`    baudrate: ${s.baudrate}`);
            if (s.label) lines.push(`    label: ${s.label}`);
            lines.push("    parser:");
            lines.push(`      type: ${s.parser}`);
        }
        lines.push("", "tabs:");
        for (const t of d.tabs) {
            lines.push(`  - label: ${t.label}`);
            lines.push("    panes:");
            for (const p of t.panes) lines.push(`      - ${p}`);
        }
        lines.push("", "server:", `  app_name: ${d.app_name}`, `  ws_port: ${d.ws_port}`, "", "logs:", `  dir: ${d.logs_dir}`);
        return lines.join("\n");
    }

    function render() {
        const root = document.getElementById("quick-setup-root");
        const tab = state.tabs[selectedTab] || state.tabs[0] || { label: "Logs", panes: [] };
        const errors = validate();
        root.innerHTML = `
            <div class="qs-shell">
                <header class="qs-header">
                    <div><div class="qs-eyebrow">First run setup</div><h1>Create your embed-log view</h1><p>Select sources, arrange them into tabs, then save this as your desktop config.</p>${state.configPath ? `<p class="qs-muted">Config will be saved to <code>${escapeHtml(state.configPath)}</code>. Relative log paths are stored next to this config.</p>` : ""}</div>
                    <button id="qs-start" class="qs-primary" ${errors.length ? "disabled" : ""}>Start logging</button>
                </header>
                <section class="qs-card">
                    <h2>1. Choose a starting point</h2>
                    <div class="qs-presets">${presets.map(p => `<button class="qs-preset" data-preset="${p.id}"><b>${p.title}</b><span>${p.desc}</span><em>${presetDiagram(p)}</em></button>`).join("")}</div>
                </section>
                <div class="qs-grid">
                    <section class="qs-card">
                        <div class="qs-row qs-between"><h2>2. Sources</h2><div><button class="qs-small" data-add-source="uart">+ Serial</button><button class="qs-small" data-add-source="udp">+ UDP</button><button class="qs-small" data-add-source="file">+ File</button></div></div>
                        <div class="qs-list">${state.sources.map(sourceEditor).join("") || `<p class="qs-muted">No sources yet. Add serial, UDP, or file source.</p>`}</div>
                    </section>
                    <section class="qs-card">
                        <div class="qs-row qs-between"><h2>3. Tabs and panes</h2><button class="qs-small" id="qs-add-tab">+ Tab</button></div>
                        <div class="qs-tabs">${state.tabs.map((t, i) => `<button class="${i === selectedTab ? "active" : ""}" data-select-tab="${i}">${escapeHtml(t.label || `Tab ${i + 1}`)}</button>`).join("")}</div>
                        ${tabEditor(tab, selectedTab)}
                        ${layoutPreview(tab)}
                    </section>
                </div>
                <section class="qs-card">
                    <h2>4. Storage</h2>
                    <label>Session logs directory <input id="qs-logs-dir" value="${escapeAttr(state.logsDir)}" placeholder="logs/"></label>
                    <p class="qs-muted">Relative paths like <code>logs/</code> are resolved relative to the config file directory. With default onboarding, sessions are saved under <code>${escapeHtml(state.configPath ? state.configPath.replace(/[^/\\]+$/, "") : "<app config dir>/")}logs/</code>.</p>
                    <details><summary>Advanced: generated YAML preview</summary><pre>${escapeHtml(configPreview())}</pre></details>
                    <div id="qs-errors" class="qs-errors">${errors.map(e => `<div>${escapeHtml(e)}</div>`).join("")}</div>
                    <div id="qs-status" class="qs-status"></div>
                </section>
            </div>`;
        bindEvents(root);
    }

    function presetDiagram(p) {
        return p.tabs.map(t => `[${t.label}: ${t.panes.length === 2 ? "▦" : "▣"}]`).join(" ") || "custom";
    }

    function sourceEditor(src) {
        const portOptions = src.type === "uart" ? `<select data-field="port" data-source="${src.id}"><option value="">Choose serial port…</option>${serialPorts.map(p => `<option ${p === src.port ? "selected" : ""}>${escapeHtml(p)}</option>`).join("")}<option value="${escapeHtml(src.port)}" ${src.port && !serialPorts.includes(src.port) ? "selected" : ""}>${escapeHtml(src.port || "Custom…")}</option></select><input data-field="port" data-source="${src.id}" placeholder="or type serial path" value="${escapeAttr(src.port)}">`
            : `<input data-field="port" data-source="${src.id}" placeholder="${src.type === "udp" ? "9000" : "/path/to/log.txt"}" value="${escapeAttr(src.port)}">`;
        const parserOptions = src.type === "udp"
            ? `<option value="text" ${src.parser === "text" ? "selected" : ""}>Text</option><option value="cbor-datagram" ${src.parser === "cbor-datagram" ? "selected" : ""}>CBOR datagram</option>`
            : `<option value="text" selected>Text</option>`;
        if (src.type !== "udp") src.parser = "text";
        return `<div class="qs-source">
            <div class="qs-row"><strong>${src.type === "uart" ? "Serial" : src.type.toUpperCase()}</strong><button class="qs-danger" data-remove-source="${src.id}">remove</button></div>
            <label>Name <input data-field="name" data-source="${src.id}" value="${escapeAttr(src.name)}"></label>
            <label>Label <input data-field="label" data-source="${src.id}" value="${escapeAttr(src.label)}"></label>
            <label>${src.type === "udp" ? "UDP port" : src.type === "file" ? "File path" : "Serial port"} ${portOptions}</label>
            ${src.type === "uart" ? `<label>Baudrate <input type="number" data-field="baudrate" data-source="${src.id}" value="${escapeAttr(src.baudrate)}"></label>` : ""}
            <label>Parser <select data-field="parser" data-source="${src.id}">${parserOptions}</select></label>
        </div>`;
    }

    function tabEditor(tab, index) {
        return `<div class="qs-tab-editor">
            <label>Tab label <input id="qs-tab-label" value="${escapeAttr(tab.label)}"></label>
            <label>Layout <select id="qs-tab-layout"><option value="1" ${tab.panes.length !== 2 ? "selected" : ""}>One pane</option><option value="2" ${tab.panes.length === 2 ? "selected" : ""}>Two panes</option></select></label>
            <label>Pane 1 ${sourceSelect("qs-pane-0", tab.panes[0])}</label>
            <label class="${tab.panes.length === 2 ? "" : "qs-hidden"}">Pane 2 ${sourceSelect("qs-pane-1", tab.panes[1])}</label>
            <button class="qs-danger" id="qs-remove-tab" ${state.tabs.length <= 1 ? "disabled" : ""}>Remove tab</button>
        </div>`;
    }

    function sourceSelect(id, value) {
        return `<select id="${id}"><option value="">Choose source…</option>${state.sources.map(s => `<option value="${escapeAttr(s.name)}" ${s.name === value ? "selected" : ""}>${escapeHtml(s.label || s.name)} (${escapeHtml(s.name)})</option>`).join("")}</select>`;
    }

    function layoutPreview(tab) {
        const panes = tab.panes.map(p => sourceByName(p)).filter(Boolean);
        return `<div class="qs-preview"><div class="qs-preview-tabs">${state.tabs.map((t, i) => `<span class="${i === selectedTab ? "active" : ""}">${escapeHtml(t.label || "Tab")}</span>`).join("")}</div><div class="qs-preview-body ${panes.length === 2 ? "split" : ""}">${panes.map(p => `<div>${escapeHtml(p.label || p.name)}<small>${escapeHtml(p.type)} ${escapeHtml(String(p.port || ""))}</small></div>`).join("") || `<div class="empty">Choose panes</div>`}</div></div>`;
    }

    function bindEvents(root) {
        root.querySelectorAll("[data-preset]").forEach(btn => btn.onclick = () => applyPreset(presets.find(p => p.id === btn.dataset.preset)));
        root.querySelectorAll("[data-add-source]").forEach(btn => btn.onclick = () => addSource(btn.dataset.addSource));
        root.querySelectorAll("[data-remove-source]").forEach(btn => btn.onclick = () => removeSource(btn.dataset.removeSource));
        root.querySelectorAll("[data-select-tab]").forEach(btn => btn.onclick = () => { selectedTab = Number(btn.dataset.selectTab); render(); });
        root.querySelector("#qs-add-tab").onclick = addTab;
        root.querySelector("#qs-remove-tab")?.addEventListener("click", () => removeTab(selectedTab));
        root.querySelectorAll("[data-source][data-field]").forEach(input => input.onchange = input.oninput = () => {
            const src = state.sources.find(s => s.id === input.dataset.source);
            if (!src) return;
            if (input.dataset.field === "name") {
                const old = src.name;
                src.name = uniqueName(input.value, src.id);
                for (const tab of state.tabs) tab.panes = tab.panes.map(p => p === old ? src.name : p);
                render();
            } else {
                src[input.dataset.field] = input.value;
            }
        });
        root.querySelector("#qs-tab-label")?.addEventListener("input", e => { state.tabs[selectedTab].label = e.target.value; });
        root.querySelector("#qs-tab-layout")?.addEventListener("change", e => {
            const tab = state.tabs[selectedTab];
            const count = Number(e.target.value);
            if (count === 1) tab.panes = [tab.panes[0]].filter(Boolean);
            if (count === 2) tab.panes = [tab.panes[0] || state.sources[0]?.name || "", tab.panes[1] || state.sources[1]?.name || state.sources[0]?.name || ""].filter(Boolean).slice(0, 2);
            render();
        });
        root.querySelector("#qs-pane-0")?.addEventListener("change", e => { state.tabs[selectedTab].panes[0] = e.target.value; render(); });
        root.querySelector("#qs-pane-1")?.addEventListener("change", e => { state.tabs[selectedTab].panes[1] = e.target.value; render(); });
        root.querySelector("#qs-logs-dir")?.addEventListener("change", e => { state.logsDir = e.target.value || "logs/"; render(); });
        root.querySelector("#qs-start")?.addEventListener("click", start);
    }

    async function start() {
        const status = document.getElementById("qs-status");
        status.textContent = "Saving config and starting server…";
        try {
            const result = await invoke("save_quick_config", { draft: draft() });
            status.textContent = `Saved ${result.config_path}. Opening log viewer…`;
            setTimeout(() => { window.location.href = result.url; }, 700);
        } catch (error) {
            status.textContent = `Error: ${error}`;
        }
    }

    function escapeHtml(value) { return String(value ?? "").replace(/[&<>]/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" }[c])); }
    function escapeAttr(value) { return escapeHtml(value).replace(/"/g, "&quot;"); }

    document.documentElement.innerHTML = `<head><title>embed-log setup</title><style>
        :root { color-scheme: dark; --bg:#17151f; --panel:#22202c; --panel2:#2b2836; --text:#f4efff; --muted:#a9a0b8; --accent:#8bd5ff; --danger:#ff8a8a; --border:#3a3548; }
        * { box-sizing: border-box; } body { margin:0; background:linear-gradient(135deg,#16131d,#252033); color:var(--text); font:14px/1.45 system-ui,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif; }
        button,input,select { font:inherit; } input,select { width:100%; margin-top:5px; padding:9px 10px; border-radius:8px; border:1px solid var(--border); background:#17151f; color:var(--text); } label { display:block; color:var(--muted); font-size:12px; margin-top:10px; } label input,label select { color:var(--text); font-size:14px; }
        .qs-shell { max-width:1200px; margin:0 auto; padding:28px; } .qs-header { display:flex; justify-content:space-between; gap:20px; align-items:start; margin-bottom:18px; } .qs-eyebrow { color:var(--accent); text-transform:uppercase; letter-spacing:.12em; font-size:12px; font-weight:700; } h1 { margin:.15em 0; font-size:34px; } h2 { margin:0 0 12px; font-size:18px; } p { color:var(--muted); margin:.3em 0; }
        .qs-card { background:rgba(34,32,44,.92); border:1px solid var(--border); border-radius:16px; padding:18px; box-shadow:0 12px 40px rgba(0,0,0,.25); margin-bottom:16px; } .qs-grid { display:grid; grid-template-columns:1.1fr .9fr; gap:16px; } .qs-row { display:flex; align-items:center; justify-content:space-between; gap:8px; } .qs-between { margin-bottom:12px; }
        .qs-primary,.qs-small,.qs-danger,.qs-preset { border:1px solid var(--border); border-radius:10px; background:var(--panel2); color:var(--text); padding:9px 12px; cursor:pointer; } .qs-primary { background:linear-gradient(135deg,#49b6ff,#9b8cff); border:0; font-weight:700; padding:12px 18px; color:#080710; } button:disabled { opacity:.45; cursor:not-allowed; } .qs-small { margin-left:6px; font-size:12px; padding:7px 9px; } .qs-danger { color:var(--danger); font-size:12px; padding:5px 8px; background:transparent; }
        .qs-presets { display:grid; grid-template-columns:repeat(4,1fr); gap:10px; } .qs-preset { text-align:left; min-height:112px; display:flex; flex-direction:column; gap:5px; } .qs-preset:hover { border-color:var(--accent); } .qs-preset span { color:var(--muted); font-size:12px; } .qs-preset em { margin-top:auto; color:var(--accent); font-style:normal; font-size:12px; }
        .qs-list { display:grid; grid-template-columns:repeat(auto-fit,minmax(230px,1fr)); gap:12px; } .qs-source { background:#1a1822; border:1px solid var(--border); border-radius:12px; padding:12px; } .qs-tabs { display:flex; gap:6px; flex-wrap:wrap; margin-bottom:12px; } .qs-tabs button { border:1px solid var(--border); color:var(--muted); background:#181620; border-radius:999px; padding:6px 10px; } .qs-tabs button.active { color:#061018; background:var(--accent); border-color:var(--accent); }
        .qs-hidden { display:none; } .qs-preview { margin-top:16px; border:1px solid var(--border); border-radius:12px; overflow:hidden; background:#15131b; } .qs-preview-tabs { display:flex; gap:4px; padding:8px; border-bottom:1px solid var(--border); } .qs-preview-tabs span { padding:5px 9px; border-radius:8px; color:var(--muted); } .qs-preview-tabs span.active { background:var(--panel2); color:var(--text); } .qs-preview-body { min-height:170px; padding:10px; display:grid; gap:10px; } .qs-preview-body.split { grid-template-columns:1fr 1fr; } .qs-preview-body div { border:1px solid var(--border); border-radius:10px; display:flex; flex-direction:column; align-items:center; justify-content:center; color:var(--text); background:#0f0e14; } .qs-preview-body small { color:var(--muted); margin-top:8px; } .qs-preview-body .empty { color:var(--muted); }
        pre { overflow:auto; padding:14px; border-radius:12px; background:#111018; color:#d8f3ff; } summary { cursor:pointer; color:var(--accent); } .qs-errors { color:var(--danger); } .qs-status { color:var(--accent); margin-top:8px; } .qs-muted { color:var(--muted); }
        @media (max-width: 900px) { .qs-grid,.qs-presets { grid-template-columns:1fr; } .qs-header { flex-direction:column; } }
    </style></head><body><div id="quick-setup-root"></div></body>`;

    Promise.allSettled([
        invoke("list_serial_ports"),
        invoke("get_server_status")
    ]).then(results => {
        if (results[0].status === "fulfilled") serialPorts = results[0].value || [];
        if (results[1].status === "fulfilled" && results[1].value && results[1].value.config_path) {
            state.configPath = results[1].value.config_path;
        }
    }).finally(() => {
        applyPreset(presets[0]);
    });
})();
