import { expect, test } from '@playwright/test';
import { openHtmlFile, saveDownload } from './helpers.js';

const record = (sequence, source_id, message) => ({
  type: 'rx', data: message, message, source_id, source_kind: 'uart',
  timestamp: '12:00:00', sequence, session_id: 'toolbar', line_idx: sequence,
});

async function serve(page, tabs) {
  await page.routeWebSocket('**/ws', ws => {
    ws.send(JSON.stringify({
      type: 'config', session: { id: 'toolbar' }, tabs,
      pane_kinds: { A: 'uart', B: 'uart' }, merges: [{ name: 'ALL', of: ['A', 'B'] }],
    }));
    ws.send(JSON.stringify(record(1, 'A', 'alpha one')));
    ws.send(JSON.stringify(record(2, 'A', 'alpha two')));
    ws.send(JSON.stringify(record(3, 'B', 'beta')));
    ws.send(JSON.stringify({ type: 'markers_update', markers: [{ paneId: 'A', lineIdx: 0 }] }));
  });
  await page.goto('/');
}

test('toolbar total counts each line once when a merged pane repeats its sources', async ({ page }) => {
  await serve(page, [{ label: 'Sources', panes: ['A', 'B'] }, { label: 'Merged', panes: ['ALL'] }]);
  await expect(page.locator('.pane-stats[data-pane-stats="ALL"]')).toContainText('3 lines');
  await expect(page.locator('#toolbar-stats')).toHaveText('· 3 lines · 22 B');
});

test('toolbar total counts merged lines whose sources have no pane of their own', async ({ page }) => {
  await serve(page, [{ label: 'Merged', panes: ['ALL'] }]);
  await expect(page.locator('#toolbar-stats')).toHaveText('· 3 lines · 22 B');
});

test('overlapping merges count loaded copies and keep original bytes in a snapshot', async ({ page, browser }, testInfo) => {
  let send;
  await page.routeWebSocket('**/ws', ws => {
    send = msg => ws.send(JSON.stringify(msg));
    send({
      type: 'config', session: { id: 'toolbar' },
      tabs: [{ label: 'Sources', panes: ['A', 'B'] }, { label: 'AB merge', panes: ['AB'] }, { label: 'BC merge', panes: ['BC'] }],
      pane_labels: { B: 'Βήτα', C: '雪' },
      merges: [{ name: 'AB', of: ['A', 'B'] }, { name: 'BC', of: ['B', 'C'] }],
    });
    send(record(2, 'A', 'alpha'));
    send(record(3, 'B', 'βeta'));
    send(record(4, 'C', '雪cat'));
    send(record(5, 'B', 'βeta'));
  });
  await page.route('**/api/sessions/toolbar/history?*', route => {
    const params = new URL(route.request().url()).searchParams;
    expect(params.get('sources')).toBe('A,B');
    expect(params.get('before')).toBe('2');
    return route.fulfill({ body: `${JSON.stringify(record(1, 'B', 'old'))}\n` });
  });
  await page.goto('/');
  const total = page.locator('#toolbar-stats');
  await expect(total).toHaveText('· 4 lines · 21 B');

  send({ type: 'clear_logs', pane: 'B' });
  await expect(page.locator('.pane-stats[data-pane-stats="B"]')).toBeEmpty();
  await expect(total).toHaveText('· 4 lines · 21 B');
  await page.getByRole('button', { name: 'AB merge', exact: true }).click();
  await page.locator('#pane-AB .filter-input').fill('old');
  await page.locator('#pane-AB .search-mode').click();
  await expect(total).toHaveText('· 5 lines · 24 B');

  const download = page.waitForEvent('download');
  await page.evaluate(() => window.__embedLogExportSnapshot());
  const exported = await openHtmlFile(browser, await saveDownload(await download, testInfo));
  try {
    const split = exported.locator('#btn-unwrap');
    await expect(split).toHaveAttribute('aria-pressed', 'false');
    const exportedTotal = exported.locator('#toolbar-stats');
    await expect(exportedTotal).toHaveText('· 5 lines · 24 B');
    await exported.getByRole('button', { name: 'BC merge', exact: true }).click();
    await expect(exportedTotal).toHaveText('· 5 lines · 24 B');
    await split.click();
    await expect(exportedTotal).toHaveText('· 5 lines · 24 B');
    await split.click();
    await expect(exportedTotal).toHaveText('· 5 lines · 24 B');
  } finally {
    await exported.close();
  }

  send({ type: 'clear_logs', pane: 'AB' });
  await expect(total).toHaveText('· 4 lines · 21 B');
  send({ type: 'clear_logs', pane: 'BC' });
  await expect(total).toHaveText('· 1 lines · 5 B');
});

test('the toolbar wraps without clipping navigation controls or covering logs', async ({ page }) => {
  await page.setViewportSize({ width: 375, height: 600 });
  await serve(page, [{ label: 'Sources', panes: ['A', 'B'] }]);
  const toolbar = page.locator('#toolbar');
  const navigation = toolbar.getByRole('group', { name: 'Log navigation' });
  const live = navigation.getByRole('button', { name: 'Live', exact: true });
  const marker = navigation.getByRole('button', { name: 'Next marker' });
  await expect(marker).toBeVisible();
  const buttons = await toolbar.getByRole('button').all();
  for (const button of buttons) await expect(button).toBeInViewport({ ratio: 1 });
  const rows = await toolbar.getByRole('button').evaluateAll(controls =>
    new Set(controls.map(button => Math.round(button.getBoundingClientRect().top))).size);
  expect(rows).toBeGreaterThan(1);
  expect((await live.boundingBox()).y).toBe((await marker.boundingBox()).y);
  const toolbarBox = await toolbar.boundingBox();
  const log = page.locator('#log-A');
  await expect(log.getByText('alpha one')).toBeVisible();
  const logBox = await log.boundingBox();
  expect(logBox.y).toBeGreaterThanOrEqual(toolbarBox.y + toolbarBox.height);
  expect(logBox.height).toBeGreaterThan(100);
  await marker.press('Enter');
  await expect(page.locator('#marker-nav-idx')).toHaveText('1');
  await expect.poll(() => page.evaluate(async () => (await import('/state.js')).state.atBottom.A)).toBe(false);
  await live.press('Enter');
  await expect.poll(() => page.evaluate(async () => (await import('/state.js')).state.atBottom.A)).toBe(true);
});

test('split-tabs and per-pane wrap buttons show their state', async ({ page }) => {
  await serve(page, [{ label: 'Sources', panes: ['A', 'B'] }]);
  const split = page.locator('#btn-unwrap');
  await expect(split).toHaveText('Split tabs');
  await expect(split).toHaveAttribute('aria-pressed', 'false');
  await split.click();
  await expect(split).toHaveText('Split tabs');
  await expect(split).toHaveAttribute('aria-pressed', 'true');
  await expect(split).toHaveClass(/active/);
  await split.click();
  await expect(split).toHaveAttribute('aria-pressed', 'false');

  const wrap = page.locator('#pane-A .pane-wrap-btn');
  const pressed = await wrap.getAttribute('aria-pressed');
  const color = () => wrap.evaluate(el => getComputedStyle(el).color);
  const before = await color();
  await wrap.click();
  await expect(wrap).toHaveAttribute('aria-pressed', String(pressed !== 'true'));
  expect(await color()).not.toBe(before);

  const timestamp = page.locator('#btn-timestamp-mode');
  expect(await timestamp.evaluate(el => getComputedStyle(el, '::before').content)).toBe('"Time: "');
});
