import { expect, test } from '@playwright/test';
import { collectPageErrors, waitForRangePair } from './helpers.js';

async function readClipboard(page) {
  return page.evaluate(() => navigator.clipboard.readText());
}

test.describe('selection copy actions', () => {
  let errors;

  test.beforeEach(async ({ page, context }) => {
    errors = collectPageErrors(page);
    await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  });

  test.afterEach(async () => {
    expect(errors).toEqual([]);
  });

  test('selection offers full and backend-compatible compact copy without format or note actions', async ({ page }, testInfo) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });

    await expect(page.locator('.format-btn')).toHaveCount(0);
    await expect(page.getByText('Add Note', { exact: true })).toHaveCount(0);
    await expect(page.locator('#copy-SENSOR_A + #copy-compact-SENSOR_A')).toBeVisible();

    await page.locator('#copy-SENSOR_A').click();
    const full = await readClipboard(page);
    expect(full).toMatch(/\[[^\]]+\] \[SENSOR_A\]/);
    expect(full).toContain('kind=prefix-cleanup');

    await page.locator('#copy-compact-SENSOR_A').click();
    const compact = await readClipboard(page);
    expect(compact).toMatch(/^\+\d+\.\d{3} seq=\d+ src=SENSOR_A#\d+ \| /);
    expect(compact).toContain('kind=prefix-cleanup');
    expect(compact.split('\n')).toHaveLength(full.split('\n').length);

    const savedBytes = full.length - compact.length;
    testInfo.annotations.push({
      type: 'copy-size',
      description: `full=${full.length} B compact=${compact.length} B delta=${savedBytes} B`,
    });
  });

  test('Copy button shows a token count estimate for Full output', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });

    await expect(page.locator('#copy-SENSOR_A')).toHaveText(/~\d+ tok/);
  });
});
