import assert from 'node:assert/strict';
import test from 'node:test';

import { normalizeConfig, normalizeMessage } from '../../frontend/kernel/protocol.js';

test('normalizes config messages with safe collection defaults', () => {
  const command = normalizeConfig({
    type: 'config',
    tabs: [{ label: 'DevA', panes: ['A'] }],
    pane_labels: { A: 'Reader' },
    pane_kinds: { A: 'udp' },
    pane_commands: { A: ['send_raw'] },
    session: { id: 's1', timestamp_mode: 'relative' },
    app_name: 'custom app',
    frontend_plugins: { p: { kind: 'line' } },
    pane_plugins: { A: [{ name: 'p' }] },
    plugin_scripts: { p: 'script' },
    markers: [{ paneId: 'A', lineIdx: 0 }],
  });

  assert.equal(command.type, 'config');
  assert.deepEqual(command.tabs, [{ label: 'DevA', panes: ['A'] }]);
  assert.deepEqual(command.paneLabels, { A: 'Reader' });
  assert.deepEqual(command.paneKinds, { A: 'udp' });
  assert.deepEqual(command.paneCommands, { A: ['send_raw'] });
  assert.deepEqual(command.session, { id: 's1', timestamp_mode: 'relative' });
  assert.equal(command.appName, 'custom app');
  assert.deepEqual(command.frontendPlugins, { p: { kind: 'line' } });
  assert.deepEqual(command.panePlugins, { A: [{ name: 'p' }] });
  assert.deepEqual(command.pluginScripts, { p: 'script' });
  assert.deepEqual(command.markers, [{ paneId: 'A', lineIdx: 0 }]);
});

test('normalizes rx and tx messages into log commands', () => {
  const rx = normalizeMessage(JSON.stringify({
    type: 'rx',
    source_id: 'SENSOR_A',
    timestamp: '00:00:01.000',
    data: 'tick=001',
    timestamp_iso: '2026-01-01T00:00:01.000+00:00',
    timestamp_num: 1000,
  }));
  const tx = normalizeMessage({ type: 'tx', source_id: 'SENSOR_A', data: 'sent' });

  assert.deepEqual(rx, {
    type: 'log',
    paneId: 'SENSOR_A',
    ts: '00:00:01.000',
    rawText: 'tick=001',
    isTx: false,
    meta: {
      timestampIso: '2026-01-01T00:00:01.000+00:00',
      numTs: 1000,
    },
    raw: {
      type: 'rx',
      source_id: 'SENSOR_A',
      timestamp: '00:00:01.000',
      data: 'tick=001',
      timestamp_iso: '2026-01-01T00:00:01.000+00:00',
      timestamp_num: 1000,
    },
  });
  assert.equal(tx.type, 'log');
  assert.equal(tx.isTx, true);
  assert.equal(tx.rawText, 'sent');
});

test('invalid and unknown messages are explicit no-op commands', () => {
  assert.deepEqual(normalizeMessage('{'), { type: 'invalid', reason: 'invalid_json', raw: '{' });
  assert.deepEqual(normalizeMessage({ type: 'rx', data: 'missing source' }), {
    type: 'invalid',
    reason: 'missing_source_id',
    raw: { type: 'rx', data: 'missing source' },
  });
  assert.deepEqual(normalizeMessage({ type: 'future_event', value: 1 }), {
    type: 'unknown',
    rawType: 'future_event',
    raw: { type: 'future_event', value: 1 },
  });
});
