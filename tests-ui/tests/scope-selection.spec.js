import { expect, test } from '@playwright/test';
import fs from 'node:fs';
import { collectPageErrors, saveDownload, waitForRangePair } from './helpers.js';

async function readClipboard(page) {
  return page.evaluate(() => navigator.clipboard.readText());
}

async function setScope(page, paneId, scope) {
  await page.locator(`#scope-${scope}-${paneId}`).click();
}

async function openMore(page, paneId) {
  await page.locator(`#more-toggle-${paneId}`).click();
}

test.describe('scope-aware selection actions', () => {
  let errors;

  test.beforeEach(async ({ page, context }) => {
    errors = collectPageErrors(page);
    await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  });

  test.afterEach(async () => {
    expect(errors).toEqual([]);
  });

  test('Exact mode copy does not include sibling pane content', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });

    // Default scope is exact
    await page.locator('#copy-SENSOR_A').click();
    const copied = await readClipboard(page);

    // Should contain selected pane content with source label
    expect(copied).toMatch(/\[SENSOR_A\]/);
    expect(copied).toContain('SENSOR_A');
    // Should NOT contain sibling pane content
    expect(copied).not.toContain('SENSOR_B');
  });

  test('Context mode copy includes sibling pane content', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });

    await setScope(page, 'SENSOR_A', 'context');
    await page.locator('#copy-SENSOR_A').click();
    const copied = await readClipboard(page);

    // Should contain both selected pane and sibling pane content with source labels
    expect(copied).toMatch(/\[SENSOR_A\]/);
    expect(copied).toMatch(/\[SENSOR_B\]/);
  });

  test('Scope toggle persists across panes', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    // Select in SENSOR_A and toggle to context
    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });
    await setScope(page, 'SENSOR_A', 'context');

    // Clear selection and select in SENSOR_B
    await page.keyboard.press('Escape');

    const rangeB = await waitForRangePair(page, 'SENSOR_B', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await rangeB.start.click();
    await rangeB.end.click({ modifiers: ['Shift'] });

    // Context should still be active on SENSOR_B
    await expect(page.locator('#scope-context-SENSOR_B')).toHaveClass(/active/);

    await page.locator('#copy-SENSOR_B').click();
    const copied = await readClipboard(page);
    expect(copied).toMatch(/\[SENSOR_A\]/);
    expect(copied).toMatch(/\[SENSOR_B\]/);
  });

  test('Exact download raw is single pane only', async ({ page }, testInfo) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });

    const downloadPromise = page.waitForEvent('download');
    await page.locator('#download-raw-SENSOR_A').click();
    const download = await downloadPromise;

    expect(download.suggestedFilename()).toMatch(/^embed-log-exact-.*\.log$/);
    const downloadedPath = await saveDownload(download, testInfo);
    const text = fs.readFileSync(downloadedPath, 'utf-8');

    expect(text).toMatch(/\[SENSOR_A\]/);
    expect(text).not.toContain('SENSOR_B');
  });

  test('Context download raw includes sibling panes', async ({ page }, testInfo) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });

    await setScope(page, 'SENSOR_A', 'context');

    const downloadPromise = page.waitForEvent('download');
    await page.locator('#download-raw-SENSOR_A').click();
    const download = await downloadPromise;

    expect(download.suggestedFilename()).toMatch(/^embed-log-snippet-.*\.log$/);
    const downloadedPath = await saveDownload(download, testInfo);
    const text = fs.readFileSync(downloadedPath, 'utf-8');

    expect(text).toMatch(/\[SENSOR_B\]/);
    expect(text).toMatch(/\[SENSOR_A\].*kind=prefix-cleanup/);
  });

  test('Exact HTML export contains only selected pane data', async ({ page }, testInfo) => {
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

    expect(html).toContain('var _logData =');
    expect(html).toContain('kind=prefix-cleanup');
    const dataMatch = html.match(/"SENSOR_A":\[/);
    expect(dataMatch).toBeTruthy();
  });

  test('Context mode add to clipboard includes sibling content', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });

    await setScope(page, 'SENSOR_A', 'context');
    await openMore(page, 'SENSOR_A');
    await page.locator('#clip-add-SENSOR_A').click();

    await expect(page.locator('#clip-indicator')).toBeVisible();
    await page.locator('#clip-peek-btn').click();
    const body = page.locator('#clip-peek-menu .clip-peek-body');
    await expect(body).toContainText('SENSOR_A');
    await expect(body).toContainText('SENSOR_B');
  });
});
