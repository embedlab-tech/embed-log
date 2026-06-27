import assert from 'node:assert/strict';
import test from 'node:test';
import { fileURLToPath, pathToFileURL } from 'node:url';
import path from 'node:path';

const here = path.dirname(fileURLToPath(import.meta.url));

async function importFreshState() {
  global.window = {};
  const url = pathToFileURL(path.resolve(here, '../../frontend/state.js'));
  url.search = `?t=${Date.now()}-${Math.random()}`;
  return import(url.href);
}

test('clear action relative reset uses next log as T+00 origin', async () => {
  const { state, resetRelativeTimestampForNextLog, buildTimestampInfo } = await importFreshState();
  state.timestampMode = 'relative';

  resetRelativeTimestampForNextLog();

  const first = buildTimestampInfo('06-01 00:00:05.000', {
    numTs: 5_000,
    absNum: 5_000,
    relNum: 5_000,
    relTs: 'T+00:00:05.000',
  });
  const second = buildTimestampInfo('06-01 00:00:06.250', {
    numTs: 6_250,
    absNum: 6_250,
    relNum: 6_250,
    relTs: 'T+00:00:06.250',
  });

  assert.equal(first.ts, 'T+00:00:00.000');
  assert.equal(first.numTs, 0);
  assert.equal(second.ts, 'T+00:00:01.250');
  assert.equal(second.numTs, 1_250);
});
