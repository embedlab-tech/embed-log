"use strict";

// ---------------------------------------------------------------------------
// Settings panel — injected after #toolbar
// Gear button (⚙) in the toolbar toggles the panel open/closed.
// Every change rerenders all panes; no page reload needed.
// ---------------------------------------------------------------------------
(function () {
    const toolbar = document.getElementById("toolbar");
    const wsStatus = document.getElementById("ws-status");

    // ---- Gear button ----
    const gearBtn = document.createElement("button");
    gearBtn.id        = "btn-settings";
    gearBtn.title     = "Settings";
    gearBtn.textContent = "⚙";
    wsStatus.before(gearBtn);   // goes just left of the WS status badge

    // ---- Settings panel (inserted after toolbar) ----
    const panel = document.createElement("div");
    panel.id = "settings-panel";
    toolbar.after(panel);

    gearBtn.addEventListener("click", () => {
        panel.classList.toggle("open");
        gearBtn.classList.toggle("active");
    });

    // ---- Builder helpers ----
    function label(text) {
        const s = document.createElement("span");
        s.className   = "set-label";
        s.textContent = text;
        return s;
    }

    function sep() {
        const s = document.createElement("span");
        s.className   = "set-sep";
        s.textContent = "|";
        return s;
    }

    // Button that acts as a binary toggle (active = on).
    function makeToggle(text, initialOn, onChange) {
        const btn = document.createElement("button");
        btn.textContent = text;
        btn.classList.toggle("active", initialOn);
        btn.addEventListener("click", () => {
            btn.classList.toggle("active");
            onChange(btn.classList.contains("active"));
            PANES.forEach(rerenderPane);
        });
        return btn;
    }

    // Group of mutually exclusive buttons; returns the array so the caller
    // can keep references if needed.
    function makeRadioGroup(options, current, onChange) {
        return options.map(([value, text]) => {
            const btn = document.createElement("button");
            btn.textContent = text;
            btn.dataset.value = value;
            btn.classList.toggle("active", value === current);
            btn.addEventListener("click", () => {
                if (btn.classList.contains("active")) return; // already selected
                grp.forEach(b => b.classList.remove("active"));
                btn.classList.add("active");
                onChange(value);
                PANES.forEach(rerenderPane);
            });
            return btn;
        });
        // `grp` is bound below after the array exists
    }

    // ---- Time format ----
    panel.appendChild(label("Time:"));
    const tsOptions = [["full", "Full"], ["time", "Time"], ["compact", "Compact"]];
    const grp = tsOptions.map(([value, text]) => {
        const btn = document.createElement("button");
        btn.textContent = text;
        btn.dataset.value = value;
        btn.classList.toggle("active", value === state.settings.tsFormat);
        btn.addEventListener("click", () => {
            if (btn.classList.contains("active")) return;
            grp.forEach(b => b.classList.remove("active"));
            btn.classList.add("active");
            state.settings.tsFormat = value;
            PANES.forEach(rerenderPane);
        });
        panel.appendChild(btn);
        return btn;
    });

    // ---- Tag colours ----
    panel.appendChild(sep());
    panel.appendChild(makeToggle(
        "Tag colors",
        state.settings.tagColors,
        v => { state.settings.tagColors = v; }
    ));

    // ---- Embedded timestamp strip ----
    panel.appendChild(sep());
    panel.appendChild(makeToggle(
        "Strip inline ts",
        state.settings.embedTsStrip,
        v => { state.settings.embedTsStrip = v; }
    ));
})();
