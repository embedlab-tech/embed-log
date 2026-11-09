import { expect, test } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { collectPageErrors, waitForSourceTestLine } from './helpers.js';

const config = session => ({
  type: 'config', session: { id: session },
  tabs: [{ label: 'Sources', panes: ['A', 'B'] }, { label: 'Merged', panes: ['ALL'] }],
  pane_labels: { A: 'ALPHA', B: 'BETA' }, pane_kinds: { A: 'uart', B: 'uart' },
  merges: [{ name: 'ALL', of: ['A', 'B'] }],
});
const record = (sequence, message, source_id = 'A') => ({
  sequence, message, source_id, source_kind: 'uart', type: 'rx',
  timestamp: '12:00:00', timestamp_num: sequence, line_idx: sequence,
});

async function highlight(page, pane, phrase) {
  await page.locator(`#pane-${pane} .search-mode`).click();
  await page.locator(`.filter-input[data-pane="${pane}"]`).fill(phrase);
}

for (const legacyCache of [false, true]) {
  test(`highlight loads history after reload with ${legacyCache ? 'legacy' : 'current'} cache`, async ({ page }) => {
    const session = 'cached-history';
    const rows = [record(1, 'needle'), record(2, 'needle needle')];
    await page.route('**/sessions/cached-history/combined.jsonl', route => route.fulfill({
      contentType: 'application/x-ndjson', body: rows.map(JSON.stringify).join('\n') + '\n',
    }));
    await page.routeWebSocket('**/ws', ws => {
      ws.send(JSON.stringify(config(session)));
      for (const row of rows) ws.send(JSON.stringify({ ...row, data: row.message, session_id: session }));
    });
    await page.goto('/');
    await expect.poll(() => page.evaluate(async () => (await import('/state.js')).state.rawLines.A.length)).toBe(2);
    await page.evaluate(() => window.dispatchEvent(new Event('beforeunload')));
    const cached = await page.evaluate(() => JSON.parse(localStorage.getItem('embed-log:session:cached-history:v1')));
    expect(cached.lines.A.map(row => row.sequence)).toEqual([1, 2]);
    expect(cached.lines.A.every(row => row.sessionId === session)).toBe(true);
    if (legacyCache) {
      await page.addInitScript(() => {
        const key = 'embed-log:session:cached-history:v1';
        const snapshot = JSON.parse(localStorage.getItem(key));
        for (const rows of Object.values(snapshot.lines)) {
          for (const row of rows) {
            delete row.sequence;
            delete row.sessionId;
            delete row.lineIdx;
          }
        }
        localStorage.setItem(key, JSON.stringify(snapshot));
      });
    }
    await page.reload();
    await expect(page.locator('#pane-A')).toBeVisible();
    await highlight(page, 'A', 'needle');
    await expect(page.locator('#pane-A .search-count')).toHaveText('0 / 3');
    await expect.poll(() => page.evaluate(async () => (await import('/state.js')).state.rawLines.A.length)).toBe(2);
    await page.locator('#pane-A .search-next').click();
    await expect(page.locator('#pane-A .search-count')).toHaveText('1 / 3');
  });
}

test('repeated pane setup keeps one highlight toggle with counter and navigation', async ({ page }) => {
  const rows = [record(1, 'needle needle'), record(2, 'context'), record(3, 'needle')];
  await page.route('**/sessions/repeated-setup/combined.jsonl', route => route.fulfill({
    contentType: 'application/x-ndjson', body: rows.map(JSON.stringify).join('\n') + '\n',
  }));
  await page.routeWebSocket('**/ws', ws => ws.send(JSON.stringify(config('repeated-setup'))));
  await page.goto('/');
  await expect(page.locator('#pane-A')).toBeVisible();
  await page.evaluate(async () => {
    const { _uiSetupPane } = await import('/ui.js');
    _uiSetupPane('A');
    _uiSetupPane('A');
  });
  await expect(page.locator('#pane-A .search-mode')).toHaveCount(1);
  await expect(page.locator('#pane-A .search-controls')).toHaveCount(1);
  await highlight(page, 'A', 'needle');
  await expect(page.locator('#pane-A .search-count')).toHaveText('0 / 3');
  await page.locator('#pane-A .search-next').click();
  await expect(page.locator('#pane-A .search-count')).toHaveText('1 / 3');
  await page.locator('#pane-A .search-prev').click();
  await expect(page.locator('#pane-A .search-count')).toHaveText('3 / 3');
});

test('session highlight loads history beyond replay, counts occurrences and navigates virtual rows without hiding context', async ({ page }) => {
  const errors = collectPageErrors(page);
  let socket;
  const rows = Array.from({ length: 6000 }, (_, i) => record(i + 1,
    i === 5 ? '\x1b[31mNee\x1b[0mdle NEEDLE' : i === 5500 ? 'needle' : `context ${i}`));
  const journal = [...rows, record(6001, 'needle', 'B')].map(JSON.stringify).join('\n') + '\n';
  let requests = 0;
  await page.route('**/sessions/session-one/combined.jsonl', route => {
    requests++;
    return route.fulfill({ contentType: 'application/x-ndjson', body: journal + '{"unfinished":' });
  });
  await page.routeWebSocket('**/ws', ws => { socket = ws; ws.send(JSON.stringify(config('session-one'))); });
  await page.goto('/');
  await expect(page.locator('#pane-A')).toBeVisible();
  for (const row of rows.slice(5000)) socket.send(JSON.stringify({ ...row, data: row.message, session_id: 'session-one' }));
  socket.send(JSON.stringify({ ...record(6001, 'needle', 'B'), data: 'needle', session_id: 'session-one' }));
  await expect.poll(() => page.evaluate(async () => (await import('/state.js')).state.rawLines.A.length)).toBe(1000);
  await page.locator('#pane-A .search-mode').click();
  await page.evaluate(async () => { (await import('/lines.js')).ensureLineVisible('A', 3); });
  await page.locator('#log-A [data-idx="3"]').click();
  await page.evaluate(() => {
    const input = document.querySelector('.filter-input[data-pane="A"]');
    input.value = 'needle';
    input.dispatchEvent(new Event('input'));
  });
  await expect(page.locator('#pane-A .search-count')).toHaveText('0 / 3');
  expect(requests).toBe(1);
  expect(await page.evaluate(async () => [...(await import('/state.js')).state.selected.A])).toEqual([5003]);
  await page.locator('#log-A [data-idx="5004"]').click({ modifiers: ['Shift'] });
  expect(await page.evaluate(async () => [...(await import('/state.js')).state.selected.A])).toEqual([5003, 5004]);
  await page.locator('.filter-input[data-pane="A"]').press('Enter');
  await expect(page.locator('#pane-A .search-count')).toHaveText('1 / 3');
  await expect(page.locator('#log-A .search-current').first()).toHaveText('Nee');
  await expect(page.locator('#log-A [data-idx="4"]')).toContainText('context 4');
  await expect(page.locator('#log-A [data-idx="6"]')).toContainText('context 6');
  await page.locator('#log-A [data-idx="5"] .search-current').first().click({ button: 'middle' });
  await expect(page.locator('.filter-input[data-pane="A"]')).toHaveValue('needle');
  await expect(page.locator('#pane-A .search-mode')).toHaveAttribute('aria-pressed', 'true');
  await expect(page.locator('#log-A [data-idx="5"]')).toHaveClass(/sync-highlight/);
  await page.locator('.filter-input[data-pane="A"]').press('Shift+Enter');
  await expect(page.locator('#pane-A .search-count')).toHaveText('3 / 3');
  await expect(page.locator('#log-A [data-idx="5500"] .search-current')).toHaveText('needle');
  expect(await page.locator('#log-A .log-line').count()).toBeLessThan(250);
  await page.locator('#pane-A .search-next').click();
  await expect(page.locator('#pane-A .search-count')).toHaveText('1 / 3');
  socket.send(JSON.stringify({ ...record(6002, 'needle needle'), data: 'needle needle', session_id: 'session-one' }));
  await expect(page.locator('#pane-A .search-count')).toHaveText('1 / 5');
  // Replaying a record already loaded from the journal cannot duplicate it.
  socket.send(JSON.stringify({ ...record(6002, 'needle needle'), data: 'needle needle', session_id: 'session-one' }));
  await expect.poll(() => page.evaluate(async () => (await import('/state.js')).state.rawLines.A.length)).toBe(6001);
  await page.locator('#pane-A .search-mode').click();
  await page.locator('.filter-input[data-pane="A"]').fill('context (20|21)\\b');
  await expect.poll(() => page.locator('#log-A .log-line').count()).toBe(2);
  await page.locator('.filter-input[data-pane="A"]').fill('[');
  await expect(page.locator('.filter-input[data-pane="A"]')).toHaveClass(/invalid/);
  await page.locator('.filter-input[data-pane="A"]').fill('');
  await page.locator('#tab-bar .tab-btn[data-tab-idx="1"]').click();
  await highlight(page, 'ALL', 'needle');
  await expect(page.locator('#pane-ALL .search-count')).toHaveText('0 / 6');
  // A config-only session switch must invalidate the old pane's full-history cache too.
  await page.route('**/sessions/session-two/combined.jsonl', route => route.fulfill({
    body: JSON.stringify(record(1, 'fresh session')) + '\n',
  }));
  socket.send(JSON.stringify(config('session-two')));
  await expect(page.locator('#pane-A')).toBeVisible();
  await highlight(page, 'A', 'fresh');
  await expect(page.locator('#pane-A .search-count')).toHaveText('0 / 1');
  expect(errors).toEqual([]);
});

test('highlight reads the real running session journal', async ({ page }) => {
  const errors = collectPageErrors(page);
  await page.goto('/');
  await waitForSourceTestLine(page, 'SENSOR_A');
  const response = page.waitForResponse(response => response.url().endsWith('/combined.jsonl'));
  await highlight(page, 'SENSOR_A', 'TEST src=SENSOR_A');
  expect((await response).status()).toBe(200);
  await expect(page.locator('#pane-SENSOR_A .search-count')).toHaveText(/^0 \/ [1-9]\d*$/);
  await page.locator('.filter-input[data-pane="SENSOR_A"]').press('Enter');
  await expect(page.locator('#log-SENSOR_A .search-current')).toHaveText('TEST src=SENSOR_A');
  expect(errors).toEqual([]);
});

test('history failures are explicit and stale fetches cannot leak records into a rotated session', async ({ page }) => {
  let socket;
  let finish, finishNew;
  await page.route('**/sessions/session-one/combined.jsonl', async route => {
    await new Promise(resolve => { finish = resolve; });
    await route.fulfill({ body: JSON.stringify(record(1, 'old needle')) + '\n' });
  });
  await page.route('**/sessions/session-two/combined.jsonl', async route => {
    await new Promise(resolve => { finishNew = resolve; });
    await route.fulfill({ status: 503, body: 'unavailable' });
  });
  await page.routeWebSocket('**/ws', ws => { socket = ws; ws.send(JSON.stringify(config('session-one'))); });
  await page.goto('/');
  await expect(page.locator('#pane-A')).toBeVisible();
  await highlight(page, 'A', 'needle');
  await expect(page.locator('#pane-A .search-count')).toHaveText('Loading history…');
  await expect.poll(() => typeof finish).toBe('function');
  socket.send(JSON.stringify({ type: 'session_rotated', session: { id: 'session-two' } }));
  socket.send(JSON.stringify(config('session-two')));
  finish();
  await expect.poll(() => typeof finishNew).toBe('function');
  await expect(page.locator('#pane-A .search-count')).toHaveText('Loading history…');
  finishNew();
  await expect(page.locator('#pane-A .search-count')).toHaveText('History unavailable');
  expect(await page.evaluate(async () => (await import('/state.js')).state.rawLines.A.length)).toBe(0);
  await expect(page.locator('#pane-A .search-next')).toBeDisabled();
});

test('recorded HTML and a re-exported HTML snapshot keep safe literal highlighting and keyboard navigation offline', async ({ page, browser }, testInfo) => {
  test.setTimeout(120_000);
  const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
  const logs = testInfo.outputPath('sessions');
  const dir = path.join(logs, 'highlight');
  fs.mkdirSync(dir, { recursive: true });
  const source = path.join(dir, 'A.log');
  fs.writeFileSync(source, Array.from({ length: 3000 }, (_, i) =>
    `[2026-01-01 12:00:00.000] ${i === 3 || i === 2900 ? 'x <script> & a.b ' : ''}context ${i}`).join('\n') + '\n');
  fs.writeFileSync(path.join(dir, 'manifest.json'), JSON.stringify({
    session_id: 'highlight', session_dir: dir, timestamp_mode: 'absolute',
    tabs: [{ label: 'Logs', panes: ['A'] }], pane_labels: { A: 'ALPHA' }, source_files: { A: source },
    combined_file: path.join(dir, 'combined.jsonl'),
  }));
  const html = testInfo.outputPath('highlight.html');
  execFileSync('cargo', ['run', '--quiet', '-p', 'embed-log-cli', '--bin', 'embed-log', '--',
    'sessions', 'export', 'highlight', '--dir', logs, '--format', 'html', '--output', html], { cwd: repo });
  const errors = collectPageErrors(page);
  await page.goto(pathToFileURL(html).href);
  await highlight(page, 'A', '<script> & a.b');
  await expect(page.locator('#pane-A .search-count')).toHaveText('0 / 2');
  await page.locator('.filter-input').press('Shift+Enter');
  await expect(page.locator('#pane-A .search-count')).toHaveText('2 / 2');
  await expect(page.locator('#log-A [data-idx="2900"] .search-current')).toHaveText('<script> & a.b');
  // Existing snapshot export fetches assets; supply that same contract for file:// without network access.
  const assets = Object.fromEntries(fs.readdirSync(path.join(repo, 'frontend')).filter(name => /\.(js|css)$/.test(name))
    .map(name => [name, fs.readFileSync(path.join(repo, 'frontend', name), 'utf8')]));
  await page.evaluate(assets => { window.fetch = async name => new Response(assets[name]); }, assets);
  const download = page.waitForEvent('download');
  await page.evaluate(() => window.__embedLogExportSnapshot({ button: null }));
  const snapshot = testInfo.outputPath('snapshot.html');
  // The existing browser snapshot exporter uses relative font URLs.
  fs.cpSync(path.join(repo, 'frontend/fonts'), testInfo.outputPath('fonts'), { recursive: true });
  await (await download).saveAs(snapshot);
  const replay = await browser.newPage();
  const replayErrors = collectPageErrors(replay);
  try {
    await replay.goto(pathToFileURL(snapshot).href);
    await highlight(replay, 'A', 'a.b');
    await replay.locator('.filter-input').press('Enter');
    await expect(replay.locator('#pane-A .search-count')).toHaveText('1 / 2');
    await expect(replay.locator('#log-A .search-current')).toHaveText('a.b');
    await replay.locator('#pane-A input[type="file"]').setInputFiles({
      name: 'local.log', mimeType: 'text/plain', buffer: Buffer.from(
        '[2026-01-01T12:00:00.000Z] needle needle\n[2026-01-01T12:00:01.000Z] surrounding context\n'),
    });
    await expect(replay.locator('#log-A')).toContainText('surrounding context');
    await replay.locator('.filter-input').fill('needle');
    await expect(replay.locator('#pane-A .search-count')).toHaveText('0 / 2');
    expect(replayErrors).toEqual([]);
  } finally { await replay.close(); }
  expect(errors).toEqual([]);
});
