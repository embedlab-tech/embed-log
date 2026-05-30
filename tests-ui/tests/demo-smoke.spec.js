import { expect, test } from '@playwright/test';
import fs from 'node:fs';
import dgram from 'node:dgram';
import { collectPageErrors, saveDownload, waitForLineContaining, waitForRangePair, waitForSourceTestLine } from './helpers.js';

async function openMore(page, paneId) {
  await page.locator(`#more-toggle-${paneId}`).click({ force: true });
}
async function sendUdpBurst(port, prefix, count = 220) {
  const socket = dgram.createSocket('udp4');
  const payload = Array.from({ length: count }, (_, i) => `${prefix}-${String(i).padStart(3, '0')}`).join('\n') + '\n';
  await new Promise((resolve, reject) => {
    socket.send(Buffer.from(payload, 'utf-8'), port, '127.0.0.1', err => {
      socket.close();
      if (err) reject(err);
      else resolve();
    });
  });
}


test.describe('embed-log deterministic demo smoke', () => {
  let errors;

  test.beforeEach(async ({ page }) => {
    errors = collectPageErrors(page);
  });

  test.afterEach(async () => {
    expect(errors).toEqual([]);
  });

  test('connects to backend and receives deterministic demo logs', async ({ page }) => {
    await page.goto('/');

    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
    await expect(page.locator('#pane-SENSOR_A')).toBeVisible();
    await expect(page.locator('#pane-SENSOR_B')).toBeVisible();
    await expect(page.locator('#pane-SENSOR_C')).toBeAttached();
    await expect(page.locator('#pane-SENSOR_A .pane-name')).toHaveText('READER');
    await expect(page.locator('#pane-SENSOR_B .pane-name')).toHaveText('CONTROLLER');

    await waitForSourceTestLine(page, 'SENSOR_A');
    await waitForSourceTestLine(page, 'SENSOR_B');

    await page.getByRole('button', { name: 'DevB', exact: true }).click();
    await expect(page.locator('#pane-SENSOR_C .pane-name')).toHaveText('READER');
    await waitForSourceTestLine(page, 'SENSOR_C');

    await page.getByRole('button', { name: 'cbor-tab', exact: true }).click();
    await expect(page.locator('#pane-SENSOR_CBOR .pane-name')).toHaveText('CBOR');
    await waitForLineContaining(page, 'SENSOR_CBOR', 'kind=sync');
  });

  test('startup does not depend on external network assets', async ({ page }) => {
    const requests = [];
    page.on('request', request => requests.push(request.url()));

    await page.goto('/');

    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
    await expect(page.locator('#pane-SENSOR_A')).toBeVisible();

    const origin = new URL(page.url()).origin;
    const externalRequests = requests.filter(url => {
      if (!url.startsWith('http://') && !url.startsWith('https://')) return false;
      return new URL(url).origin !== origin;
    });
    expect(externalRequests).toEqual([]);
  });

  test('per-pane download button triggers raw .log download', async ({ page }, testInfo) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
    await waitForSourceTestLine(page, 'SENSOR_A');

    const downloadPromise = page.waitForEvent('download');
    await page.locator('#pane-SENSOR_A .pane-download-btn').click();
    const download = await downloadPromise;

    expect(download.suggestedFilename()).toBe('SENSOR_A.log');
    const saved = await saveDownload(download, testInfo);
    const text = fs.readFileSync(saved, 'utf-8');
    expect(text).toContain('TEST src=SENSOR_A');
    expect(text).toContain('kind=sync');
  });

  test('shift-click selects a deterministic range and per-pane download still works', async ({ page }, testInfo) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
    await waitForSourceTestLine(page, 'SENSOR_A');

    // Select a range
    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });

    await expect.poll(async () => page.locator('#log-SENSOR_A .log-line.selected').count())
      .toBeGreaterThanOrEqual(2);

    // Download full pane log via the per-pane Download button
    const downloadPromise = page.waitForEvent('download');
    await page.locator('#pane-SENSOR_A .pane-download-btn').click();
    const download = await downloadPromise;

    expect(download.suggestedFilename()).toBe('SENSOR_A.log');
    const downloadedPath = await saveDownload(download, testInfo);

    const text = fs.readFileSync(downloadedPath, 'utf-8');
    expect(text).toContain('[SENSOR_A]');
    expect(text).toContain('kind=prefix-cleanup');
  });

  test('HTML snippet uses the regular embed-log exported UI', async ({ page }, testInfo) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });
    const downloadPromise = page.waitForEvent('download');
    await page.evaluate(() => {
      document.getElementById('export-html-SENSOR_A').click();
    });
    const download = await downloadPromise;

    expect(download.suggestedFilename()).toMatch(/^embed-log-exact-.*\.html$/);
    const downloadedPath = await saveDownload(download, testInfo);

    const html = fs.readFileSync(downloadedPath, 'utf-8');
    expect(html).toContain('<div id="toolbar">');
    expect(html).toContain('<div id="tab-bar"></div>');
    expect(html).toContain('var _logData =');
    expect(html).toContain('kind=prefix-cleanup');
    expect(html).toMatch(/\[SENSOR_A\]/);
    expect(html).not.toContain('<h1>embed-log snippet</h1>');
  });

test('live DOM keeps full pane history while tailing', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
  await waitForSourceTestLine(page, 'SENSOR_A');
  await waitForSourceTestLine(page, 'SENSOR_B');

  const firstA = (await page.locator('#log-SENSOR_A .log-line').first().textContent())?.trim();
  const firstB = (await page.locator('#log-SENSOR_B .log-line').first().textContent())?.trim();
  expect(firstA).toBeTruthy();
  expect(firstB).toBeTruthy();

  await Promise.all([
    sendUdpBurst(6000, 'burst-a'),
    sendUdpBurst(6001, 'burst-b'),
  ]);

  await expect.poll(async () => page.locator('#log-SENSOR_A .log-line').count()).toBeGreaterThan(200);
  await expect.poll(async () => page.locator('#log-SENSOR_B .log-line').count()).toBeGreaterThan(200);

  await expect(page.locator('#log-SENSOR_A')).toContainText(firstA);
  await expect(page.locator('#log-SENSOR_A')).toContainText('burst-a-219');
  await expect(page.locator('#log-SENSOR_B')).toContainText(firstB);
  await expect(page.locator('#log-SENSOR_B')).toContainText('burst-b-219');
});

test('runtime settings panel exposes working font-size controls', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
  await waitForSourceTestLine(page, 'SENSOR_A');

  await page.locator('#btn-settings').click();
  await expect(page.locator('#settings-panel')).toHaveClass(/open/);
  await expect(page.locator('#btn-font-dec')).toBeVisible();
  await expect(page.locator('#btn-font-reset')).toBeVisible();
  await expect(page.locator('#btn-font-inc')).toBeVisible();

  const line = page.locator('#log-SENSOR_A .log-line').first();
  const before = await line.evaluate(el => getComputedStyle(el).fontSize);

  await page.locator('#btn-font-inc').click();
  await expect.poll(async () => {
    return line.evaluate(el => getComputedStyle(el).fontSize);
  }).not.toBe(before);

  await page.locator('#btn-font-reset').click();
  await expect.poll(async () => {
    return line.evaluate(el => getComputedStyle(el).fontSize);
  }).toBe(before);
});

  test('marker rendering, tooltip, and navigation', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
    await waitForSourceTestLine(page, 'SENSOR_A');

    // Pick a log line index to mark
    const markerLineIdx = await page.evaluate(() => {
      const logEl = document.getElementById('log-SENSOR_A');
      if (!logEl || !logEl.children.length) return -1;
      const idx = Math.min(2, logEl.children.length - 1);
      return parseInt(logEl.children[idx].dataset.idx, 10);
    });
    expect(markerLineIdx).toBeGreaterThanOrEqual(0);

    // Send a save_markers WS command — the server broadcasts markers_update back
    await page.evaluate((idx) => {
      window.wsSend({
        cmd: 'save_markers',
        markers: [{
          paneId: 'SENSOR_A',
          lineIdx: idx,
          endIdx: idx,
          numTs: 0,
          description: 'Test marker description',
          createdAt: new Date().toISOString(),
        }],
      });
    }, markerLineIdx);

    // Wait for markers_update to be received and processed
    await expect(page.locator('#marker-nav')).not.toBeHidden({ timeout: 15_000 });
    await expect(page.locator('#marker-nav-total')).toHaveText('1');

    // Check that the marked line has the has-marker CSS class
    await expect(
      page.locator(`#log-SENSOR_A [data-idx="${markerLineIdx}"]`)
    ).toHaveClass(/has-marker/);

    // Check that the tooltip appears on hover
    const lineLocator = page.locator(`#log-SENSOR_A [data-idx="${markerLineIdx}"]`);
    await lineLocator.hover();
    await expect(page.locator('#marker-tooltip')).toBeVisible();
    await expect(page.locator('#marker-tooltip')).toContainText('Test marker description');

    // Check that navigation buttons work
    await page.locator('#marker-nav-next').click();
    await expect(page.locator('#marker-nav-idx')).toHaveText('1');

    // Remove the marker
    await page.evaluate(() => {
      window.wsSend({ cmd: 'save_markers', markers: [] });
    });

    // Wait for markers_update with empty list
    await expect(page.locator('#marker-nav')).toBeHidden({ timeout: 15_000 });
  });

});

