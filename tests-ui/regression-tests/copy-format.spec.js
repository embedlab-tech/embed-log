import { expect, test } from '@playwright/test';
import { collectPageErrors, waitForRangePair } from './helpers.js';

async function readClipboard(page) {
  return page.evaluate(() => navigator.clipboard.readText());
}

test.describe('copy format levels', () => {
  let errors;

  test.beforeEach(async ({ page, context }) => {
    errors = collectPageErrors(page);
    await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  });

  test.afterEach(async () => {
    expect(errors).toEqual([]);
  });

  // Scenario: default ("Full") copy format is unchanged from today's behavior
  //   Given the user selects a range in SENSOR_A without touching the format picker
  //   When  they copy
  //   Then  the clipboard contains the full source name (backward compatibility)

  test('default Full format still contains the full source name', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });

    await page.locator('#copy-SENSOR_A').click();
    const copied = await readClipboard(page);
    expect(copied).toContain('SENSOR_A');
  });

  // Scenario: Compact format denoises and shortcodes, no full source name
  //   Given the user selects a range and switches to Compact
  //   When  they copy
  //   Then  each line starts with an elapsed-time-shaped prefix and a short source code,
  //         and the full source name is gone

  test('Compact format uses elapsed time and a source shortcode', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });

    await page.locator('#format-compact-SENSOR_A').click();
    await page.locator('#copy-SENSOR_A').click();
    const copied = (await readClipboard(page)).trimEnd();

    // Check the structural prefix specifically, not the whole line — the
    // fixture's own message *content* legitimately contains "SENSOR_A"
    // (that's what the kind=prefix-cleanup fixture exercises), so a blanket
    // "doesn't contain SENSOR_A" assertion would be wrong. The prefix itself
    // must use the shortcode, not the full name.
    for (const line of copied.split('\n')) {
      expect(line).toMatch(/^\d+(:\d{2}){0,2}\.\d{3} [A-Z]\d*(#\d+)? /);
      expect(line).not.toMatch(/^\d+(:\d{2}){0,2}\.\d{3} SENSOR_A/);
    }
  });

  // Scenario: JSON format produces valid JSONL with short keys
  //   Given the user selects a range and switches to JSON
  //   When  they copy
  //   Then  every line parses as JSON with t/s/m keys

  test('JSON format produces one valid JSON object per line', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });

    await page.locator('#format-json-SENSOR_A').click();
    await page.locator('#copy-SENSOR_A').click();
    const copied = (await readClipboard(page)).trimEnd();

    const lines = copied.split('\n');
    expect(lines.length).toBeGreaterThan(0);
    for (const line of lines) {
      const obj = JSON.parse(line);
      expect(obj).toHaveProperty('t');
      expect(obj).toHaveProperty('s');
      expect(obj).toHaveProperty('m');
    }
  });

  // Scenario: Copy button shows a live token estimate once a selection exists
  //   Given the user selects a range
  //   Then  the Copy button label includes an approximate token count

  test('Copy button shows a token count estimate', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });

    await expect(page.locator('#copy-SENSOR_A')).toHaveText(/~\d+ tok/);
  });
});
