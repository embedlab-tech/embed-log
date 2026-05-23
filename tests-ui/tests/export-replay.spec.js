import { expect, test } from '@playwright/test';
import fs from 'node:fs';
import { openHtmlFile, saveDownload, waitForLineContaining, waitForRangePair, waitForSourceTestLine } from './helpers.js';

test.describe('HTML export replay', () => {
  test('opens downloaded HTML snippet and replays regular pane layout', async ({ page, browser }, testInfo) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });

    const downloadPromise = page.waitForEvent('download');
    await page.locator('#download-range-html-SENSOR_A').click();
    const download = await downloadPromise;
    const htmlPath = await saveDownload(download, testInfo);

    const snippet = await openHtmlFile(browser, htmlPath);
    try {
      await expect(snippet.locator('#toolbar')).toBeVisible();
      await expect(snippet.locator('#tab-bar')).toBeVisible();
      await expect(snippet.locator('#pane-SENSOR_A')).toBeVisible();
      await expect(snippet.locator('#pane-SENSOR_B')).toBeVisible();
      await expect(snippet.locator('#log-SENSOR_A')).toContainText('kind=prefix-cleanup');
      await expect(snippet.locator('#log-SENSOR_A')).toContainText('kind=timestamp-cleanup');
      await expect(snippet.locator('#ws-status')).toBeHidden();
    } finally {
      await snippet.close();
    }
  });

  test('full toolbar Export opens as a static snapshot with deterministic logs', async ({ page, browser }, testInfo) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
    await waitForSourceTestLine(page, 'SENSOR_A');
    await waitForSourceTestLine(page, 'SENSOR_B');
    await waitForLineContaining(page, 'SENSOR_A', 'kind=filter-alpha');

    const downloadPromise = page.waitForEvent('download');
    await page.locator('#btn-export').click();
    const download = await downloadPromise;
    expect(download.suggestedFilename()).toMatch(/^embed-log-.*\.html$/);
    const htmlPath = await saveDownload(download, testInfo);

    const html = fs.readFileSync(htmlPath, 'utf-8');
    expect(html).toContain('var _logData =');
    expect(html).toContain('kind=filter-alpha');

    const exported = await openHtmlFile(browser, htmlPath);
    try {
      await expect(exported.locator('#toolbar')).toBeVisible();
      await expect(exported.locator('#pane-SENSOR_A')).toBeVisible();
      await expect(exported.locator('#pane-SENSOR_B')).toBeVisible();
      await expect(exported.locator('#log-SENSOR_A')).toContainText('kind=filter-alpha');
      await exported.getByRole('button', { name: 'Other Sensor', exact: true }).click();
      await expect(exported.locator('#pane-SENSOR_C')).toBeVisible();
    } finally {
      await exported.close();
    }
  });
});
