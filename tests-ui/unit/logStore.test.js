import assert from 'node:assert/strict';
import test from 'node:test';

import { createLogStore, isRawTuple } from '../../frontend/kernel/logStore.js';

function createTestStore() {
  const analyzed = [];
  const timestampApplied = [];
  const state = { rawLines: { A: [] } };
  const store = createLogStore({
    state,
    parseAnsi: text => `html:${text}`,
    buildTimestampInfo: (ts, meta) => ({
      ts,
      numTs: Number.isFinite(meta.numTs) ? meta.numTs : -1,
      absTs: meta.timestampIso || null,
    }),
    applyTimestampModeToLine: line => {
      timestampApplied.push(line.rawText);
      line.ts = `mode:${line.ts}`;
    },
    analyzeLinePlugins: (paneId, line) => {
      analyzed.push({ paneId, rawText: line.rawText });
      line.pluginFilterText = `plugin:${line.rawText}`;
    },
  });
  return { state, store, analyzed, timestampApplied };
}

test('appendBatch stores compact tuples and ignores unknown panes', () => {
  const { state, store, analyzed } = createTestStore();

  const result = store.appendBatch([
    { paneId: 'A', ts: 't1', rawText: 'one', isTx: false, meta: { numTs: 1 } },
    { paneId: 'missing', ts: 't2', rawText: 'two', isTx: false },
  ]);

  assert.equal(state.rawLines.A.length, 1);
  assert.equal(isRawTuple(state.rawLines.A[0]), true);
  assert.deepEqual(state.rawLines.A[0], ['t1', 'one', false, { numTs: 1 }]);
  assert.deepEqual([...result.touched], ['A']);
  assert.deepEqual(result.newByPane.get('A'), [['t1', 'one', false, { numTs: 1 }]]);
  assert.deepEqual(analyzed, []);
});

test('getLine lazily hydrates tuple once and preserves cached line', () => {
  const { state, store, analyzed } = createTestStore();
  store.appendBatch([{ paneId: 'A', ts: 't1', rawText: 'one', isTx: true, meta: { numTs: 1 } }]);

  const first = store.getLine('A', 0);
  const second = store.getLine('A', 0);

  assert.equal(first, second);
  assert.equal(isRawTuple(state.rawLines.A[0]), false);
  assert.deepEqual(first, {
    paneId: 'A',
    ts: 't1',
    numTs: 1,
    absTs: null,
    html: 'html:one',
    rawText: 'one',
    isTx: true,
    pluginData: null,
    pluginFilterText: 'plugin:one',
    pluginClassNames: [],
    pluginInlineText: '',
  });
  assert.deepEqual(analyzed, [{ paneId: 'A', rawText: 'one' }]);
});

test('reanalyze and timestamp mode operations touch hydrated lines only', () => {
  const { store, analyzed, timestampApplied } = createTestStore();
  store.appendBatch([
    { paneId: 'A', ts: 't1', rawText: 'one', isTx: false },
    { paneId: 'A', ts: 't2', rawText: 'two', isTx: false },
  ]);
  store.getLine('A', 0);
  analyzed.length = 0;

  store.reanalyzeHydratedLines('A');
  store.applyTimestampModeToHydratedLines('A');

  assert.deepEqual(analyzed, [{ paneId: 'A', rawText: 'one' }]);
  assert.deepEqual(timestampApplied, ['one']);
  assert.equal(store.getLine('A', 0).ts, 'mode:t1');
  assert.equal(isRawTuple(store.getLine('A', 1)), false);
});
