import { expect, test } from '@playwright/test';

// A regex filter must not rescan the whole pane for every live batch, and
// the incrementally extended result must equal a full rescan.

async function setup(page, panes = ['A']) {
  let socket;
  await page.setViewportSize({ width: 1200, height: 800 });
  await page.routeWebSocket('**/ws', ws => {
    socket = ws;
    ws.send(JSON.stringify({
      type: 'config', session: { id: 'live-filter' },
      tabs: [{ label: 'Logs', panes }], pane_kinds: { A: 'uart', B: 'uart' },
      source_levels: { A: null, B: null },
    }));
  });
  await page.goto('/');
  await expect(page.locator('.filter-input[data-pane="A"]')).toBeVisible();
  return socket;
}

test('live batches with an active filter extend the projection and match a full rescan', async ({ page }) => {
  test.setTimeout(120000);
  await setup(page);
  const N = 200000;
  await page.evaluate(async N => {
    const { appendLineBatch } = await import('/lines.js');
    for (let b = 0; b < N; b += 2000) {
      const rows = [];
      for (let i = b; i < b + 2000; i++) rows.push({ paneId: 'A', ts: '11-16 12:00:00.000', rawText: `packet ${i} len=${i % 1500}`, isTx: false, meta: { sequence: i + 1, absTs: '11-16 12:00:00.000', relTs: 'T+00:00:00.000', absNum: 1e12 + i, relNum: i } });
      appendLineBatch(rows);
    }
  }, N);
  await page.locator('.filter-input[data-pane="A"]').fill('len=7(?!\\d)');
  await page.locator('.filter-input[data-pane="A"]').press('Enter');
  await expect(page.locator('#log-A .log-line').first()).toContainText('len=7');

  const perBatch = await page.evaluate(async N => {
    const { appendLineBatch } = await import('/lines.js');
    const started = performance.now();
    for (let b = 0; b < 20; b++) {
      const rows = [];
      for (let i = 0; i < 100; i++) {
        const n = N + b * 100 + i;
        rows.push({ paneId: 'A', ts: '11-16 12:00:01.000', rawText: `live ${n} len=${i % 2 ? 7 : 8}`, isTx: false, meta: { sequence: n + 1, absTs: '11-16 12:00:01.000', relTs: 'T+00:00:01.000', absNum: 1e12 + n, relNum: n } });
      }
      appendLineBatch(rows);
    }
    return (performance.now() - started) / 20;
  }, N);
  // A full rescan of 200k rows per batch took ~35 ms locally.
  expect(perBatch).toBeLessThan(20);

  const projection = await page.evaluate(async () => {
    const { state } = await import('/state.js');
    const { getLine, matchesFilter } = await import('/lines.js');
    const lines = state.rawLines.A;
    let expected = 0;
    for (let i = 0; i < lines.length; i++) if (matchesFilter(getLine('A', i), state.filters.A, 'A')) expected++;
    const rendered = [...document.querySelectorAll('#log-A .log-line')].map(el => el.textContent);
    return { expected, rendered };
  });
  // 133 matches in the history (len=7 every 1500 rows) + 1000 odd live rows.
  expect(projection.expected).toBe(Math.ceil((N - 7) / 1500) + 1000);
  expect(projection.rendered.every(text => /len=7(?!\d)/.test(text))).toBe(true);
  // Applying a filter jumps to the first match, so follow the tail explicitly.
  await page.evaluate(async () => (await import('/lines.js')).scrollPaneToBottom('A'));
  await expect(page.locator('#log-A .log-line').last()).toContainText(`live ${N + 1999} len=7`);
  const spacer = await page.evaluate(() => parseFloat(document.querySelector('#log-A .log-spacer').style.height));
  const rowH = await page.evaluate(() => document.querySelector('#log-A .log-line').offsetHeight);
  expect(Math.round(spacer / rowH)).toBeGreaterThanOrEqual(projection.expected);
});

test('changing the timestamp mode re-evaluates a filter that matches timestamps', async ({ page }) => {
  await setup(page);
  await page.evaluate(async () => {
    const { appendLineBatch } = await import('/lines.js');
    appendLineBatch([0, 1, 2].map(i => ({ paneId: 'A', ts: '11-16 12:00:00.000', rawText: `row ${i}`, isTx: false,
      meta: { sequence: i + 1, absTs: '11-16 12:00:00.000', relTs: 'T+00:00:0' + i + '.000', absNum: 1e12 + i, relNum: i * 1000 } })));
  });
  const input = page.locator('.filter-input[data-pane="A"]');
  await input.fill('T\\+00:00:01');
  await input.press('Enter');
  await expect(page.locator('#log-A .log-line')).toHaveCount(0);
  await page.evaluate(async () => (await import('/lines.js')).setTimestampMode('relative'));
  await expect(page.locator('#log-A .log-line')).toHaveCount(1);
  await expect(page.locator('#log-A .log-line')).toContainText('row 1');
});

test('late session metadata re-evaluates existing timestamp matches', async ({ page }) => {
  const socket = await setup(page);
  await page.evaluate(async () => {
    const { appendLineBatch } = await import('/lines.js');
    appendLineBatch([{ paneId: 'A', ts: 'T+00:00:00.000', rawText: 'early row', isTx: false,
      meta: { sequence: 1, relNum: 0 } }]);
  });
  const input = page.locator('.filter-input[data-pane="A"]');
  await input.fill('^early row early row \\d{2}-\\d{2} ');
  await input.press('Enter');
  await expect(page.locator('#log-A .log-line')).toHaveCount(0);
  socket.send(JSON.stringify({ type: 'session_info', session: { first_log_at: '2026-11-16T12:00:00Z' } }));
  await expect(page.locator('#log-A .log-line')).toHaveText(/early row/);
});

test('reanalyzing one pane does not rescan another pane on live append', async ({ page }) => {
  await setup(page, ['A', 'B']);
  const checks = await page.evaluate(async () => {
    const { state } = await import('/state.js');
    const { appendLineBatch, reanalyzePanePlugins, rerenderPane } = await import('/lines.js');
    const row = i => ({ paneId: 'B', ts: '', rawText: `row ${i}`, isTx: false });
    appendLineBatch(Array.from({ length: 1000 }, (_, i) => row(i)));
    let checks = 0;
    const filter = /row 0\b/;
    filter.test = text => { checks++; return RegExp.prototype.test.call(filter, text); };
    state.filters.B = filter;
    rerenderPane('B');
    checks = 0;
    reanalyzePanePlugins('A');
    appendLineBatch([row(1000)]);
    return checks;
  });
  expect(checks).toBeLessThan(100);
  await expect(page.locator('#log-B .log-line')).toContainText('row 0');
});
