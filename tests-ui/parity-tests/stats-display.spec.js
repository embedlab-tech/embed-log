import { expect, test } from '@playwright/test';
import { waitForSourceTestLine } from './helpers.js';

// Scenario: Live pane and toolbar display line/kB stats
//   Given the user loads the app and the demo WS sends log lines
//   When  at least one log line is rendered in a pane
//   Then  the pane header shows "N lines · ... B/kB" with N >= 1
//   And   the toolbar shows a non-empty total of "N lines · ... kB"

test.describe('embed-log line/kB stats display', () => {
  test('pane and toolbar show line counts and byte sizes during live streaming', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    await waitForSourceTestLine(page, 'SENSOR_A');
    await waitForSourceTestLine(page, 'SENSOR_B');

    // Per-pane stats: visible, non-empty, with the expected "N lines" prefix
    const paneA = page.locator('#pane-SENSOR_A .pane-stats');
    const paneB = page.locator('#pane-SENSOR_B .pane-stats');
    await expect(paneA).toBeVisible();
    await expect(paneB).toBeVisible();
    const aText = (await paneA.textContent()) || '';
    const bText = (await paneB.textContent()) || '';
    expect(aText).toMatch(/^\d{1,3}(,\d{3})* lines · [\d.]+ (B|kB|MB)$/);
    expect(bText).toMatch(/^\d{1,3}(,\d{3})* lines · [\d.]+ (B|kB|MB)$/);

    // Toolbar stats: visible and aggregates both panes
    const toolbar = page.locator('#toolbar-stats');
    await expect(toolbar).toBeVisible();
    const tText = (await toolbar.textContent()) || '';
    expect(tText).toMatch(/· \d{1,3}(,\d{3})* lines · [\d.]+ (B|kB|MB)$/);
  });
});
