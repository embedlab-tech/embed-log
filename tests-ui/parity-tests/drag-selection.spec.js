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

// Scenario: Drag-select creates contiguous selection only in active pane and shows copy actions
//   Given the user drags from line 1 to line 5 in SENSOR_A
//   When  the drag ends
//   Then  selected lines are contiguous
//   And   no lines are selected in SENSOR_B (inactive pane)
//   And   copy actions are visible for SENSOR_A
  test('drag-select creates contiguous selection only in active pane and shows copy actions', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    await waitForLineContaining(page, 'SENSOR_A', 'kind=warning');

    const lines = page.locator('#log-SENSOR_A .log-line');
    await expect.poll(async () => lines.count()).toBeGreaterThanOrEqual(6);
    const start = lines.nth(1);
    const end = lines.nth(5);
    await expect(start).toBeVisible();
    await expect(end).toBeVisible();

    const startBox = await start.boundingBox();
    const endBox = await end.boundingBox();
    expect(startBox).toBeTruthy();
    expect(endBox).toBeTruthy();

    await page.mouse.move(startBox.x + 8, startBox.y + startBox.height / 2);
    await page.mouse.down();
    await page.mouse.move(endBox.x + 8, endBox.y + endBox.height / 2, { steps: 8 });
    await page.mouse.up();

    await expect.poll(async () => (await selectedLineTicks(page, 'SENSOR_A')).length).toBeGreaterThanOrEqual(3);
    const ticks = await selectedLineTicks(page, 'SENSOR_A');
    expect(ticks.length).toBeGreaterThanOrEqual(3);

    const sorted = [...ticks].sort();
    expect(ticks).toEqual(sorted);

    await expect(page.locator('#log-SENSOR_B .log-line.selected')).toHaveCount(0);
    await expect(page.locator('#copy-actions-SENSOR_A')).toHaveClass(/visible/);
  });
});
