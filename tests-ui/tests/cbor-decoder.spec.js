import { expect, test } from '@playwright/test';
import fs from 'node:fs';
import { collectPageErrors, saveDownload, waitForLineContaining } from './helpers.js';


test.describe('CBOR decoder demo', () => {
  let errors;

  test.beforeEach(async ({ page }) => {
    errors = collectPageErrors(page);
  });

  test.afterEach(async () => {
    expect(errors).toEqual([]);
  });

  test('cbor-tab shows CBOR-decoded lines with key=value pairs', async ({ page }) => {
    await page.goto('/');

    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    // Switch to the CBOR tab
    await page.getByRole('button', { name: 'cbor-tab', exact: true }).click();
    await expect(page.locator('#pane-SENSOR_CBOR .pane-name')).toHaveText('CBOR');

    // Wait for a decoded line — CBOR-generated content contains kind=sync
    const firstLine = await waitForLineContaining(page, 'SENSOR_CBOR', 'kind=sync');
    const text = await firstLine.textContent();

    // Verify the line is human-readable key=value text, not raw CBOR bytes
    expect(text).not.toBeNull();
    expect(text.trim()).not.toBe('');
    // CBOR-decoded output uses key=value pairs — confirm no raw binary chars leak
    expect(text).toMatch(/=/);
    // Should contain src=SENSOR_CBOR as a field
    expect(text).toContain('src=SENSOR_CBOR');
    // Must not contain raw CBOR control bytes (0x00-0x1f range excluding whitespace)
    for (let i = 0; i < text.length; i++) {
      const code = text.charCodeAt(i);
      if (code < 0x20 && code !== 0x09 && code !== 0x0a && code !== 0x0d) {
        expect(code).withContext(`raw control byte ${code} at position ${i} in "${text}"`).toBeGreaterThanOrEqual(0x20);
      }
    }
  });

  test('CBOR-decoded lines appear in exported HTML', async ({ page }, testInfo) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    // Wait for at least one CBOR-decoded line to appear
    await page.getByRole('button', { name: 'cbor-tab', exact: true }).click();
    await waitForLineContaining(page, 'SENSOR_CBOR', 'kind=sync');

    // Export the full session HTML
    const downloadPromise = page.waitForEvent('download');
    await page.locator('#btn-export').click();
    const download = await downloadPromise;

    expect(download.suggestedFilename()).toMatch(/^embed-log-.*\.html$/);
    const htmlPath = await saveDownload(download, testInfo);

    const html = fs.readFileSync(htmlPath, 'utf-8');

    // The exported HTML must contain CBOR-decoded text
    expect(html).toContain('src=SENSOR_CBOR');
    expect(html).toContain('kind=sync');

    // No raw CBOR binary bytes in the exported HTML
    expect(html).not.toContain('\\u0000');
    expect(html).not.toContain('\\x00');
  });
});
