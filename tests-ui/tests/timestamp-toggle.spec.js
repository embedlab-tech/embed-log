import { expect, test } from '@playwright/test';
import { collectPageErrors, waitForSourceTestLine } from './helpers.js';

test.describe('timestamp mode toggle', () => {
  let errors;

  test.beforeEach(async ({ page }) => {
    errors = collectPageErrors(page);
  });

  test.afterEach(async () => {
    expect(errors).toEqual([]);
  });

  test('live viewer toggles between absolute and relative timestamps', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
    await waitForSourceTestLine(page, 'SENSOR_A');

    const firstTs = page.locator('#log-SENSOR_A .log-line .ts').first();
    await expect(firstTs).toHaveText(/\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3}/);

    await page.locator('#btn-settings').click();
    await expect(page.locator('#settings-panel')).toHaveClass(/open/);
    await expect(page.locator('#btn-timestamp-mode')).toHaveText('Absolute');

    await page.locator('#btn-timestamp-mode').click();
    await expect(page.locator('#btn-timestamp-mode')).toHaveText('Relative');
    await expect(firstTs).toHaveText(/^T\+\d+:\d{2}:\d{2}\.\d{3}$/);

    await page.locator('#btn-timestamp-mode').click();
    await expect(page.locator('#btn-timestamp-mode')).toHaveText('Absolute');
    await expect(firstTs).toHaveText(/\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3}/);
  });
});
