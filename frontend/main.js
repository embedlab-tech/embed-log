// Entry point for live mode.
// Import order partly matters: renderToolbar must come first so the toolbar
// is available before handler modules bind to its buttons. settings.js must
// run before fontsize.js so the settings panel exists before font controls are
// appended. profile.js must run before ws.js (which via tabcreate.js loads
// ui.js) so capability-gated features like sessionApi are available from the
// start.
import './profile.js';
import './renderToolbar.js';
import './themes.js';
import './settings.js';
import './fontsize.js';
import './persist.js';
import './ws.js';
import './export.js';
