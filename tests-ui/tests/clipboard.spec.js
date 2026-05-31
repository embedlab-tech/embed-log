import { expect, test } from '@playwright/test';
import fs from 'node:fs';
import { collectPageErrors, saveDownload, waitForRangePair } from './helpers.js';

const COPY_SHORTCUT = process.platform === 'darwin' ? 'Meta+C' : 'Control+C';

async function readClipboard(page) {
  return page.evaluate(() => navigator.clipboard.readText());
}

async function setScope(page, paneId, scope) {
  const btn = page.locator(`#scope-${scope}-${paneId}`);
  await btn.click();
}

async function openMore(page, paneId) {
  await page.locator(`#more-toggle-${paneId}`).click();
}

test.describe('clipboard UX', () => {
  let errors;

  test.beforeEach(async ({ page, context }) => {
    errors = collectPageErrors(page);
    await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  });

  test.afterEach(async () => {
    expect(errors).toEqual([]);
  });

// Scenario: Context copy matches raw file content character-for-character
//   Given the user selects a range in SENSOR_A and switches to context scope
//   When  they click the copy button
//   Then  the clipboard text matches the downloaded raw context file content exactly

  test('context copy matches downloaded context raw file content', async ({ page }, testInfo) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });

    // Switch to context scope
    await setScope(page, 'SENSOR_A', 'context');

    await page.locator('#copy-SENSOR_A').click();
    const copied = (await readClipboard(page)).trimEnd();

    await openMore(page, 'SENSOR_A');
    const downloadPromise = page.waitForEvent('download');
    await page.locator('#download-raw-SENSOR_A').click();
    const download = await downloadPromise;
    const rawPath = await saveDownload(download, testInfo);
    const raw = fs.readFileSync(rawPath, 'utf-8').trimEnd();

    expect(copied).toBe(raw);
  });

// Scenario: Cmd/Ctrl+C copies exact selection to clipboard
//   Given the user selects a range in SENSOR_A
//   When  they press the platform copy shortcut (Cmd/Ctrl+C)
//   Then  the clipboard contains the selected text including SENSOR_A

  test('platform shortcut copies exact selection', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });

    await page.keyboard.press(COPY_SHORTCUT);
    const copied = await readClipboard(page);

    expect(copied.trim().length).toBeGreaterThan(0);
    expect(copied).toContain('SENSOR_A');
  });

  // Clipboard buffer UI was removed in frontend refactoring.
// Scenario: Clipboard buffer add, peek, copy all, and clear across panes
//   Given the user selects lines in SENSOR_A and SENSOR_B and adds them to the clipboard buffer
//   When  they peek at the buffer, copy all contents, then clear it
//   Then  the peek menu shows both sources, copy-all yields both selections, and clearing hides the indicator
  test.skip('clipboard buffer add, peek, copy all, and clear works across panes', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const rangeA = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await rangeA.start.click();
    await rangeA.end.click({ modifiers: ['Shift'] });
    await openMore(page, 'SENSOR_A');
    await page.locator('#clip-add-SENSOR_A').click();

    const rangeB = await waitForRangePair(page, 'SENSOR_B', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await rangeB.start.click();
    await rangeB.end.click({ modifiers: ['Shift'] });
    await openMore(page, 'SENSOR_B');
    await page.locator('#clip-add-SENSOR_B').click();

    await expect(page.locator('#clip-indicator')).toBeVisible();
    await expect(page.locator('#clip-indicator .clip-count')).toContainText(/lines/i);

    await page.locator('#clip-peek-btn').click();
    await expect(page.locator('#clip-peek-menu')).toHaveClass(/open/);
    const body = page.locator('#clip-peek-menu .clip-peek-body');
    await expect(body).toContainText('SENSOR_A');
    await expect(body).toContainText('SENSOR_B');

    await page.locator('#clip-peek-menu .clip-peek-copyall').click();
    const copiedAll = await readClipboard(page);
    expect(copiedAll).toContain('SENSOR_A');
    expect(copiedAll).toContain('SENSOR_B');

    await page.locator('#clip-indicator .clip-clear').click();
    await expect(page.locator('#clip-indicator')).toBeHidden();
    await expect(body).toContainText('(Clipboard buffer is empty)');
  });
});
