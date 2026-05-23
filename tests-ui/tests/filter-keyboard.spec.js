import { expect, test } from '@playwright/test';
import { waitForLineContaining, waitForRangePair } from './helpers.js';

test.describe('filter and keyboard UX', () => {
  test('filtering by deterministic kind shows only matching visible lines', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
    await waitForLineContaining(page, 'SENSOR_A', 'kind=filter-alpha');

    const input = page.locator('.filter-input[data-pane="SENSOR_A"]');
    await input.fill('filter-alpha');

    await expect(page.locator('#log-SENSOR_A .log-line:visible')).toHaveCount(1, { timeout: 10_000 });
    await expect(page.locator('#log-SENSOR_A .log-line:visible')).toContainText('kind=filter-alpha');

    await input.fill('');
    await expect.poll(async () => page.locator('#log-SENSOR_A .log-line:visible').count())
      .toBeGreaterThan(1);
  });

  test('Escape clears range selection and hides copy actions', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });

    await expect(page.locator('#copy-actions-SENSOR_A')).toHaveClass(/visible/);
    await page.keyboard.press('Escape');

    await expect(page.locator('#log-SENSOR_A .log-line.selected')).toHaveCount(0);
    await expect(page.locator('#copy-actions-SENSOR_A')).not.toHaveClass(/visible/);
  });
});
