export function normalizeConfig(msg) {
  const safeObject = value => value && typeof value === 'object' && !Array.isArray(value) ? value : {};
  return {
    type: 'config',
    tabs: Array.isArray(msg?.tabs) ? msg.tabs : [],
    paneLabels: safeObject(msg?.pane_labels),
    paneKinds: safeObject(msg?.pane_kinds),
    paneCommands: safeObject(msg?.pane_commands),
    session: safeObject(msg?.session),
    appName: typeof msg?.app_name === 'string' ? msg.app_name : 'embed-log',
    frontendPlugins: safeObject(msg?.frontend_plugins),
    panePlugins: safeObject(msg?.pane_plugins),
    pluginScripts: safeObject(msg?.plugin_scripts),
    markers: Array.isArray(msg?.markers) ? msg.markers : [],
    raw: msg,
  };
}

export function normalizeMessage(input) {
  let raw = input;
  if (typeof input === 'string') {
    try {
      raw = JSON.parse(input);
    } catch {
      return { type: 'invalid', reason: 'invalid_json', raw: input };
    }
  }

  if (!raw || typeof raw !== 'object') {
    return { type: 'invalid', reason: 'invalid_message', raw };
  }

  if (raw.type === 'config') return normalizeConfig(raw);

  if (raw.type === 'rx' || raw.type === 'tx') {
    if (!raw.source_id) return { type: 'invalid', reason: 'missing_source_id', raw };
    return {
      type: 'log',
      paneId: raw.source_id,
      ts: raw.timestamp || '',
      rawText: raw.data || '',
      isTx: raw.type === 'tx',
      meta: {
        timestampIso: raw.timestamp_iso,
        numTs: raw.timestamp_num,
      },
      raw,
    };
  }

  return { type: 'unknown', rawType: raw.type, raw };
}
