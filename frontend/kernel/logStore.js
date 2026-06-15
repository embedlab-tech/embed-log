export function isRawTuple(value) {
  return Array.isArray(value);
}

export function createLogStore({ state, parseAnsi, buildTimestampInfo, applyTimestampModeToLine, analyzeLinePlugins }) {
  function appendBatch(commands) {
    const touched = new Set();
    const newByPane = new Map();
    for (const command of commands) {
      const paneLines = state.rawLines[command.paneId];
      if (!paneLines) continue;
      const tuple = command.meta === undefined
        ? [command.ts, command.rawText, command.isTx]
        : [command.ts, command.rawText, command.isTx, command.meta];
      paneLines.push(tuple);
      touched.add(command.paneId);
      if (!newByPane.has(command.paneId)) newByPane.set(command.paneId, []);
      newByPane.get(command.paneId).push(tuple);
    }
    return { touched, newByPane };
  }

  function hydrateTuple(paneId, tuple) {
    const [ts, rawText, isTx, meta = {}] = tuple;
    const line = {
      paneId,
      ...buildTimestampInfo(ts, meta && typeof meta === 'object' ? meta : {}),
      html: parseAnsi(rawText),
      rawText,
      isTx,
      pluginData: null,
      pluginFilterText: '',
      pluginClassNames: [],
      pluginInlineText: '',
    };
    analyzeLinePlugins(paneId, line);
    return line;
  }

  function getLine(paneId, index) {
    const paneLines = state.rawLines[paneId];
    if (!paneLines) return null;
    const line = paneLines[index];
    if (!isRawTuple(line)) return line || null;
    const hydrated = hydrateTuple(paneId, line);
    paneLines[index] = hydrated;
    return hydrated;
  }

  function reanalyzeHydratedLines(paneId) {
    for (const line of state.rawLines[paneId] || []) {
      if (!isRawTuple(line)) analyzeLinePlugins(paneId, line);
    }
  }

  function applyTimestampModeToHydratedLines(paneId) {
    for (const line of state.rawLines[paneId] || []) {
      if (!isRawTuple(line)) applyTimestampModeToLine(line);
    }
  }

  return { appendBatch, getLine, reanalyzeHydratedLines, applyTimestampModeToHydratedLines };
}
