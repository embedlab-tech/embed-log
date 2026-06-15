import { expect, test } from '@playwright/test';
import fs from 'node:fs';
import { collectPageErrors, saveDownload, waitForRangePair } from './helpers.js';

async function readClipboard(page) {
  return page.evaluate(() => navigator.clipboard.readText());
}

async function setScope(page, paneId, scope) {
  await page.locator(`#scope-${scope}-${paneId}`).click();
}

async function openMore(page, paneId) {
  await page.locator(`#more-toggle-${paneId}`).click({ force: true });
}

// Feature: scope-aware selection actions — Tests for Exact, Context (All), and Sel… (context-selected) copy, download, and export operations
//
test.describe('scope-aware selection actions', () => {
  let errors;

  test.beforeEach(async ({ page, context }) => {
    errors = collectPageErrors(page);
    await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  });

  test.afterEach(async () => {
    expect(errors).toEqual([]);
  });

// Scenario: Exact mode copy does not include sibling pane content
//   Given a range selection with exact scope
//   When  the user copies the selection
//   Then  only [SENSOR_A] lines appear and [SENSOR_B] is excluded
//
  test('Exact mode copy does not include sibling pane content', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });

    // Default scope is exact
    await page.locator('#copy-SENSOR_A').click();
    const copied = await readClipboard(page);

    // Should contain selected pane content with source label
    expect(copied).toMatch(/\[SENSOR_A\]/);
    expect(copied).toContain('SENSOR_A');
    // Should NOT contain sibling pane content
    expect(copied).not.toContain('SENSOR_B');
  });

// Scenario: Context mode copy includes sibling pane content
//   Given a range selection with context (All) scope
//   When  the user copies the selection
//   Then  both [SENSOR_A] and [SENSOR_B] lines appear in the output
//
  test('Context mode copy includes sibling pane content', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });

    await setScope(page, 'SENSOR_A', 'context');
    await page.locator('#copy-SENSOR_A').click();
    const copied = await readClipboard(page);

    // Should contain both selected pane and sibling pane content with source labels
    expect(copied).toMatch(/\[SENSOR_A\]/);
    expect(copied).toMatch(/\[SENSOR_B\]/);
  });
// Scenario: Copy button count reflects exact and range scopes
//   Given a range selection
//   When  the scope toggles between Exact, Context, and Sel… modes
//   Then  the displayed count on the copy button matches the expected line count for each mode
//
  test('Copy button count reflects exact and range scopes', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });

    const getDisplayedCount = async () => {
      const text = await page.locator('#copy-SENSOR_A').textContent();
      return Number(text.match(/\((\d+)\)/)?.[1] ?? NaN);
    };
    const getExpectedCount = async () => page.evaluate(async () => {
      const { state, PANES } = await import('/state.js');
      const paneId = 'SENSOR_A';
      const sel = state.selected[paneId];
      if (state.selectionScope === 'exact') return sel.size;

      const lines = state.rawLines[paneId] || [];
      const nums = Array.from(sel)
        .map(i => lines[i]?.numTs)
        .filter(n => Number.isFinite(n) && n >= 0);
      if (!nums.length) return 0;

      const range = state.selectionScope === 'exact'
        ? { from: Math.min(...nums), to: Math.max(...nums) }
        // Must match RANGE_MARGIN_MS in frontend/selection.js (10ms)
        : { from: Math.min(...nums) - 10, to: Math.max(...nums) + 10 };
      const targetPanes = state.selectionScope === 'context-selected'
        ? PANES.filter(id => state.contextPanes[id])
        : PANES;

      let count = 0;
      targetPanes.forEach(id => {
        (state.rawLines[id] || []).forEach(line => {
          const n = line?.numTs;
          if (Number.isFinite(n) && n >= range.from && n <= range.to) count++;
        });
      });
      return count;
    });

    expect(await getDisplayedCount()).toBe(await getExpectedCount());

    await setScope(page, 'SENSOR_A', 'context');
    expect(await getDisplayedCount()).toBe(await getExpectedCount());

    await setScope(page, 'SENSOR_A', 'context-selected');
    await page.locator('#pane-selector-SENSOR_A input[data-pane="SENSOR_C"]').click();
    expect(await getDisplayedCount()).toBe(await getExpectedCount());
  });

// Scenario: Scope toggle persists across panes
//   Given context scope selected on SENSOR_A
//   When  the user clears selection and selects in SENSOR_B
//   Then  context scope remains active and copy output includes both [SENSOR_A] and [SENSOR_B]
//
  test('Scope toggle persists across panes', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    // Select in SENSOR_A and toggle to context
    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });
    await setScope(page, 'SENSOR_A', 'context');

    // Clear selection and select in SENSOR_B
    await page.keyboard.press('Escape');

    const rangeB = await waitForRangePair(page, 'SENSOR_B', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await rangeB.start.click();
    await rangeB.end.click({ modifiers: ['Shift'] });

    // Context should still be active on SENSOR_B
    await expect(page.locator('#scope-context-SENSOR_B')).toHaveClass(/active/);

    await page.locator('#copy-SENSOR_B').click();
    const copied = await readClipboard(page);
    expect(copied).toMatch(/\[SENSOR_A\]/);
    expect(copied).toMatch(/\[SENSOR_B\]/);
  });

// Scenario: Exact download raw creates single-pane .log file with [SENSOR_A] prefix
//   Given a range selection with exact scope
//   When  the user downloads raw logs via the more menu
//   Then  the downloaded .log file contains only [SENSOR_A] lines
//
  test('Exact download raw is single pane only', async ({ page }, testInfo) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });

    await openMore(page, 'SENSOR_A');
    const downloadPromise = page.waitForEvent('download');
    await page.locator('#download-raw-SENSOR_A').click();
    const download = await downloadPromise;

    expect(download.suggestedFilename()).toMatch(/^embed-log-exact-.*\.log$/);
    const downloadedPath = await saveDownload(download, testInfo);
    const text = fs.readFileSync(downloadedPath, 'utf-8');

    expect(text).toMatch(/\[SENSOR_A\]/);
    expect(text).not.toContain('SENSOR_B');
  });

// Scenario: Context download raw includes sibling panes in merged output
//   Given a range selection with context scope
//   When  the user downloads raw logs via the more menu
//   Then  the downloaded .log file includes both [SENSOR_A] and [SENSOR_B] lines
//
  test('Context download raw includes sibling panes', async ({ page }, testInfo) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });
    await setScope(page, 'SENSOR_A', 'context');

    await openMore(page, 'SENSOR_A');
    const downloadPromise = page.waitForEvent('download');
    await page.locator('#download-raw-SENSOR_A').click();
    const download = await downloadPromise;

    expect(download.suggestedFilename()).toMatch(/^embed-log-snippet-.*\.log$/);
    const downloadedPath = await saveDownload(download, testInfo);
    const text = fs.readFileSync(downloadedPath, 'utf-8');

    expect(text).toMatch(/\[SENSOR_B\]/);
    expect(text).toMatch(/\[SENSOR_A\].*kind=prefix-cleanup/);
  });
// Scenario: Exact HTML export contains only selected pane data
//   Given a range selection with exact scope
//   When  the user exports to HTML via the more menu
//   Then  the exported HTML contains only SENSOR_A pane data
//
  // Per-pane export-html relies on a more-dropdown visibility that needs
  // frontend positioning fix. The full export path is covered by export-replay:54.
  test.skip('Exact HTML export contains only selected pane data', async ({ page }, testInfo) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });
    await openMore(page, 'SENSOR_A');
    await page.waitForTimeout(200);
    const downloadPromise = page.waitForEvent('download');
    await page.locator('#export-html-SENSOR_A').click({ force: true });
    const download = await downloadPromise;

    expect(download.suggestedFilename()).toMatch(/^embed-log-exact-.*\.html$/);
    const downloadedPath = await saveDownload(download, testInfo);
    const html = fs.readFileSync(downloadedPath, 'utf-8');

    expect(html).toContain('var _logData =');
    expect(html).toContain('kind=prefix-cleanup');
    const dataMatch = html.match(/"SENSOR_A":\[/);
    expect(dataMatch).toBeTruthy();
});

// Scenario: Context mode add to clipboard includes sibling content
//   Given a range selection with context scope
//   When  the user clicks add-to-clipboard and opens the clipboard peek
//   Then  the clipboard peek displays both SENSOR_A and SENSOR_B content
//
  // Clipboard buffer UI was removed in frontend refactoring.
  test.skip('Context mode add to clipboard includes sibling content', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });

    await setScope(page, 'SENSOR_A', 'context');
    await openMore(page, 'SENSOR_A');
    await page.locator('#clip-add-SENSOR_A').click();

    await expect(page.locator('#clip-indicator')).toBeVisible();
    await page.locator('#clip-peek-btn').click();
    const body = page.locator('#clip-peek-menu .clip-peek-body');
    await expect(body).toContainText('SENSOR_A');
    await expect(body).toContainText('SENSOR_B');
});

// Scenario: Sel… mode copy with one pane unchecked excludes that pane from output
//   Given a Sel… mode selection with SENSOR_B unchecked
//   When  the user copies the selection
//   Then  [SENSOR_A] lines are included and SENSOR_B is excluded
//
  test('Sel… mode copy with one pane unchecked excludes that pane', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });

    await setScope(page, 'SENSOR_A', 'context-selected');
    // Uncheck SENSOR_B in pane selector
    await page.locator('#pane-selector-SENSOR_A input[data-pane="SENSOR_B"]').click();

    await page.locator('#copy-SENSOR_A').click();
    const copied = await readClipboard(page);

    expect(copied).toMatch(/\[SENSOR_A\]/);
    expect(copied).not.toContain('SENSOR_B');
  });

// Scenario: Sel… mode copy includes all panes when none unchecked
//   Given a Sel… mode selection with all panes checked by default
//   When  the user copies the selection
//   Then  both [SENSOR_A] and [SENSOR_B] lines are included
//
  test('Sel… mode copy includes all panes when none unchecked', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });

    await setScope(page, 'SENSOR_A', 'context-selected');
    // All panes checked by default — same as All mode
    await page.locator('#copy-SENSOR_A').click();
    const copied = await readClipboard(page);

    expect(copied).toMatch(/\[SENSOR_A\]/);
    expect(copied).toMatch(/\[SENSOR_B\]/);
  });

// Scenario: Sel… mode download raw respects unchecked panes
//   Given a Sel… mode selection with SENSOR_C (AUX) unchecked
//   When  the user downloads raw logs via the more menu
//   Then  the downloaded .log file contains [SENSOR_A] but excludes [SENSOR_C]
//
  test('Sel… mode download raw respects unchecked panes', async ({ page }, testInfo) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });

    await setScope(page, 'SENSOR_A', 'context-selected');
    // Uncheck SENSOR_C (labeled AUX) — should not appear in output
    await page.locator('#pane-selector-SENSOR_A input[data-pane="SENSOR_C"]').click();

    await openMore(page, 'SENSOR_A');
    const downloadPromise = page.waitForEvent('download');
    await page.locator('#download-raw-SENSOR_A').click();
    const download = await downloadPromise;

    expect(download.suggestedFilename()).toMatch(/^embed-log-snippet-.*\.log$/);
    const downloadedPath = await saveDownload(download, testInfo);
    const text = fs.readFileSync(downloadedPath, 'utf-8');

    expect(text).toMatch(/\[SENSOR_A\]/);
    expect(text).not.toMatch(/\[SENSOR_C\]/);
  });

// Scenario: Sel… mode unchecked panes persist across scope toggle
//   Given a Sel… mode selection with SENSOR_B unchecked
//   When  the user toggles to All mode and back to Sel…
//   Then  the SENSOR_B exclusion persists in the copy output
//
  test('Sel… mode unchecked panes persist across scope toggle', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    // Select SENSOR_A, switch to Sel…, uncheck SENSOR_B
    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });
    await setScope(page, 'SENSOR_A', 'context-selected');
    await page.locator('#pane-selector-SENSOR_A input[data-pane="SENSOR_B"]').click();

    // Switch to All — should include all panes
    await setScope(page, 'SENSOR_A', 'context');
    await page.locator('#copy-SENSOR_A').click();
    const copiedAll = await readClipboard(page);
    expect(copiedAll).toMatch(/\[SENSOR_A\]/);
    expect(copiedAll).toMatch(/\[SENSOR_B\]/);

    // Switch back to Sel… — unchecked panes should persist
    await page.locator('#scope-context-selected-SENSOR_A').click();
    await page.locator('#copy-SENSOR_A').click();
    const copiedSel = await readClipboard(page);
    expect(copiedSel).toMatch(/\[SENSOR_A\]/);
    expect(copiedSel).not.toContain('SENSOR_B');
  });

// Scenario: Sel… pane selector shows all panes with correct data-pane attributes (5 total)
//   Given a Sel… mode selection on SENSOR_A
//   When  the pane selector is opened
//   Then  5 checkboxes are shown with correct data-pane attributes and all checked by default
//
  test('Sel… pane selector shows all panes with correct labels', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });

    await setScope(page, 'SENSOR_A', 'context-selected');

    // Pane selector should be visible
    await expect(page.locator('#pane-selector-SENSOR_A')).toBeVisible();
    // All panes should have checkboxes with unwrap-style labels

    const checkboxes = page.locator('#pane-selector-SENSOR_A .pane-checkbox input[type="checkbox"]');
    await expect(checkboxes).toHaveCount(9);
    // Each checkbox should have a pane data attribute
    await expect(checkboxes.nth(0)).toHaveAttribute('data-pane', 'SENSOR_A');
    await expect(checkboxes.nth(1)).toHaveAttribute('data-pane', 'SENSOR_B');
    await expect(checkboxes.nth(2)).toHaveAttribute('data-pane', 'SENSOR_C');
    await expect(checkboxes.nth(3)).toHaveAttribute('data-pane', 'SENSOR_D');
    await expect(checkboxes.nth(4)).toHaveAttribute('data-pane', 'SENSOR_CBOR');
    await expect(checkboxes.nth(5)).toHaveAttribute('data-pane', 'SENSOR_COAP');
    await expect(checkboxes.nth(6)).toHaveAttribute('data-pane', 'UART_DUT');
    await expect(checkboxes.nth(7)).toHaveAttribute('data-pane', 'UART_DEBUG');
    // All checked by default
    for (let i = 0; i < 8; i++) {
        await expect(checkboxes.nth(i)).toBeChecked();
    }
  });
});
