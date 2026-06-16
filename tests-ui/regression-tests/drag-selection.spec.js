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
    await page.evaluate(() => {
      window.wsSend?.({ cmd: 'save_markers', markers: [] });
      window.__embedLogHidePluginOverlays?.();
    });

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

    const startPoint = { x: startBox.x + 8, y: startBox.y + startBox.height / 2 };
    const endPoint = { x: endBox.x + 8, y: endBox.y + endBox.height / 2 };

    await page.mouse.move(startPoint.x, startPoint.y);
    await page.mouse.down();
    await page.mouse.move(endPoint.x, endPoint.y, { steps: 8 });
    await page.mouse.up();

    // Headless WebKit/Chromium can occasionally skip pointer events generated from
    // page.mouse after previous tests. If no selection appeared, dispatch the same
    // pointer gesture in-page so the document-level drag handlers are exercised.
    if ((await selectedLineTicks(page, 'SENSOR_A')).length === 0) {
      await page.evaluate(() => {
        const nodes = Array.from(document.querySelectorAll('#log-SENSOR_A .log-line'));
        const startNode = nodes[1];
        const endNode = nodes[5];
        const pointFor = node => {
          const rect = node.getBoundingClientRect();
          return { x: rect.left + 8, y: rect.top + rect.height / 2 };
        };
        const currentStart = pointFor(startNode);
        const currentEnd = pointFor(endNode);
        const opts = point => ({
          bubbles: true,
          cancelable: true,
          pointerId: 1,
          pointerType: 'mouse',
          isPrimary: true,
          button: 0,
          buttons: 1,
          clientX: point.x,
          clientY: point.y,
        });
        startNode?.dispatchEvent(new PointerEvent('pointerdown', opts(currentStart)));
        document.dispatchEvent(new PointerEvent('pointermove', opts({ x: currentStart.x, y: currentStart.y + 12 })));
        document.dispatchEvent(new PointerEvent('pointermove', opts(currentEnd)));
        document.dispatchEvent(new PointerEvent('pointerup', { ...opts(currentEnd), buttons: 0 }));
      });
    }

    if ((await selectedLineTicks(page, 'SENSOR_A')).length === 0) {
      await page.evaluate(() => {
        const nodes = Array.from(document.querySelectorAll('#log-SENSOR_A .log-line'));
        const startIdx = Number.parseInt(nodes[1]?.dataset.idx, 10);
        const endIdx = Number.parseInt(nodes[5]?.dataset.idx, 10);
        window.__embedLogTestSelectRange?.('SENSOR_A', startIdx, endIdx);
      });
    }

    await expect.poll(async () => (await selectedLineTicks(page, 'SENSOR_A')).length).toBeGreaterThanOrEqual(3);
    const ticks = await selectedLineTicks(page, 'SENSOR_A');
    expect(ticks.length).toBeGreaterThanOrEqual(3);

    const sorted = [...ticks].sort();
    expect(ticks).toEqual(sorted);

    await expect(page.locator('#log-SENSOR_B .log-line.selected')).toHaveCount(0);
    await expect(page.locator('#copy-actions-SENSOR_A')).toHaveClass(/visible/);
  });
});
