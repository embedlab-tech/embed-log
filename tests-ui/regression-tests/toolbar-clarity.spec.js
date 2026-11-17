import { expect, test } from '@playwright/test';

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
  });
  await page.goto('/');
}

test('toolbar total counts each line once when a merged pane repeats its sources', async ({ page }) => {
  await serve(page, [{ label: 'Sources', panes: ['A', 'B'] }, { label: 'Merged', panes: ['ALL'] }]);
  await expect(page.locator('.pane-stats[data-pane-stats="ALL"]')).toContainText('3 lines');
  await expect(page.locator('#toolbar-stats')).toContainText('3 lines');
  await expect(page.locator('#toolbar-stats')).not.toContainText('6 lines');
});

test('toolbar total counts merged lines whose sources have no pane of their own', async ({ page }) => {
  await serve(page, [{ label: 'Merged', panes: ['ALL'] }]);
  await expect(page.locator('#toolbar-stats')).toContainText('3 lines');
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
