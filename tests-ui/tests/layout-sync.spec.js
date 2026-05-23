import { expect, test } from '@playwright/test';
import { collectPageErrors, lineTick, selectedLineTicks, visiblePaneNames, waitForLineContaining, waitForSourceTestLine } from './helpers.js';

test.describe('layout and time synchronization', () => {
  let errors;

  test.beforeEach(async ({ page }) => {
    errors = collectPageErrors(page);
  });

  test.afterEach(async () => {
    expect(errors).toEqual([]);
  });

  test('demo tabs and pane order match backend config', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    await expect(page.getByRole('button', { name: 'Simulated Devices', exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Other Sensor', exact: true })).toBeVisible();

    await expect.poll(async () => visiblePaneNames(page)).toEqual(['SENSOR_A', 'SENSOR_B']);
    await expect(page.locator('#tab-content-0 .splitter')).toHaveCount(1);

    await page.getByRole('button', { name: 'Other Sensor', exact: true }).click();
    await expect.poll(async () => visiblePaneNames(page)).toEqual(['SENSOR_C']);
    await expect(page.locator('#tab-content-1 .splitter')).toHaveCount(0);
  });

  test('clicking a line sync-highlights nearest timestamp in sibling pane', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const lineA = await waitForSourceTestLine(page, 'SENSOR_A');
    const tick = await lineTick(lineA);
    await waitForLineContaining(page, 'SENSOR_B', `tick=${tick}`);

    await lineA.click();

    await expect(page.locator('#log-SENSOR_A .log-line.sync-highlight')).toContainText(`tick=${tick}`);
    await expect(page.locator('#log-SENSOR_B .log-line.sync-highlight')).toContainText(new RegExp(`tick=${tick}`));
  });

  test('Shift+Click selects a contiguous range without selecting sibling panes', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    await waitForLineContaining(page, 'SENSOR_A', 'kind=warning');
    const lines = page.locator('#log-SENSOR_A .log-line');
    await lines.nth(0).click();
    await lines.nth(3).click({ modifiers: ['Shift'] });

    await expect(page.locator('#copy-actions-SENSOR_A')).toHaveClass(/visible/);
    const ticks = await selectedLineTicks(page, 'SENSOR_A');
    expect(ticks.length).toBeGreaterThanOrEqual(4);
    await expect(page.locator('#log-SENSOR_B .log-line.selected')).toHaveCount(0);
  });

  test('per-pane wrap toggle makes long lines wrap when pane is narrowed', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    await waitForLineContaining(page, 'SENSOR_A', 'kind=warning');
    await expect(page.locator('#log-SENSOR_A .log-line').first()).toBeVisible();

    // Capture log area scroll height before wrapping (full-width, no wrap)
    const { scrollBefore } = await page.evaluate(() => {
      const log = document.getElementById('log-SENSOR_A');
      return { scrollBefore: log.scrollHeight };
    });

    // Toggle wrap ON
    await page.locator('#pane-SENSOR_A .pane-wrap-btn').click();
    await expect(page.locator('#pane-SENSOR_A .pane-wrap-btn')).toHaveClass(/active/);

    // Narrow the log area so lines must wrap
    await page.evaluate(() => {
      document.getElementById('log-SENSOR_A').style.width = '180px';
    });

    // Wait a frame for layout to settle
    await page.waitForTimeout(100);

    // Capture scroll height after wrap + narrow
    const scrollAfter = await page.evaluate(() => {
      return document.getElementById('log-SENSOR_A').scrollHeight;
    });

    // The same lines now take more vertical space
    expect(scrollAfter).toBeGreaterThan(scrollBefore);
  });

  test('per-pane wrap does not affect sibling pane', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    await waitForLineContaining(page, 'SENSOR_A', 'kind=warning');

    // Toggle wrap on SENSOR_A only
    await page.locator('#pane-SENSOR_A .pane-wrap-btn').click();

    // SENSOR_B should NOT get wrap
    await expect(page.locator('#log-SENSOR_B')).not.toHaveClass(/wrap/);
    await expect(page.locator('#pane-SENSOR_B .pane-wrap-btn')).not.toHaveClass(/active/);

    // SENSOR_A SHOULD have wrap
    await expect(page.locator('#log-SENSOR_A')).toHaveClass(/wrap/);
    await expect(page.locator('#pane-SENSOR_A .pane-wrap-btn')).toHaveClass(/active/);
  });
});
