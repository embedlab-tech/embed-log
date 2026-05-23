import { expect, test } from '@playwright/test';
import fs from 'node:fs';

async function waitForTick(page, paneId, tick, timeout = 30_000) {
  const tickText = `tick=${String(tick).padStart(3, '0')}`;
  const line = page.locator(`#log-${paneId} .log-line`, { hasText: tickText }).first();
  await expect(line, `${paneId} should contain ${tickText}`).toBeVisible({ timeout });
  return line;
}

test.describe('embed-log demo UI', () => {
  test('connects to backend and receives deterministic demo logs', async ({ page }) => {
    await page.goto('/');

    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
    await expect(page.locator('#pane-SENSOR_A')).toBeVisible();
    await expect(page.locator('#pane-SENSOR_B')).toBeVisible();
    await expect(page.locator('#pane-SENSOR_C')).toBeAttached();

    await waitForTick(page, 'SENSOR_A', 5);
    await waitForTick(page, 'SENSOR_B', 5);

    await page.getByRole('button', { name: 'Other Sensor' }).click();
    await waitForTick(page, 'SENSOR_C', 5);
  });

  test('shift-click selects a deterministic range and raw snippet downloads cleaned merged text', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const start = await waitForTick(page, 'SENSOR_A', 9);
    const end = await waitForTick(page, 'SENSOR_A', 11);
    await start.click();
    await end.click({ modifiers: ['Shift'] });

    await expect.poll(async () => page.locator('#log-SENSOR_A .log-line.selected').count())
      .toBeGreaterThanOrEqual(3);
    await expect(page.locator('#copy-actions-SENSOR_A')).toHaveClass(/visible/);

    const downloadPromise = page.waitForEvent('download');
    await page.locator('#download-range-raw-SENSOR_A').click();
    const download = await downloadPromise;

    expect(download.suggestedFilename()).toMatch(/^embed-log-snippet-.*\.log$/);
    const downloadedPath = await download.path();
    expect(downloadedPath).toBeTruthy();

    const text = fs.readFileSync(downloadedPath, 'utf-8');
    expect(text).toContain('[SENSOR_A]');
    expect(text).toContain('tick=009');
    expect(text).toContain('kind=prefix-cleanup');
    expect(text).toContain('kind=timestamp-cleanup');
    expect(text).not.toMatch(/\[SENSOR_A\]\s+\[SENSOR_A\]/);
    expect(text).not.toMatch(/\[SENSOR_A\]\s+\[\d{4}-\d{2}-\d{2}T/);
  });

  test('HTML snippet uses the regular embed-log exported UI', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const start = await waitForTick(page, 'SENSOR_A', 9);
    const end = await waitForTick(page, 'SENSOR_A', 11);
    await start.click();
    await end.click({ modifiers: ['Shift'] });

    const downloadPromise = page.waitForEvent('download');
    await page.locator('#download-range-html-SENSOR_A').click();
    const download = await downloadPromise;

    expect(download.suggestedFilename()).toMatch(/^embed-log-snippet-.*\.html$/);
    const downloadedPath = await download.path();
    expect(downloadedPath).toBeTruthy();

    const html = fs.readFileSync(downloadedPath, 'utf-8');
    expect(html).toContain('<div id="toolbar">');
    expect(html).toContain('<div id="tab-bar"></div>');
    expect(html).toContain('var _logData =');
    expect(html).not.toContain('<h1>embed-log snippet</h1>');
  });
});
