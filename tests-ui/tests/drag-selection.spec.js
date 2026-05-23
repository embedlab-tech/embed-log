import { expect, test } from '@playwright/test';
import { collectPageErrors, selectedLineTicks, waitForLineContaining } from './helpers.js';

test.describe('drag selection UX', () => {
  let errors;

  test.beforeEach(async ({ page }) => {
    errors = collectPageErrors(page);
  });

  test.afterEach(async () => {
    expect(errors).toEqual([]);
  });

  test('drag-select creates contiguous selection only in active pane and shows copy actions', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    await waitForLineContaining(page, 'SENSOR_A', 'kind=warning');

    const lines = page.locator('#log-SENSOR_A .log-line');
    const start = lines.nth(1);
    const end = lines.nth(5);

    await start.hover();
    await page.mouse.down();
    await end.hover();
    await page.mouse.up();

    const ticks = await selectedLineTicks(page, 'SENSOR_A');
    expect(ticks.length).toBeGreaterThanOrEqual(3);

    const sorted = [...ticks].sort();
    expect(ticks).toEqual(sorted);

    await expect(page.locator('#log-SENSOR_B .log-line.selected')).toHaveCount(0);
    await expect(page.locator('#copy-actions-SENSOR_A')).toHaveClass(/visible/);
  });
});
