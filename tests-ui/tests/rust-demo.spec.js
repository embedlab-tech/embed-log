import { expect, test } from '@playwright/test';
import dgram from 'node:dgram';
import fs from 'node:fs';
import { collectPageErrors, openHtmlFile, saveDownload, waitForLineContaining, waitForWs } from './helpers.js';

async function sendUdp(port, payload) {
  const socket = dgram.createSocket('udp4');
  await new Promise((resolve, reject) => {
    socket.send(Buffer.from(payload), port, '127.0.0.1', err => {
      socket.close();
      err ? reject(err) : resolve();
    });
  });
}

function cborText(text) {
  const bytes = Buffer.from(text, 'utf8');
  if (bytes.length >= 24) throw new Error('test text too long');
  return Buffer.concat([Buffer.from([0x60 | bytes.length]), bytes]);
}

function cborUint(n) {
  if (n < 24) return Buffer.from([n]);
  return Buffer.from([0x18, n]);
}

function cborMap(entries) {
  return Buffer.concat([
    Buffer.from([0xa0 | entries.length]),
    ...entries.flatMap(([key, value]) => [cborText(key), typeof value === 'number' ? cborUint(value) : cborText(value)]),
  ]);
}

test.describe('Rust backend browser e2e', () => {
  let errors;

  test.beforeEach(async ({ page }) => {
    errors = collectPageErrors(page);
  });

  test.afterEach(async () => {
    expect(errors).toEqual([]);
  });

  test('connects to Rust backend, builds panes, and renders UDP logs', async ({ page }) => {
    await page.goto('/');
    await waitForWs(page);

    await expect(page.locator('#pane-DUT .pane-name')).toHaveText('DUT UART');
    await expect(page.locator('#pane-HOST .pane-name')).toHaveText('Host Debug');

    await sendUdp(16000, 'E2E DUT boot\n');
    await sendUdp(16001, 'E2E HOST ready\n');

    await waitForLineContaining(page, 'DUT', 'E2E DUT boot');
    await waitForLineContaining(page, 'HOST', 'E2E HOST ready');
  });

  test('restores a full cached pane promptly and renders rapid virtual scrolls', async ({ page }) => {
    await page.goto('/');
    await waitForWs(page);
    await expect(page.locator('#pane-DUT .pane-name')).toHaveText('DUT UART');

    await page.evaluate(() => {
      const sessionId = localStorage.getItem('embed-log:last-session-id');
      const key = `embed-log:session:${sessionId}:v1`;
      const marker = 'E2E cached refresh marker 1499';
      localStorage.setItem(key, JSON.stringify({
        tabs: [],
        activeTab: 0,
        fontSize: 14,
        timestampMode: 'absolute',
        panePluginUiState: {},
        lines: {
          DUT: Array.from({ length: 1500 }, (_, index) => ({
            ts: `12:00:${String(index % 60).padStart(2, '0')}`,
            text: index === 1499 ? marker : `E2E cached refresh row ${index}`,
            isTx: false,
          })),
        },
        savedAt: Date.now(),
      }));

      // Do not let beforeunload overwrite the seeded full cache.
      const setItem = Storage.prototype.setItem;
      Storage.prototype.setItem = function (name, value) {
        if (name === key) return;
        return setItem.call(this, name, value);
      };
    });

    const startedAt = Date.now();
    await page.reload();
    await expect(page.locator('#log-DUT')).toContainText('E2E cached refresh marker 1499');
    expect(Date.now() - startedAt).toBeLessThan(4_000);

    const renderedViewport = await page.evaluate(async () => {
      const log = document.getElementById('log-DUT');
      const maxScroll = log.scrollHeight - log.clientHeight;
      if (maxScroll <= 0) return { hasVisibleRow: false, renderedRows: 0 };

      // Queue a render for an old position, then make a large second jump
      // before rAF runs. The renderer must use this final scroll position.
      log.scrollTop = maxScroll * 0.1;
      log.dispatchEvent(new Event('scroll'));
      log.scrollTop = maxScroll * 0.8;
      log.dispatchEvent(new Event('scroll'));

      const hasVisibleRow = () => {
        const viewport = log.getBoundingClientRect();
        return [...log.querySelectorAll('.log-line')].some(row => {
          const bounds = row.getBoundingClientRect();
          return bounds.bottom > viewport.top && bounds.top < viewport.bottom;
        });
      };
      // The viewport is already outside the prior virtual range. It must be
      // populated in this scroll event, not only in a later animation frame.
      const hasImmediateVisibleRow = hasVisibleRow();
      await new Promise(requestAnimationFrame);
      await new Promise(requestAnimationFrame);

      return {
        renderedRows: log.querySelectorAll('.log-line').length,
        hasImmediateVisibleRow,
        hasVisibleRow: hasVisibleRow(),
      };
    });
    expect(renderedViewport.renderedRows).toBeGreaterThan(0);
    expect(renderedViewport.hasImmediateVisibleRow).toBe(true);
    expect(renderedViewport.hasVisibleRow).toBe(true);
  });

  test('decodes CBOR datagrams in the browser pane', async ({ page }) => {
    await page.goto('/');
    await waitForWs(page);
    await page.getByRole('button', { name: 'Sensors', exact: true }).click();

    await sendUdp(16002, cborMap([['kind', 'sync'], ['seq', 7]]));

    await waitForLineContaining(page, 'SENSORS', 'kind=sync');
    await waitForLineContaining(page, 'SENSORS', 'seq=7');
  });

  test('session export produces replayable HTML', async ({ page, browser }, testInfo) => {
    await page.goto('/');
    await waitForWs(page);
    await sendUdp(16000, 'E2E export marker\n');
    await waitForLineContaining(page, 'DUT', 'E2E export marker');

    const result = await page.evaluate(async () => {
      const response = await fetch('/api/session/export', { method: 'POST' });
      return response.json();
    });
    expect(result.ok).toBe(true);
    expect(result.session.id).toBeTruthy();

    const downloadPromise = page.waitForEvent('download');
    await page.locator('#pane-DUT .pane-download-btn').click();
    const download = await downloadPromise;
    const rawPath = await saveDownload(download, testInfo);
    expect(fs.readFileSync(rawPath, 'utf8')).toContain('E2E export marker');

    const replay = await openHtmlFile(browser, result.html_path);
    await expect(replay.locator('#log-DUT')).toContainText('E2E export marker');
    await replay.close();
  });

  test('session rotation clears panes and routes new logs to the new session', async ({ page }) => {
    await page.goto('/');
    await waitForWs(page);
    await sendUdp(16000, 'E2E before rotate\n');
    await waitForLineContaining(page, 'DUT', 'E2E before rotate');

    const rotated = await page.evaluate(async () => {
      const response = await fetch('/api/session/rotate', { method: 'POST' });
      return response.json();
    });
    expect(rotated.ok).toBe(true);
    expect(rotated.old_session.id).not.toBe(rotated.session.id);

    await expect(page.locator('#log-DUT')).not.toContainText('E2E before rotate');
    await sendUdp(16000, 'E2E after rotate\n');
    await waitForLineContaining(page, 'DUT', 'E2E after rotate');
  });
});
