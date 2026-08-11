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
        // Static exports have no WS badge. Keep Settings inside the existing
        // right toolbar group; appending directly to #toolbar creates a fourth
        // grid item and makes CSS grid place the button on a second row.
        const rightGroup = toolbar?.querySelector(".toolbar-right");
        (rightGroup || toolbar)?.appendChild(gearBtn);
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
