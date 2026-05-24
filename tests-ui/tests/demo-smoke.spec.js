import { expect, test } from '@playwright/test';
import fs from 'node:fs';
import { collectPageErrors, saveDownload, waitForLineContaining, waitForRangePair, waitForSourceTestLine } from './helpers.js';

async function openMore(page, paneId) {
  await page.locator(`#more-toggle-${paneId}`).click();
}

test.describe('embed-log deterministic demo smoke', () => {
  let errors;

  test.beforeEach(async ({ page }) => {
    errors = collectPageErrors(page);
  });

  test.afterEach(async () => {
    expect(errors).toEqual([]);
  });

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
    await page.locator('#download-raw-SENSOR_A').click();
    const download = await downloadPromise;

    expect(download.suggestedFilename()).toMatch(/^embed-log-exact-.*\.log$/);
    const downloadedPath = await saveDownload(download, testInfo);

    const text = fs.readFileSync(downloadedPath, 'utf-8');
    expect(text).toMatch(/\[SENSOR_A\]/);
    expect(text).toContain('kind=prefix-cleanup');
    expect(text).toContain('kind=timestamp-cleanup');
  });

  test('HTML snippet uses the regular embed-log exported UI', async ({ page }, testInfo) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });

    await openMore(page, 'SENSOR_A');
    const downloadPromise = page.waitForEvent('download');
    await page.locator('#export-html-SENSOR_A').click();
    const download = await downloadPromise;

    expect(download.suggestedFilename()).toMatch(/^embed-log-exact-.*\.html$/);
    const downloadedPath = await saveDownload(download, testInfo);

    const html = fs.readFileSync(downloadedPath, 'utf-8');
    expect(html).toContain('<div id="toolbar">');
    expect(html).toContain('<div id="tab-bar"></div>');
    expect(html).toContain('var _logData =');
    expect(html).toContain('kind=prefix-cleanup');
    expect(html).toMatch(/\[SENSOR_A\]/);
    expect(html).not.toContain('<h1>embed-log snippet</h1>');
  });

test('DOM does not grow unbounded when tailing - lines are pruned past threshold', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

  // Wait long enough for many lines to accumulate (past MAX_RENDERED of 200)
  // At ~3.5 lines/tick with 100ms tick, 200 lines = ~5.7s.  We wait 8s for margin.
  await page.waitForTimeout(8000);

  // DOM should be capped at roughly MAX_RENDERED (200) lines per pane
  const aCount = await page.locator('#log-SENSOR_A .log-line').count();
  const bCount = await page.locator('#log-SENSOR_B .log-line').count();
  expect(aCount).toBeLessThanOrEqual(210);
  expect(bCount).toBeLessThanOrEqual(210);

  // Lines should still be arriving (not stuck)
  await expect(page.locator('#log-SENSOR_A')).toContainText('TEST src=SENSOR_A');
});

test('runtime settings panel exposes working font-size controls', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
  await waitForSourceTestLine(page, 'SENSOR_A');

  await page.locator('#btn-settings').click();
  await expect(page.locator('#settings-panel')).toHaveClass(/open/);
  await expect(page.locator('#btn-font-dec')).toBeVisible();
  await expect(page.locator('#btn-font-reset')).toBeVisible();
  await expect(page.locator('#btn-font-inc')).toBeVisible();

  const line = page.locator('#log-SENSOR_A .log-line').first();
  const before = await line.evaluate(el => getComputedStyle(el).fontSize);

  await page.locator('#btn-font-inc').click();
  await expect.poll(async () => {
    return line.evaluate(el => getComputedStyle(el).fontSize);
  }).not.toBe(before);

  await page.locator('#btn-font-reset').click();
  await expect.poll(async () => {
    return line.evaluate(el => getComputedStyle(el).fontSize);
  }).toBe(before);
});

});

