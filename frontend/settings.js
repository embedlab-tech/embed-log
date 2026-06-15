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
    if (wsStatus) {
        wsStatus.before(gearBtn);
    } else {
        toolbar?.appendChild(gearBtn);
    }

    // ---- Settings panel (inserted after toolbar) ----
    const panel = document.createElement("div");
    panel.id = "settings-panel";
    toolbar.after(panel);

    gearBtn.addEventListener("click", () => {
        panel.classList.toggle("open");
        gearBtn.classList.toggle("active");
    });


})();
