import { expect, test } from '@playwright/test';
import fs from 'node:fs';
import { collectPageErrors, openHtmlFile, saveDownload, waitForLineContaining, waitForRangePair, waitForSourceTestLine } from './helpers.js';

async function openMore(page, paneId) {
  await page.locator(`#more-toggle-${paneId}`).click();
}

test.describe('HTML export replay', () => {
  let errors;

  test.beforeEach(async ({ page }) => {
    errors = collectPageErrors(page);
  });

  test.afterEach(async () => {
    expect(errors).toEqual([]);
  });

  test('opens downloaded HTML snippet and replays regular pane layout', async ({ page, browser }, testInfo) => {
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
    const htmlPath = await saveDownload(download, testInfo);

    const snippet = await openHtmlFile(browser, htmlPath);
    try {
      await expect(snippet.locator('#toolbar')).toBeVisible();
      await expect(snippet.locator('#tab-bar')).toBeVisible();
      await expect(snippet.locator('#pane-SENSOR_A')).toBeVisible();
      await expect(snippet.locator('#pane-SENSOR_B')).toBeVisible();
      await expect(snippet.locator('#log-SENSOR_A')).toContainText('kind=prefix-cleanup');
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

test('repeated Export captures newer log content that arrived after first export', async ({ page }, testInfo) => {
  await page.goto('/');
  await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

  // Wait for initial data in both panes of the active tab
  await waitForSourceTestLine(page, 'SENSOR_A');
  await waitForSourceTestLine(page, 'SENSOR_B');

  // First export — should contain the initial lines
  const dl1 = page.waitForEvent('download');
  await page.locator('#btn-export').click();
  const snap1 = await saveDownload(await dl1, testInfo);
  const html1 = fs.readFileSync(snap1, 'utf-8');
  expect(html1).toContain('TEST src=SENSOR_A');
  expect(html1).toContain('TEST src=SENSOR_B');

  // Count log lines in both exports
  const dataMatch1 = html1.match(/\[SENSOR_A\].*TEST src=SENSOR_A/g);
  const count1 = dataMatch1 ? dataMatch1.length : 0;

  // Wait for enough additional ticks to accumulate more data
  await waitForLineContaining(page, 'SENSOR_A', 'kind=filter-alpha');

  // Second export — should contain all lines from the first PLUS new ones
  const dl2 = page.waitForEvent('download');
  await page.locator('#btn-export').click();
  const snap2 = await saveDownload(await dl2, testInfo);
  const html2 = fs.readFileSync(snap2, 'utf-8');

  const dataMatch2 = html2.match(/\[SENSOR_A\].*TEST src=SENSOR_A/g);
  const count2 = dataMatch2 ? dataMatch2.length : 0;

  expect(html2).toContain('TEST src=SENSOR_A');
  expect(html2).toContain('kind=filter-alpha');
  expect(count2).toBeGreaterThan(count1);
});
});
