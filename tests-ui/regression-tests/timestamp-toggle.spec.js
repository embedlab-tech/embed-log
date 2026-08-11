import { expect, test } from '@playwright/test';
import { collectPageErrors, waitForRangePair, waitForSourceTestLine } from './helpers.js';

// Feature: timestamp mode toggle — Live viewer toggles between absolute and relative timestamp formats
//
test.describe('timestamp mode toggle', () => {
  let errors;

  test.beforeEach(async ({ page, context }) => {
    errors = collectPageErrors(page);
    await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  });

  test.afterEach(async () => {
    expect(errors).toEqual([]);
  });

// Scenario: Live viewer toggles between absolute (MM-DD HH:MM:SS.mmm) and relative (T+HH:MM:SS.mmm) timestamps
//   Given the live log viewer
//   When  the user clicks the timestamp mode toggle in settings
//   Then  timestamps switch between relative (T+HH:MM:SS.mmm) and absolute (MM-DD HH:MM:SS.mmm) formats
//
  test('live viewer toggles between absolute and relative timestamps', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
    await waitForSourceTestLine(page, 'SENSOR_A');

    const firstTs = page.locator('#log-SENSOR_A .log-line .ts').first();
    await expect(firstTs).toHaveText(/^T\+\d+:\d{2}:\d{2}\.\d{3}$/);

    await page.locator('#btn-settings').click();
    await expect(page.locator('#settings-panel')).toHaveClass(/open/);
    await expect(page.locator('#btn-timestamp-mode')).toHaveText('Relative');

    // The toggle cycles Relative → No time → Absolute → Relative.
    await page.locator('#btn-timestamp-mode').click();
    await expect(page.locator('#btn-timestamp-mode')).toHaveText('No time');
    await expect(firstTs).toBeHidden();

    await page.locator('#btn-timestamp-mode').click();
    await expect(page.locator('#btn-timestamp-mode')).toHaveText('Absolute');
    await expect(firstTs).toHaveText(/\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3}/);

    await page.locator('#btn-timestamp-mode').click();
    await expect(page.locator('#btn-timestamp-mode')).toHaveText('Relative');
    await expect(firstTs).toHaveText(/^T\+\d+:\d{2}:\d{2}\.\d{3}$/);

    await page.locator('#btn-timestamp-mode').click();
    await expect(page.locator('#btn-timestamp-mode')).toHaveText('No time');
    await expect(firstTs).toBeHidden();

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });
    await page.locator('#copy-SENSOR_A').click();
    const copied = await page.evaluate(() => navigator.clipboard.readText());
    expect(copied).toMatch(/^\[SENSOR_A\] /m);
    expect(copied).not.toMatch(/T\+\d+:\d{2}:\d{2}\.\d{3}/);

    await page.locator('#btn-timestamp-mode').click();
    await expect(page.locator('#btn-timestamp-mode')).toHaveText('Absolute');
    await expect(firstTs).toHaveText(/\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3}/);
  });
});
