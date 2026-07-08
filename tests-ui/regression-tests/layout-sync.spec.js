import { expect, test } from '@playwright/test';
import { collectPageErrors, selectedLineTicks, visiblePaneNames, waitForLineContaining, waitForSourceTestLine } from './helpers.js';

async function findCommonVisibleTick(page, leftPane, rightPane) {
  return page.evaluate(({ leftPane, rightPane }) => {
    const ticks = paneId => Array.from(document.querySelectorAll(`#log-${paneId} .log-line`))
      .map(node => /tick=(\d{3})/.exec(node.textContent || '')?.[1])
      .filter(Boolean);
    const right = new Set(ticks(rightPane));
    return ticks(leftPane).find(tick => right.has(tick)) || null;
  }, { leftPane, rightPane });
}

async function ensureLineContainingVisible(page, paneId, text) {
  const rawIndex = await page.evaluate(({ paneId, text }) =>
    window.__embedLogFindRawIndexContaining?.(paneId, text) ?? -1,
    { paneId, text }
  );
  expect(rawIndex).toBeGreaterThanOrEqual(0);
  await page.evaluate(({ paneId, rawIndex }) => {
    window.__embedLogEnsureLineVisible?.(paneId, rawIndex, { align: 'center' });
  }, { paneId, rawIndex });
  const line = page.locator(`#log-${paneId} [data-idx="${rawIndex}"]`);
  await expect(line).toBeVisible({ timeout: 10_000 });
  return line;
}

// Feature: layout and time synchronization — tab layout matching backend config, line sync-highlights, range selection, per-pane wrap, and UNWRAP mode

test.describe('layout and time synchronization', () => {
  let errors;

  test.beforeEach(async ({ page }) => {
    errors = collectPageErrors(page);
  });

  test.afterEach(async () => {
    expect(errors).toEqual([]);
  });

// Scenario: Demo tabs and pane order match backend config labels
//   Given a fresh session
//   Then  DevA and DevB tab buttons are visible, DevA tab shows DEVICE_A and HOST panes with a splitter, DevB tab shows AUX pane without splitter

  test('demo tabs and pane order match backend config', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    await expect(page.getByRole('button', { name: 'DevA', exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: 'DevB', exact: true })).toBeVisible();

    await expect.poll(async () => visiblePaneNames(page)).toEqual(['DEVICE_A', 'HOST']);
    await expect(page.locator('#tab-content-0 .splitter')).toHaveCount(1);

    await page.getByRole('button', { name: 'DevB', exact: true }).click();
    await expect.poll(async () => visiblePaneNames(page)).toEqual(['AUX']);
    await expect(page.locator('#tab-content-1 .splitter')).toHaveCount(0);
  });

// Scenario: Clicking a line sync-highlights the nearest timestamp in the sibling pane and survives rerenders/tab switches
//   Given a line in SENSOR_A with a known tick
//   When  the line is clicked, panes rerender, and the user switches tabs
//   Then  SENSOR_A, SENSOR_B, and the synced pane on DevB retain sync-highlight at the matching tick

  test('clicking a line keeps sync-highlight across rerenders and tabs', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    await waitForSourceTestLine(page, 'SENSOR_A');
    await waitForSourceTestLine(page, 'SENSOR_B');
    let tick = null;
    await expect.poll(async () => {
      tick = await findCommonVisibleTick(page, 'SENSOR_A', 'SENSOR_B');
      return tick;
    }, { timeout: 30_000 }).not.toBeNull();
    expect(tick).toMatch(/^\d{3}$/);

    const lineA = await ensureLineContainingVisible(page, 'SENSOR_A', `tick=${tick}`);
    await ensureLineContainingVisible(page, 'SENSOR_B', `tick=${tick}`);

    await lineA.click();

    await expect(await ensureLineContainingVisible(page, 'SENSOR_A', `tick=${tick}`)).toHaveClass(/sync-highlight/);
    await expect(await ensureLineContainingVisible(page, 'SENSOR_B', `tick=${tick}`)).toHaveClass(/sync-highlight/);

    await page.evaluate(() => {
      const original = window.__embedLogSchedulePersist;
      window.__testLogFlushesAfterSyncClick = 0;
      window.__embedLogSchedulePersist = function (...args) {
        window.__testLogFlushesAfterSyncClick += 1;
        return original?.apply(this, args);
      };
    });
    await expect.poll(() => page.evaluate(() => window.__testLogFlushesAfterSyncClick || 0)).toBeGreaterThan(0);

    await expect(await ensureLineContainingVisible(page, 'SENSOR_A', `tick=${tick}`)).toHaveClass(/sync-highlight/);
    await expect(await ensureLineContainingVisible(page, 'SENSOR_B', `tick=${tick}`)).toHaveClass(/sync-highlight/);

    await page.locator('#btn-settings').click();
    await page.locator('#btn-font-inc').click();

    await expect(await ensureLineContainingVisible(page, 'SENSOR_A', `tick=${tick}`)).toHaveClass(/sync-highlight/);
    await expect(await ensureLineContainingVisible(page, 'SENSOR_B', `tick=${tick}`)).toHaveClass(/sync-highlight/);

    await page.getByRole('button', { name: 'DevB', exact: true }).click();
    await expect(page.locator('#log-SENSOR_C .log-line.sync-highlight')).toContainText('TEST src=SENSOR_C');

    await page.getByRole('button', { name: 'DevA', exact: true }).click();
    await expect(await ensureLineContainingVisible(page, 'SENSOR_A', `tick=${tick}`)).toHaveClass(/sync-highlight/);
    await expect(await ensureLineContainingVisible(page, 'SENSOR_B', `tick=${tick}`)).toHaveClass(/sync-highlight/);
  });

// Scenario: Shift+Click selects a contiguous range without selecting sibling panes
//   Given lines in SENSOR_A
//   When  the first and fourth lines are clicked with Shift held
//   Then  copy actions are shown, at least 4 ticks are selected in SENSOR_A, and SENSOR_B has no selected lines

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

// Scenario: Per-pane wrap toggle makes long lines wrap when the pane is narrowed
//   Given a pane with visible log lines
//   When  wrap is toggled on and the log area is narrowed to 180px
//   Then  scrollHeight increases as lines take more vertical space

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

// Scenario: Per-pane wrap does not affect sibling pane wrapping state
//   Given two panes SENSOR_A and SENSOR_B
//   When  wrap is toggled on SENSOR_A only
//   Then  SENSOR_B does not have wrap class or active wrap button, while SENSOR_A does

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
// Scenario: UNWRAP toggle creates one tab per pane with pane names as labels
//   Given a session with group tabs DevA/DevB
//   When  UNWRAP is toggled on
//   Then  tab bar shows [DEVICE_A-DevA, HOST-DevA, AUX-DevB, PYTEST-PYTEST, CBOR-cbor-tab] and no add-tab button

test('UNWRAP toggle creates one tab per pane with pane names as labels', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
  await waitForSourceTestLine(page, 'SENSOR_A');

  // Before unwrap: tabs are group labels
  await expect(page.getByRole('button', { name: 'DevA', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: 'DevB', exact: true })).toBeVisible();

  // Click UNWRAP
  await page.locator('#btn-unwrap').click();
  await expect(page.locator('#btn-unwrap')).toHaveClass(/active/);
  // Now tabs are pane names
  await expect(page.locator('#tab-bar .tab-btn')).toHaveText(['DEVICE_A-DevA', 'HOST-DevA', 'AUX-DevB', 'PYTEST-PYTEST', 'CBOR-cbor-tab', 'CoAP-CoAP', 'DUT-UART', 'DEBUG-UART', 'Net-Network']);
  for (let i = 0; i < 9; i++) {
    await page.locator('#tab-bar .tab-btn').nth(i).click();
  }

  // Verify no "+" button in unwrap mode
  await expect(page.locator('#tab-bar .tab-add')).toHaveCount(0);
});

// Scenario: UNWRAP preserves log content when toggled on and off
//   Given a session with log data in SENSOR_A
//   When  UNWRAP is toggled on and then off again
//   Then  logs remain visible and contain expected test content after each toggle

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

// Scenario: UNWRAP mode shows full-width single pane and all pane tabs exist
//   Given a session with data in SENSOR_A and SENSOR_B
//   When  UNWRAP is toggled on
//   Then  each tab shows a single pane (no splitter) and all panes are accessible via tab buttons

test('UNWRAP mode shows full-width single pane and all panes exist', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
  await waitForSourceTestLine(page, 'SENSOR_A');
  await waitForSourceTestLine(page, 'SENSOR_B');

  await page.locator('#btn-unwrap').click();

  // SENSOR_A and SENSOR_B are on separate tabs now
  await page.locator('#tab-bar .tab-btn').nth(0).click();
  await expect(page.locator('#pane-SENSOR_A')).toBeVisible();
  await expect(page.locator('#log-SENSOR_A')).toContainText('TEST src=SENSOR_A');

  await page.locator('#tab-bar .tab-btn').nth(1).click();
  await expect(page.locator('#pane-SENSOR_B')).toBeVisible();
  await expect(page.locator('#log-SENSOR_B')).toContainText('TEST src=SENSOR_B');

  // Verify no splitters in unwrap mode (single pane per tab)
  await expect(page.locator('#tab-content-0 .splitter')).toHaveCount(0);
});
// Scenario: UNWRAP preserves the currently visible pane when toggled from another tab
//   Given a session on the DevB tab showing AUX pane
//   When  UNWRAP is toggled on
//   Then  the visible pane name becomes AUX-DevB and its log lines respond to clicks

test('UNWRAP preserves the currently visible pane when toggled from another tab', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

  await page.getByRole('button', { name: 'DevB', exact: true }).click();
  await waitForSourceTestLine(page, 'SENSOR_C');
  await expect.poll(async () => visiblePaneNames(page)).toEqual(['AUX']);

  await page.locator('#btn-unwrap').click();
  await expect(page.locator('#btn-unwrap')).toHaveClass(/active/);
  await expect.poll(async () => visiblePaneNames(page)).toEqual(['AUX-DevB']);

  const lineC = await waitForSourceTestLine(page, 'SENSOR_C');
  await lineC.click();
  await expect(page.locator('#log-SENSOR_C .log-line.sync-highlight')).toContainText('TEST src=SENSOR_C');
});

// Scenario: Pane headers keep only Wrap controls after layout creation and rebuild
//   Given a pane with a wrap button
//   Then  the pane has a wrap button but no import button or file input
//   When  UNWRAP is toggled on and a different unwrap tab is selected, and then unwrap is toggled off
//   Then  the pane still shows only the wrap button without import controls
test('pane headers keep only Wrap controls after layout creation and rebuild', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
  await waitForSourceTestLine(page, 'SENSOR_A');

  await expect(page.locator('#pane-SENSOR_A .pane-wrap-btn')).toBeVisible();
  await expect(page.locator('#pane-SENSOR_A .import-btn')).toHaveCount(0);
  await expect(page.locator('#pane-SENSOR_A input[type="file"]')).toHaveCount(0);

  await page.locator('#btn-unwrap').click();
  await page.locator('#tab-bar .tab-btn').nth(2).click();
  await expect(page.locator('#pane-SENSOR_C .pane-wrap-btn')).toBeVisible();
  await expect(page.locator('#pane-SENSOR_C .import-btn')).toHaveCount(0);
  await expect(page.locator('#pane-SENSOR_C input[type="file"]')).toHaveCount(0);

  await page.locator('#btn-unwrap').click();
  await page.getByRole('button', { name: 'DevB', exact: true }).click();
  await expect(page.locator('#pane-SENSOR_C .pane-wrap-btn')).toBeVisible();
  await expect(page.locator('#pane-SENSOR_C .import-btn')).toHaveCount(0);
});

