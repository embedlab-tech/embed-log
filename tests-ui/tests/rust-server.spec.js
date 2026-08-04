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
    await expect(page.locator('#btn-clear')).toHaveCount(0);
    await expect(page.locator('#btn-new-session')).toHaveCount(0);
    await expect(page.locator('#btn-sessions')).toHaveCount(0);
    await expect(page.locator('#btn-save-to-server')).toHaveCount(0);

    await sendUdp(16000, 'E2E DUT boot\n');
    await sendUdp(16001, 'E2E HOST ready\n');

    await waitForLineContaining(page, 'DUT', 'E2E DUT boot');
    await waitForLineContaining(page, 'HOST', 'E2E HOST ready');
  });

  test('renders text UDP datagrams in the sensor pane', async ({ page }) => {
    await page.goto('/');
    await waitForWs(page);
    await page.getByRole('button', { name: 'Sensors', exact: true }).click();

    await sendUdp(16002, 'kind=sync seq=7\n');

    await waitForLineContaining(page, 'SENSORS', 'kind=sync');
    await waitForLineContaining(page, 'SENSORS', 'seq=7');
  });

  test('disconnecting the final browser does not export HTML', async ({ page }) => {
    await page.goto('/');
    await waitForWs(page);
    const session = await page.evaluate(async () => (await fetch('/api/session/current')).json());
    expect(session.html_status).toBe('pending');
    await page.close();
    await new Promise(resolve => setTimeout(resolve, 2500));
    const manifest = JSON.parse(fs.readFileSync(session.manifest, 'utf8'));
    expect(manifest.html_status).toBe('pending');
    expect(fs.existsSync(`${session.dir}/session.html`)).toBe(false);
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
      const response = await fetch('/api/session/rotate', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ title: 'Browser Reconnect Attempt #3' }),
      });
      return response.json();
    });
    expect(rotated.ok).toBe(true);
    expect(rotated.old_session.id).not.toBe(rotated.session.id);
    expect(rotated.session.id).toContain('_browser-reconnect-attempt-3');
    expect(rotated.session.title).toBe('Browser Reconnect Attempt #3');

    await expect(page.locator('#log-DUT')).not.toContainText('E2E before rotate');
    await sendUdp(16000, 'E2E after rotate\n');
    await waitForLineContaining(page, 'DUT', 'E2E after rotate');
  });
});
