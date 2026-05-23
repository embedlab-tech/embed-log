import { expect, test } from '@playwright/test';
import fs from 'node:fs';
import { saveDownload, waitForLineContaining, waitForRangePair, waitForSourceTestLine } from './helpers.js';

test.describe('embed-log deterministic demo smoke', () => {
  test('connects to backend and receives deterministic demo logs', async ({ page }) => {
    await page.goto('/');

    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
    await expect(page.locator('#pane-SENSOR_A')).toBeVisible();
    await expect(page.locator('#pane-SENSOR_B')).toBeVisible();
    await expect(page.locator('#pane-SENSOR_C')).toBeAttached();

    await waitForSourceTestLine(page, 'SENSOR_A');
    await waitForSourceTestLine(page, 'SENSOR_B');

    await page.getByRole('button', { name: 'Other Sensor', exact: true }).click();
    await waitForSourceTestLine(page, 'SENSOR_C');
  });

  test('shift-click selects a deterministic range and raw snippet downloads cleaned merged text', async ({ page }, testInfo) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });

    await expect.poll(async () => page.locator('#log-SENSOR_A .log-line.selected').count())
      .toBeGreaterThanOrEqual(2);
    await expect(page.locator('#copy-actions-SENSOR_A')).toHaveClass(/visible/);

    const downloadPromise = page.waitForEvent('download');
    await page.locator('#download-range-raw-SENSOR_A').click();
    const download = await downloadPromise;

    expect(download.suggestedFilename()).toMatch(/^embed-log-snippet-.*\.log$/);
    const downloadedPath = await saveDownload(download, testInfo);

    const text = fs.readFileSync(downloadedPath, 'utf-8');
    expect(text).toContain('[SENSOR_A]');
    expect(text).toContain('kind=prefix-cleanup');
    expect(text).toContain('kind=timestamp-cleanup');
    expect(text).not.toMatch(/\[SENSOR_A\]\s+\[SENSOR_A\]/);
    expect(text).not.toMatch(/\[SENSOR_A\]\s+\[\d{4}-\d{2}-\d{2}T/);
  });

  test('HTML snippet uses the regular embed-log exported UI', async ({ page }, testInfo) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });

    const downloadPromise = page.waitForEvent('download');
    await page.locator('#download-range-html-SENSOR_A').click();
    const download = await downloadPromise;

    expect(download.suggestedFilename()).toMatch(/^embed-log-snippet-.*\.html$/);
    const downloadedPath = await saveDownload(download, testInfo);

    const html = fs.readFileSync(downloadedPath, 'utf-8');
    expect(html).toContain('<div id="toolbar">');
    expect(html).toContain('<div id="tab-bar"></div>');
    expect(html).toContain('var _logData =');
    expect(html).toContain('kind=prefix-cleanup');
    expect(html).not.toContain('<h1>embed-log snippet</h1>');
  });
});
