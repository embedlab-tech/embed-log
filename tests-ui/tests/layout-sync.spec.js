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
test('UNWRAP toggle creates one tab per pane with pane names as labels', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
  await waitForSourceTestLine(page, 'SENSOR_A');

  // Before unwrap: tabs are group labels
  await expect(page.getByRole('button', { name: 'Simulated Devices', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Other Sensor', exact: true })).toBeVisible();

  // Click UNWRAP
  await page.locator('#btn-unwrap').click();
  await expect(page.locator('#btn-unwrap')).toHaveClass(/active/);

  // Now tabs are pane names
  await page.getByRole('button', { name: 'SENSOR_A', exact: true }).click();
  await page.getByRole('button', { name: 'SENSOR_B', exact: true }).click();
  await page.getByRole('button', { name: 'SENSOR_C', exact: true }).click();

  // Verify no "+" button in unwrap mode
  await expect(page.locator('#tab-bar .tab-add')).toHaveCount(0);
});

test('UNWRAP preserves log content across toggle', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
  await waitForSourceTestLine(page, 'SENSOR_A');

  // Click UNWRAP — verify logs still visible
  await page.locator('#btn-unwrap').click();
  await expect(page.locator('#log-SENSOR_A .log-line').first()).toBeVisible();
  await expect(page.locator('#log-SENSOR_A')).toContainText('TEST src=SENSOR_A');

  // Toggle back to grouped — verify logs still visible
  await page.locator('#btn-unwrap').click();
  await expect(page.locator('#btn-unwrap')).not.toHaveClass(/active/);
  await expect(page.locator('#log-SENSOR_A').first()).toBeVisible();
  await expect(page.locator('#log-SENSOR_A')).toContainText('TEST src=SENSOR_A');
});

test('UNWRAP mode shows full-width single pane and all panes exist', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
  await waitForSourceTestLine(page, 'SENSOR_A');
  await waitForSourceTestLine(page, 'SENSOR_B');

  await page.locator('#btn-unwrap').click();

  // SENSOR_A and SENSOR_B are on separate tabs now
  await page.getByRole('button', { name: 'SENSOR_A', exact: true }).click();
  await expect(page.locator('#pane-SENSOR_A')).toBeVisible();
  await expect(page.locator('#log-SENSOR_A')).toContainText('TEST src=SENSOR_A');

  await page.getByRole('button', { name: 'SENSOR_B', exact: true }).click();
  await expect(page.locator('#pane-SENSOR_B')).toBeVisible();
  await expect(page.locator('#log-SENSOR_B')).toContainText('TEST src=SENSOR_B');

  // Verify no splitters in unwrap mode (single pane per tab)
  await expect(page.locator('#tab-content-0 .splitter')).toHaveCount(0);
});
test('UNWRAP preserves the currently visible pane when toggled from another tab', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

  await page.getByRole('button', { name: 'Other Sensor', exact: true }).click();
  await waitForSourceTestLine(page, 'SENSOR_C');
  await expect.poll(async () => visiblePaneNames(page)).toEqual(['SENSOR_C']);

  await page.locator('#btn-unwrap').click();
  await expect(page.locator('#btn-unwrap')).toHaveClass(/active/);
  await expect.poll(async () => visiblePaneNames(page)).toEqual(['SENSOR_C']);

  const lineC = await waitForSourceTestLine(page, 'SENSOR_C');
  await lineC.click();
  await expect(page.locator('#log-SENSOR_C .log-line.sync-highlight')).toContainText('TEST src=SENSOR_C');
});

test('pane headers keep only Wrap controls after layout creation and rebuild', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
  await waitForSourceTestLine(page, 'SENSOR_A');

  await expect(page.locator('#pane-SENSOR_A .pane-wrap-btn')).toBeVisible();
  await expect(page.locator('#pane-SENSOR_A .import-btn')).toHaveCount(0);
  await expect(page.locator('#pane-SENSOR_A input[type="file"]')).toHaveCount(0);

  await page.locator('#btn-unwrap').click();
  await page.getByRole('button', { name: 'SENSOR_C', exact: true }).click();
  await expect(page.locator('#pane-SENSOR_C .pane-wrap-btn')).toBeVisible();
  await expect(page.locator('#pane-SENSOR_C .import-btn')).toHaveCount(0);
  await expect(page.locator('#pane-SENSOR_C input[type="file"]')).toHaveCount(0);

  await page.locator('#btn-unwrap').click();
  await page.getByRole('button', { name: 'Other Sensor', exact: true }).click();
  await expect(page.locator('#pane-SENSOR_C .pane-wrap-btn')).toBeVisible();
  await expect(page.locator('#pane-SENSOR_C .import-btn')).toHaveCount(0);
});

