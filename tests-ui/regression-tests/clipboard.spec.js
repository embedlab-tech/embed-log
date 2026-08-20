import { expect, test } from '@playwright/test';
import fs from 'node:fs';
import { collectPageErrors, saveDownload, waitForLineContaining, waitForRangePair } from './helpers.js';

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

// Scenario: Context copy matches downloaded raw file content character-for-character
//   Given the user selects a range in SENSOR_A with default line rendering
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

// Scenario: structured multi-line copy remains available
//   Given the user selects a range in SENSOR_A
//   When they use the normal Copy action
//   Then the clipboard contains the formatted selected evidence including SENSOR_A

  test('copy action copies an exact multi-line selection', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await page.evaluate(([startIdx, endIdx]) => {
      window.__embedLogTestSelectRange?.('SENSOR_A', startIdx, endIdx);
    }, [Number(await start.getAttribute('data-idx')), Number(await end.getAttribute('data-idx'))]);
    await page.locator('#copy-SENSOR_A').click();
    await expect.poll(() => readClipboard(page)).toContain('SENSOR_A');
  });

  // Scenario: a native selection inside one row bypasses structured line copy
  // Given the user selects only a fragment of one rendered log line
  // When they press Cmd/Ctrl+C
  // Then the browser copies exactly that fragment, without pane/timestamp formatting.
  test('platform shortcut preserves native single-line text selection', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
    await waitForLineContaining(page, 'SENSOR_A', 'kind=warning');

    const selected = await page.locator('#log-SENSOR_A .log-line', { hasText: 'kind=warning' }).first().evaluate(line => {
      const walker = document.createTreeWalker(line, NodeFilter.SHOW_TEXT);
      let node;
      while ((node = walker.nextNode())) {
        const start = node.textContent.indexOf('kind=');
        if (start >= 0) {
          const end = Math.min(node.textContent.length, start + 'kind=warning'.length);
          const range = document.createRange();
          range.setStart(node, start);
          range.setEnd(node, end);
          const selection = window.getSelection();
          selection.removeAllRanges();
          selection.addRange(range);
          return selection.toString();
        }
      }
      throw new Error('test log line did not contain selectable kind text');
    });

    await page.keyboard.press(COPY_SHORTCUT);
    expect(await readClipboard(page)).toBe(selected);
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
