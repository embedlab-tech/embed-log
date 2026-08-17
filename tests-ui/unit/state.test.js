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

test('device-only click resolves through a matching live device clock to system time', async () => {
  const sync = await import(pathToFileURL(path.resolve(here, '../../frontend/timeSync.js')).href + `?t=${Date.now()}-${Math.random()}`);
  const anchor = sync.resolveSyncAnchor(
    { timeCandidates: [{ domain: 'device', num: 1_000 }] },
    [[
      { timeCandidates: [
        { domain: 'system', num: 2_000 },
        { domain: 'device', num: 1_003 },
      ] },
    ]],
  );
  assert.deepEqual(anchor, { numTs: 2_000, domain: 'system', deviceNum: 1_000 });
});

test('system candidate has priority over device candidates', async () => {
  const sync = await import(pathToFileURL(path.resolve(here, '../../frontend/timeSync.js')).href + `?t=${Date.now()}-${Math.random()}`);
  const anchor = sync.resolveSyncAnchor(
    { timeCandidates: [
      { domain: 'system', num: 3_000 },
      { domain: 'device', num: 1_000 },
    ] },
    [],
  );
  assert.deepEqual(anchor, { numTs: 3_000, domain: 'system', deviceNum: null });
});

test('device-only click falls back to device domain without a match', async () => {
  const sync = await import(pathToFileURL(path.resolve(here, '../../frontend/timeSync.js')).href + `?t=${Date.now()}-${Math.random()}`);
  const anchor = sync.resolveSyncAnchor(
    { timeCandidates: [{ domain: 'device', num: 1_000 }] },
    [[{ timeCandidates: [{ domain: 'device', num: 20_000 }] }]],
  );
  assert.deepEqual(anchor, { numTs: 1_000, domain: 'device', deviceNum: 1_000 });
});

test('timestamp candidates keep host and embedded device clocks separate', async () => {
  const { buildTimestampInfo } = await importFreshState();
  const line = buildTimestampInfo('08-17 08:49:17.384', {
    absNum: 1786956557384,
    absTs: '08-17 08:49:17.384',
    rawText: '[CHANNEL_X] [2026-08-17T08:12:20.873Z]: generic event',
  });

  assert.deepEqual(line.timeCandidates.map(candidate => candidate.domain), ['system', 'device']);
  assert.equal(line.timeCandidates[0].num, 1786956557384);
  assert.equal(line.timeCandidates[1].num, Date.parse('2026-08-17T08:12:20.873Z'));
});

test('structured device record with a separate clock resolves through a host line', async () => {
  const sync = await import(pathToFileURL(path.resolve(here, '../../frontend/timeSync.js')).href + `?t=${Date.now()}-${Math.random()}`);
  const imported = {
    timeCandidates: [{ domain: 'device', num: Date.parse('2026-08-17T08:12:20.873Z') }],
  };
  const hostLine = {
    timeCandidates: [
      { domain: 'system', num: Date.parse('2026-08-17T08:49:17.384Z') },
      { domain: 'device', num: Date.parse('2026-08-17T08:12:20.873Z') },
    ],
  };
  const anchor = sync.resolveSyncAnchor(imported, [[hostLine]]);
  assert.equal(anchor.domain, 'system');
  assert.equal(anchor.numTs, Date.parse('2026-08-17T08:49:17.384Z'));
});

test('device-only imported timestamp is kept in the device clock domain', async () => {
  const { buildTimestampInfo } = await importFreshState();
  const line = buildTimestampInfo('08-17 08:12:20.873', {
    absNum: Date.parse('2026-08-17T08:12:20.873Z'),
    absTs: '08-17 08:12:20.873',
    timeDomain: 'device',
    rawText: "{'timestamp': '2026-08-17T08:12:20.873Z', 'code': 'READY'}",
  });

  assert.deepEqual(line.timeCandidates.map(candidate => candidate.domain), ['device']);
});

test('clear action relative reset uses next log as T+00 origin', async () => {
  const { state, resetRelativeTimestampBase, buildTimestampInfo } = await importFreshState();
  state.timestampMode = 'relative';

  resetRelativeTimestampBase();

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
