import fs from 'node:fs';
import { expect, test } from '@playwright/test';
import { openHtmlFile, saveDownload, waitForRangePair } from './helpers.js';

// Review follow-ups for the large-log audit work: markers identify a line by its
// server line index, a short reconnect keeps what is loaded, and the byte counter
// counts the logged message, not the colour the server adds for display.

const SESSION = 'review';
const config = markers => ({
  type: 'config', session: { id: SESSION },
  tabs: [{ label: 'Logs', panes: ['A'] }], pane_kinds: { A: 'uart' }, markers,
});
const record = (n, extra = {}) => ({
  type: 'rx', data: `msg${n}`, message: `msg${n}`, source_id: 'A', source_kind: 'uart',
  timestamp: '12:00:00', sequence: n, session_id: SESSION, line_idx: n - 1, ...extra,
});
const marker = lineIdx => ({
  paneId: 'A', lineIdx, endIdx: lineIdx, numTs: 1, description: 'look here', createdAt: 'now',
});

// Serves each connection from a list of frames; the test can drop the socket.
async function mockServer(page, connections) {
  let current;
  let count = 0;
  await page.routeWebSocket('**/ws', ws => {
    current = ws;
    (connections[count++] || []).forEach(frame => ws.send(JSON.stringify(frame)));
  });
  return { drop: () => current.close(), count: () => count };
}

const markedRows = page => page.locator('#log-A .has-marker');

test('a marker stays on its own line after a reconnect', async ({ page }) => {
  const server = await mockServer(page, [
    [config([marker(0)]), record(1), record(2), record(3)],
    // The server only replays the recent records, not msg1.
    [config([marker(0)]), record(2), record(3)],
  ]);
  await page.goto('/');
  await expect(page.locator('#log-A')).toContainText('msg3');
  await expect(markedRows(page)).toHaveCount(1);
  await expect(markedRows(page)).toContainText('msg1');

  server.drop();
  await expect.poll(server.count, { timeout: 10_000 }).toBe(2);
  await expect(page.locator('#ws-status')).toContainText(/connected/i);
  // Nothing was dropped, and the marker did not slide to msg2.
  for (const n of [1, 2, 3]) await expect(page.locator('#log-A')).toContainText(`msg${n}`);
  await expect(markedRows(page)).toHaveCount(1);
  await expect(markedRows(page)).toContainText('msg1');
});

test('a marker is not drawn on another line while its line is not loaded', async ({ page }) => {
  // The replay starts at the second line; the marker belongs to the first.
  await mockServer(page, [[config([marker(0)]), record(2), record(3)]]);
  await page.goto('/');
  await expect(page.locator('#log-A')).toContainText('msg3');
  await expect(markedRows(page)).toHaveCount(0);
});

test('a short reconnect keeps older history, and a real gap is reported', async ({ page }) => {
  const server = await mockServer(page, [
    [config([]), record(1), record(2), record(3)],
    [config([]), record(3), record(4)], // contiguous: replay overlaps what we have
    [config([]), record(9)], // records 5..8 were missed and cannot be replayed
  ]);
  await page.goto('/');
  await expect(page.locator('#log-A')).toContainText('msg3');
  const notice = page.locator('#stream-gap');

  server.drop();
  await expect.poll(server.count, { timeout: 10_000 }).toBe(2);
  await expect(page.locator('#log-A')).toContainText('msg4');
  for (const n of [1, 2, 3, 4]) await expect(page.locator('#log-A')).toContainText(`msg${n}`);
  await expect(page.locator('[data-pane-stats="A"]')).toContainText('4 lines');
  await expect(notice).toHaveCount(0);

  server.drop();
  await expect.poll(server.count, { timeout: 10_000 }).toBe(3);
  await expect(notice).toBeVisible();
});

test('the live byte counter counts the logged message, not the added colour', async ({ page }) => {
  const colored = n => record(n, { data: `\u001b[32mmsg${n}\u001b[0m` });
  await mockServer(page, [[config([]), colored(1), colored(2), colored(3)]]);
  await page.goto('/');
  await expect(page.locator('#log-A')).toContainText('msg3');
  // Three records of four bytes each.
  await expect(page.locator('[data-pane-stats="A"]')).toContainText('3 lines · 12 B');
  await expect(page.locator('#toolbar-stats')).toContainText('3 lines · 12 B');
});

// A merged pane of A and B, where each source's first line has line_idx 0.
const mergedConfig = markers => ({
  type: 'config', session: { id: SESSION },
  tabs: [{ label: 'Sources', panes: ['A', 'B'] }, { label: 'Merged', panes: ['ALL'] }],
  pane_kinds: { A: 'uart', B: 'uart', ALL: 'merge' },
  merges: [{ name: 'ALL', of: ['A', 'B'] }],
  markers,
});
const mergedRecord = (n, source, time) => ({
  type: 'rx', data: `msg${n}`, message: `msg${n}`, source_id: source, source_kind: 'uart',
  timestamp: '12:00:00', timestamp_num: time, absNum: time, sequence: n, session_id: SESSION, line_idx: 0,
});
const mergedMarker = extra => ({
  paneId: 'ALL', lineIdx: 0, endIdx: 0, description: 'look here', createdAt: 'now', ...extra,
});

test('a merged-pane marker marks the record it names, not the row at its line index', async ({ page }) => {
  // The marker is for B's record (time 2000); A's record also has line_idx 0.
  await mockServer(page, [[
    mergedConfig([mergedMarker({ numTs: 2000 })]),
    mergedRecord(1, 'A', 1000), mergedRecord(2, 'B', 2000),
  ]]);
  await page.goto('/');
  await page.locator('#tab-bar').getByRole('button', { name: 'Merged', exact: true }).click();
  await expect(page.locator('#log-ALL')).toContainText('msg2');
  await expect(page.locator('#log-ALL .has-marker')).toHaveCount(1);
  await expect(page.locator('#log-ALL .has-marker')).toContainText('msg2');
});

test('a merged-pane marker with a sequence is matched by it, even for equal timestamps', async ({ page }) => {
  await mockServer(page, [[
    mergedConfig([mergedMarker({ numTs: 1000, sequence: 2 })]),
    mergedRecord(1, 'A', 1000), mergedRecord(2, 'B', 1000),
  ]]);
  await page.goto('/');
  await page.locator('#tab-bar').getByRole('button', { name: 'Merged', exact: true }).click();
  await expect(page.locator('#log-ALL')).toContainText('msg2');
  await expect(page.locator('#log-ALL .has-marker')).toHaveCount(1);
  await expect(page.locator('#log-ALL .has-marker')).toContainText('msg2');
});

test('exporting only B from a merged pane does not hand it the marker of A', async ({ page, browser }, testInfo) => {
  await mockServer(page, [[
    mergedConfig([mergedMarker({ numTs: 1000, sequence: 1 })]), // A: msg1 is marked
    mergedRecord(1, 'A', 1000), mergedRecord(2, 'B', 2000),
  ]]);
  await page.goto('/');
  await page.locator('#tab-bar').getByRole('button', { name: 'Merged', exact: true }).click();
  await expect(page.locator('#log-ALL .has-marker')).toContainText('msg1');
  await page.locator('#log-ALL .log-line', { hasText: 'msg2' }).click();
  const download = page.waitForEvent('download');
  await page.evaluate(() => document.getElementById('export-html-ALL').click());
  const file = await saveDownload(await download, testInfo);

  const exported = await openHtmlFile(browser, file);
  await exported.locator('#tab-bar').getByRole('button', { name: 'Merged', exact: true }).click();
  await expect(exported.locator('#log-ALL')).toContainText('msg2');
  await expect(exported.locator('#log-ALL .has-marker')).toHaveCount(0);
  await exported.close();
});

test('an older merged-pane marker whose time is shared by two sources is not drawn', async ({ page }) => {
  await mockServer(page, [[
    mergedConfig([mergedMarker({ numTs: 1000 })]), // no sequence: A and B both have time 1000
    mergedRecord(1, 'A', 1000), mergedRecord(2, 'B', 1000),
  ]]);
  await page.goto('/');
  await page.locator('#tab-bar').getByRole('button', { name: 'Merged', exact: true }).click();
  await expect(page.locator('#log-ALL')).toContainText('msg2');
  await expect(page.locator('#log-ALL .has-marker')).toHaveCount(0);
});

test('an older marker keeps its record in an export, and a tied one is not guessed', async ({ page, browser }, testInfo) => {
  // Unique time 2000 names B; time 1000 is shared. Export only A and B's tied sibling.
  await mockServer(page, [[
    mergedConfig([mergedMarker({ numTs: 2000 }), mergedMarker({ lineIdx: 1, endIdx: 1, numTs: 1000 })]),
    mergedRecord(1, 'A', 1000), mergedRecord(2, 'B', 2000), mergedRecord(3, 'B', 1000),
  ]]);
  await page.goto('/');
  await page.locator('#tab-bar').getByRole('button', { name: 'Merged', exact: true }).click();
  await expect(page.locator('#log-ALL .has-marker')).toHaveCount(1); // only the unique one: msg2
  await page.locator('#log-ALL .log-line', { hasText: 'msg1' }).click();
  const download = page.waitForEvent('download');
  await page.evaluate(() => document.getElementById('export-html-ALL').click());
  const file = await saveDownload(await download, testInfo);

  const exported = await openHtmlFile(browser, file);
  await exported.locator('#tab-bar').getByRole('button', { name: 'Merged', exact: true }).click();
  await expect(exported.locator('#log-ALL')).toContainText('msg1');
  // msg1 alone is now unique for time 1000, but the marker was tied in the session: not guessed.
  await expect(exported.locator('#log-ALL .has-marker')).toHaveCount(0);
  await exported.close();
});

test('the #marker-1 link of an export focuses the record the marker names', async ({ page, browser }, testInfo) => {
  await mockServer(page, [[
    mergedConfig([mergedMarker({ numTs: 1000, sequence: 2 })]), // B's record, same time as A's
    mergedRecord(1, 'A', 1000), mergedRecord(2, 'B', 1000),
  ]]);
  await page.goto('/');
  await page.locator('#tab-bar').getByRole('button', { name: 'Merged', exact: true }).click();
  await expect(page.locator('#log-ALL')).toContainText('msg2');
  await page.locator('#log-ALL .log-line', { hasText: 'msg1' }).click();
  await page.locator('#log-ALL .log-line', { hasText: 'msg2' }).click({ modifiers: ['Shift'] });
  const download = page.waitForEvent('download');
  await page.evaluate(() => document.getElementById('export-html-ALL').click());
  const file = await saveDownload(await download, testInfo);

  const exported = await browser.newPage();
  await exported.goto(`file://${file}#marker-1`);
  await expect(exported.locator('#log-ALL .has-marker')).toContainText('msg2');
  // The focused (sync-highlighted) row is the marked one, not msg1.
  await expect(exported.locator('#log-ALL .sync-highlight')).toContainText('msg2');
  await expect(exported.locator('#log-ALL .sync-highlight')).toHaveCount(1);
  await exported.close();
});

test('an exported selection keeps markers on their own lines', async ({ page, browser }, testInfo) => {
  // msg1 carries the marker; only msg2 and msg3 are exported.
  await mockServer(page, [[config([marker(0)]), record(1), record(2), record(3)]]);
  await page.goto('/');
  await expect(page.locator('#log-A')).toContainText('msg3');
  await page.locator('#log-A .log-line', { hasText: 'msg2' }).click();
  await page.locator('#log-A .log-line', { hasText: 'msg3' }).click({ modifiers: ['Shift'] });
  const download = page.waitForEvent('download');
  await page.evaluate(() => document.getElementById('export-html-A').click());
  const file = await saveDownload(await download, testInfo);
  expect(fs.readFileSync(file, 'utf-8')).toContain('"lineIdx":1');

  const exported = await openHtmlFile(browser, file);
  await expect(exported.locator('#log-A')).toContainText('msg2');
  // The exported page holds msg2 and msg3, not the marked msg1: nothing is marked.
  await expect(exported.locator('#log-A .has-marker')).toHaveCount(0);
  await exported.close();
});
