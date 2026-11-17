import { expect, test } from '@playwright/test';

// Filtering and highlighting scan the whole pane, so typing applies them once
// it pauses; Enter applies immediately.

async function setup(page) {
  await page.routeWebSocket('**/ws', ws => ws.send(JSON.stringify({
    type: 'config', session: { id: 'debounce' }, tabs: [{ label: 'Logs', panes: ['A'] }], pane_kinds: { A: 'uart' },
  })));
  await page.goto('/');
  await expect(page.locator('#ws-status')).toContainText(/connected/i);
  await page.evaluate(async () => {
    const { appendLineBatch } = await import('/lines.js');
    appendLineBatch(['packet 1', 'packet 2', 'other'].map((rawText, i) => ({ paneId: 'A', ts: '12:00:00', rawText, isTx: false, meta: { sequence: i + 1 } })));
    // Count how often the filter for pane A is (re)applied.
    const { state } = await import('/state.js');
    window.__filterApplies = 0;
    let value = state.filters.A ?? null;
    Object.defineProperty(state.filters, 'A', {
      configurable: true, enumerable: true,
      get: () => value,
      set: next => { window.__filterApplies++; value = next; },
    });
  });
}

test('typing a filter applies it once after a pause', async ({ page }) => {
  await setup(page);
  await page.locator('.filter-input[data-pane="A"]').pressSequentially('packet', { delay: 20 });
  await expect(page.locator('#log-A .log-line')).toHaveCount(2);
  await page.waitForTimeout(300);
  expect(await page.evaluate(() => window.__filterApplies)).toBe(1);
});

test('Enter applies the filter without waiting', async ({ page }) => {
  await setup(page);
  const input = page.locator('.filter-input[data-pane="A"]');
  await input.evaluate(el => {
    el.value = 'other';
    el.dispatchEvent(new Event('input'));
    el.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
  });
  // Applied synchronously by Enter, before the debounce timer fires.
  expect(await page.evaluate(() => window.__filterApplies)).toBe(1);
  await expect(page.locator('#log-A .log-line')).toHaveCount(1);
  await page.waitForTimeout(300);
  expect(await page.evaluate(() => window.__filterApplies)).toBe(1);
});
