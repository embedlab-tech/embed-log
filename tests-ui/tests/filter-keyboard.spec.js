import { expect, test } from '@playwright/test';
import { collectPageErrors, waitForLineContaining, waitForRangePair } from './helpers.js';

test.describe('filter and keyboard UX', () => {
  let errors;

  test.beforeEach(async ({ page }) => {
    errors = collectPageErrors(page);
  });

  test.afterEach(async () => {
    expect(errors).toEqual([]);
  });

  test('valid regex filter shows only matching visible lines', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
    await waitForLineContaining(page, 'SENSOR_A', 'kind=filter-alpha');

    const input = page.locator('.filter-input[data-pane="SENSOR_A"]');
    await input.fill('filter-alpha');

    await expect(page.locator('#log-SENSOR_A .log-line:visible')).toContainText('kind=filter-alpha');
    // Only lines matching the filter should be visible — there should be 1 or 2
    const visibleCount = await page.locator('#log-SENSOR_A .log-line:visible').count();
    expect(visibleCount).toBeGreaterThanOrEqual(1);
    expect(visibleCount).toBeLessThanOrEqual(3);

    // Clear filter — all lines reappear
    await input.fill('');
    await expect.poll(async () => page.locator('#log-SENSOR_A .log-line:visible').count())
      .toBeGreaterThan(3);
  });

  test('invalid regex does not break UI and shows input error state', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
    await waitForLineContaining(page, 'SENSOR_A', 'kind=filter-alpha');

    const input = page.locator('.filter-input[data-pane="SENSOR_A"]');

    // Enter an invalid regex
    await input.fill('filter-(alpha');
    await expect(input).toHaveClass(/invalid/);

    // UI should still be responsive — clicking a line works
    await waitForLineContaining(page, 'SENSOR_A', 'kind=warning');
    const line = page.locator('#log-SENSOR_A .log-line').first();
    await line.click();
    await expect(page.locator('#log-SENSOR_A .log-line.sync-highlight')).toHaveCount(1);

    // Clearing the input removes the error state and shows all lines
    await input.fill('');
    await expect(input).not.toHaveClass(/invalid/);
    await expect.poll(async () => page.locator('#log-SENSOR_A .log-line:visible').count())
      .toBeGreaterThan(3);
  });

  test('invalid regex preserves previous valid filter', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
    await waitForLineContaining(page, 'SENSOR_A', 'kind=filter-alpha');

    const input = page.locator('.filter-input[data-pane="SENSOR_A"]');

    // First enter a valid filter
    await input.fill('filter-alpha');
    await expect(input).not.toHaveClass(/invalid/);
    await expect(page.locator('#log-SENSOR_A .log-line:visible')).toContainText('kind=filter-alpha');

    // Now type a broken character — the previous valid filter should still be active
    await input.fill('filter-alpha(');
    await expect(input).toHaveClass(/invalid/);
    // Only filter-alpha lines should still be visible (previous filter preserved)
    await expect(page.locator('#log-SENSOR_A .log-line:visible')).toContainText('kind=filter-alpha');

    // Fix the regex — invalid class should be removed
    await input.fill('filter-alpha');
    await expect(input).not.toHaveClass(/invalid/);
    await expect(page.locator('#log-SENSOR_A .log-line:visible')).toContainText('kind=filter-alpha');
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
