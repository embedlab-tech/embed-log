import { expect, test } from '@playwright/test';
import fs from 'node:fs';
import { collectPageErrors, openHtmlFile, saveDownload, visiblePaneNames, waitForRangePair } from './helpers.js';

// Feature: add and remove panes in a view. Views: DevA = [DEVICE_A, HOST],
// DevB = [AUX], UART = [DUT, DEBUG]. A removed pane keeps recording.

// The header counter is updated for hidden panes too: it proves they keep recording.
const lineCount = (page, paneId) => page.evaluate(id => {
  const text = document.querySelector(`[data-pane-stats="${id}"]`)?.textContent || '';
  return Number((/([\d,]+) lines/.exec(text)?.[1] || '0').replace(/,/g, ''));
}, paneId);

// The tab popup uses custom dropdowns: open the trigger, then pick an option.
const pickFromMenu = async (page, label, value) => {
  await page.getByLabel(label, { exact: true }).click();
  await page.locator(`.dd-menu:not([hidden]) .dd-option[data-value="${value}"]`).click();
};

const openTabMenu = async (page, label) => {
  await page.getByRole('button', { name: label, exact: true }).hover();
  await expect(page.locator('#tab-swap-menu.open')).toBeVisible();
};

test.describe('pane add / remove', () => {
  let errors;
  test.beforeEach(async ({ page }) => {
    errors = collectPageErrors(page);
    // Fresh storage for the test, but kept across its own reloads.
    await page.addInitScript(() => {
      if (!sessionStorage.getItem('fresh')) { localStorage.clear(); sessionStorage.setItem('fresh', '1'); }
    });
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
  });
  test.afterEach(async () => { expect(errors).toEqual([]); });

  test('a pane can be removed from its header, and the last one cannot', async ({ page }) => {
    await expect.poll(() => visiblePaneNames(page)).toEqual(['DEVICE_A', 'HOST']);
    await expect(page.locator('#pane-SENSOR_B .pane-close-btn')).toBeVisible();
    await page.locator('#pane-SENSOR_B .pane-close-btn').click();

    await expect.poll(() => visiblePaneNames(page)).toEqual(['DEVICE_A']);
    await expect(page.locator('#tab-content-0 .splitter')).toHaveCount(0);
    // A view keeps at least one pane.
    await expect(page.locator('#pane-SENSOR_A .pane-close-btn')).toBeHidden();
    // A single-pane view has nothing to remove either.
    await page.getByRole('button', { name: 'DevB', exact: true }).click();
    await expect(page.locator('#pane-SENSOR_C .pane-close-btn')).toBeHidden();
  });

  test('a removed pane keeps recording and shows its history when added back', async ({ page }) => {
    await expect.poll(() => lineCount(page, 'SENSOR_B'), { timeout: 30_000 }).toBeGreaterThan(2);
    await page.locator('#pane-SENSOR_B .pane-close-btn').click();
    await expect.poll(() => visiblePaneNames(page)).toEqual(['DEVICE_A']);
    const whileHidden = await lineCount(page, 'SENSOR_B');
    await expect.poll(() => lineCount(page, 'SENSOR_B'), { timeout: 30_000 }).toBeGreaterThan(whileHidden);
    const before = await lineCount(page, 'SENSOR_B');

    await openTabMenu(page, 'DevA');
    await pickFromMenu(page, 'Add a pane to DevA', 'SENSOR_B');
    await expect.poll(() => visiblePaneNames(page)).toEqual(['DEVICE_A', 'HOST']);
    await expect(page.locator('#tab-content-0 .splitter')).toHaveCount(1);
    expect(await lineCount(page, 'SENSOR_B')).toBeGreaterThanOrEqual(before);
    await expect(page.locator('#log-SENSOR_B .log-line').first()).toBeVisible();
  });

  test('a pane of another view can be moved in, but never out of a one-pane view', async ({ page }) => {
    await openTabMenu(page, 'DevA');
    await page.getByLabel('Add a pane to DevA', { exact: true }).click();
    // AUX is alone in DevB, so it is not offered; DUT and DEBUG share UART, so they are.
    await expect(page.locator('.dd-menu:not([hidden]) .dd-option[data-value="SENSOR_C"]')).toHaveCount(0);
    await expect(page.locator('.dd-menu:not([hidden]) .dd-group')).toHaveText(['Move from another view']);
    await page.locator('.dd-menu:not([hidden]) .dd-option[data-value="UART_DEBUG"]').click();
    await expect.poll(() => visiblePaneNames(page)).toEqual(['DEVICE_A', 'HOST', 'DEBUG']);

    await page.getByRole('button', { name: 'UART', exact: true }).click();
    await expect.poll(() => visiblePaneNames(page)).toEqual(['DUT']);
    await expect(page.locator('#pane-UART_DUT .pane-close-btn')).toBeHidden();
  });

  test('the layout survives a reload and Reset layout restores the configured one', async ({ page }) => {
    await page.locator('#pane-SENSOR_B .pane-close-btn').click();
    await expect.poll(() => visiblePaneNames(page)).toEqual(['DEVICE_A']);
    await page.reload();
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
    await expect.poll(() => visiblePaneNames(page)).toEqual(['DEVICE_A']);

    await openTabMenu(page, 'DevA');
    await page.getByRole('button', { name: 'Reset layout' }).click();
    await expect.poll(() => visiblePaneNames(page)).toEqual(['DEVICE_A', 'HOST']);
    await expect(page.getByRole('button', { name: 'Reset layout' })).toHaveCount(0);

    await page.reload();
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
    await expect.poll(() => visiblePaneNames(page)).toEqual(['DEVICE_A', 'HOST']);
  });

  test('removing works from the tab menu and survives UNWRAP', async ({ page }) => {
    await openTabMenu(page, 'DevA');
    await page.locator('#tab-swap-menu').getByRole('button', { name: 'Remove HOST from this view' }).click();
    await expect.poll(() => visiblePaneNames(page)).toEqual(['DEVICE_A']);
    await expect(page.locator('#tab-swap-menu').getByRole('button', { name: 'Remove DEVICE_A from this view' })).toBeDisabled();

    await page.getByRole('button', { name: /^(Unwrap|Split tabs)$/ }).click();
    await page.getByRole('button', { name: /^(Unwrap|Split tabs)$/ }).click();
    await expect.poll(() => visiblePaneNames(page)).toEqual(['DEVICE_A']);
    await expect(page.locator('#pane-SENSOR_B')).toHaveCount(1);
    await expect(page.locator('#hidden-panes #pane-SENSOR_B')).toHaveCount(1);
  });

  test('swap and add use custom dropdowns that work from the keyboard', async ({ page }) => {
    await openTabMenu(page, 'DevA');
    // Hidden panes are listed under their own heading and shown with the view they are in.
    await page.locator('#tab-swap-menu').getByRole('button', { name: 'Remove HOST from this view' }).click();
    await page.getByLabel('Add a pane to DevA', { exact: true }).click();
    await expect(page.locator('.dd-menu:not([hidden]) .dd-group').first()).toHaveText('Hidden');
    await page.keyboard.press('Escape');
    await expect(page.locator('.dd-menu:not([hidden])')).toHaveCount(0);

    // Swap DEVICE_A with the pane of DevB from the keyboard.
    const swap = page.getByLabel('Swap DEVICE_A with', { exact: true });
    await swap.focus();
    await page.keyboard.press('ArrowDown');
    await expect(page.locator('.dd-menu:not([hidden])')).toBeVisible();
    await page.locator('.dd-menu:not([hidden]) .dd-option[data-value="SENSOR_C"]').focus();
    await page.keyboard.press('Enter');
    await expect.poll(() => visiblePaneNames(page)).toEqual(['AUX']);
    await expect(page.locator('#tab-content-1 #pane-SENSOR_A')).toHaveCount(1);
  });

  test('dragging a separator between two panes never squeezes the third', async ({ page }) => {
    // UART = [DUT, DEBUG]; bring HOST over so that the view reads DUT | DEBUG | HOST.
    await page.locator('#pane-SENSOR_B .pane-close-btn').click();
    await page.getByRole('button', { name: 'UART', exact: true }).click();
    await openTabMenu(page, 'UART');
    await pickFromMenu(page, 'Add a pane to UART', 'SENSOR_B');
    await expect.poll(() => visiblePaneNames(page)).toEqual(['DUT', 'DEBUG', 'HOST']);

    await page.mouse.move(700, 400); // off the tab: let its popup close
    await expect(page.locator('#tab-swap-menu.open')).toHaveCount(0);
    const width = id => page.locator(`#pane-${id}`).evaluate(el => el.getBoundingClientRect().width);
    const before = { debug: await width('UART_DEBUG'), third: await width('SENSOR_B') };
    const splitter = page.locator('#tab-content-4 .splitter').first();
    const box = await splitter.boundingBox();
    await page.mouse.move(box.x + box.width / 2, box.y + 40);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width / 2 + 60, box.y + 40, { steps: 5 });
    await page.mouse.up();

    expect(await width('UART_DEBUG')).toBeLessThan(before.debug - 30); // it did move
    expect(Math.abs((await width('SENSOR_B')) - before.third)).toBeLessThan(2); // the third did not
  });

  test('swapping panes refreshes the remove buttons, enables Reset and survives a reload', async ({ page }) => {
    // DevA = [DEVICE_A, HOST], DevB = [AUX]: after swapping DEVICE_A with AUX, DevA = [AUX, HOST].
    await openTabMenu(page, 'DevA');
    await pickFromMenu(page, 'Swap DEVICE_A with', 'SENSOR_C');
    await expect.poll(() => visiblePaneNames(page)).toEqual(['AUX', 'HOST']);
    // The pane that moved to the two-pane view can be removed; the one now alone cannot.
    await expect(page.locator('#pane-SENSOR_C .pane-close-btn')).toBeVisible();
    await page.getByRole('button', { name: 'DevB', exact: true }).click();
    await expect(page.locator('#pane-SENSOR_A .pane-close-btn')).toBeHidden();
    await page.getByRole('button', { name: 'DevA', exact: true }).click();
    await openTabMenu(page, 'DevA');
    await expect(page.getByRole('button', { name: 'Reset layout' })).toBeVisible();

    await page.reload();
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
    await expect.poll(() => visiblePaneNames(page)).toEqual(['AUX', 'HOST']);
  });

  test('a hidden pane can be added back in an exported selection', async ({ page, browser }, testInfo) => {
    await page.locator('#pane-SENSOR_B .pane-close-btn').click();
    await expect.poll(() => visiblePaneNames(page)).toEqual(['DEVICE_A']);
    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });
    const download = page.waitForEvent('download');
    await page.evaluate(() => document.getElementById('export-html-SENSOR_A').click());
    const file = await saveDownload(await download, testInfo);
    expect(fs.readFileSync(file, 'utf-8')).toContain('id="pane-SENSOR_B"');

    const exported = await openHtmlFile(browser, file);
    await exported.getByRole('button', { name: 'DevA', exact: true }).hover();
    await pickFromMenu(exported, 'Add a pane to DevA', 'SENSOR_B');
    await expect.poll(() => visiblePaneNames(exported)).toEqual(['DEVICE_A', 'HOST']);
    await expect(exported.locator('#pane-SENSOR_B')).toBeVisible();
    await exported.close();
  });

  test('the tab menu edits nothing in UNWRAP mode, where a tab is a pane and not a view', async ({ page }) => {
    await page.getByRole('button', { name: /^(Unwrap|Split tabs)$/ }).click();
    // DevA = [DEVICE_A, HOST], DevB = [AUX]: hovering the tab of HOST (the second pane) used to
    // open the menu of the second view and let ✕ hide AUX.
    for (const name of ['DEVICE_A-DevA', 'HOST-DevA', 'AUX-DevB']) {
      await page.getByRole('button', { name, exact: true }).hover();
      await page.waitForTimeout(150);
      await expect(page.locator('#tab-swap-menu.open')).toHaveCount(0);
    }
    // Nothing was hidden: still the same single pane on screen, AUX untouched.
    await expect.poll(() => visiblePaneNames(page)).toEqual(['DEVICE_A-DevA']);
    await expect(page.locator('#hidden-panes #pane-SENSOR_C')).toHaveCount(0);
    // Back in grouped mode the menu works again, on the right view.
    await page.getByRole('button', { name: /^(Unwrap|Split tabs|Group tabs)$/ }).click();
    await openTabMenu(page, 'DevB');
    await expect(page.locator('#tab-swap-menu .tab-swap-title')).toHaveText('Panes in “DevB”');
  });
});
