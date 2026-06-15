import { expect, test } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';

import {
  getFreeTcpPort,
  getFreeUdpPort,
  makeTempDir,
  sendUdpLine,
  spawnRustServer,
  terminateChild,
  waitForLineContaining,
  waitForServer,
} from './helpers.js';

test('throwing line plugin is isolated and raw log rendering continues', async ({ page }) => {
  const httpPort = await getFreeTcpPort();
  const udpPort = await getFreeUdpPort();
  const tmpDir = makeTempDir('embed-log-plugin-isolation-');
  const configPath = path.join(tmpDir, 'embed-log.yml');
  const pluginPath = path.join(tmpDir, 'boom-plugin.js');
  const baseUrl = `http://127.0.0.1:${httpPort}/`;
  const pageErrors = [];

  page.on('pageerror', err => pageErrors.push(String(err)));

  fs.writeFileSync(pluginPath, `
window.EmbedLogPlugins.register({
  apiVersion: 1,
  kind: 'line',
  name: 'boom',
  analyzeLine() {
    throw new Error('intentional plugin failure');
  },
});
`, 'utf-8');

  fs.writeFileSync(configPath, `version: 1
server:
  host: 127.0.0.1
  ws_port: ${httpPort}
  open_browser: false
frontend_plugins:
  boom:
    path: boom-plugin.js
sources:
  - name: TEST_UDP
    label: Test
    type: udp
    port: ${udpPort}
tabs:
  - label: Test
    panes:
      - source: TEST_UDP
        plugins: [boom]
`, 'utf-8');

  const child = spawnRustServer(configPath);

  try {
    await waitForServer(baseUrl);
    await page.goto(baseUrl);
    await expect(page.locator('#pane-TEST_UDP')).toBeVisible();

    await sendUdpLine(udpPort, 'plugin failure should not hide this line');
    await sendUdpLine(udpPort, 'second line still renders after failure');

    await waitForLineContaining(page, 'TEST_UDP', 'plugin failure should not hide this line');
    await waitForLineContaining(page, 'TEST_UDP', 'second line still renders after failure');
    await expect(page.locator('#log-TEST_UDP .log-line')).toHaveCount(2);
    expect(pageErrors).toEqual([]);
  } finally {
    await terminateChild(child);
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});
