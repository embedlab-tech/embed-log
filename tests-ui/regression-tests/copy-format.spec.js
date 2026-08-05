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

  test('selection copy always uses Full formatting and has no format or note actions', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });

    await expect(page.locator('.format-btn')).toHaveCount(0);
    await expect(page.getByText('Compact', { exact: true })).toHaveCount(0);
    await expect(page.getByText('Add Note', { exact: true })).toHaveCount(0);

    await page.locator('#copy-SENSOR_A').click();
    const copied = await readClipboard(page);
    expect(copied).toMatch(/\[[^\]]+\] \[SENSOR_A\]/);
    expect(copied).toContain('kind=prefix-cleanup');
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
