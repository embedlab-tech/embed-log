import { expect, test } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';

import {
  collectPageErrors,
  getFreeTcpPort,
  getFreeUdpPort,
  makeTempDir,
  sendUdpLine,
  spawnRustServer,
  terminateChild,
  waitForLineContaining,
  waitForServer,
} from './helpers.js';

// Scenario: UDP traffic drives a CoAP hex decode in the plugin-enabled pane.
//   Given a pane configured with the built-in hex-coap plugin
//   When  a coap-demo message for SENSOR_A is sent over UDP
//   Then  the frontend annotates that line with the decoded CoAP request summary.
test('deterministic demo traffic drives the CoAP pane plugin', async ({ page }) => {
  const httpPort = await getFreeTcpPort();
  const udpPort = await getFreeUdpPort();
  const tmpDir = makeTempDir('embed-log-det-demo-');
  const configPath = path.join(tmpDir, 'embed-log.yml');
  const logDir = path.join(tmpDir, 'logs');
  const baseUrl = `http://127.0.0.1:${httpPort}/`;
  const errors = collectPageErrors(page);

  fs.writeFileSync(configPath, `version: 1
server:
  host: 127.0.0.1
  ws_port: ${httpPort}
  open_browser: false
frontend_plugins:
  hex-coap:
    builtin: hex-coap
logs:
  dir: ${logDir}
sources:
  - name: SENSOR_A
    label: DEVICE_A
    type: udp
    port: ${udpPort}
tabs:
  - label: DevA
    panes:
      - source: SENSOR_A
        plugins: [hex-coap]
`, 'utf-8');

  const server = spawnRustServer(configPath, { httpPort });

  try {
    await waitForServer(baseUrl);
    await page.goto(baseUrl);
    await expect(page.locator('#pane-SENSOR_A')).toBeVisible();

    // Send deterministic UDP traffic including a CoAP demo line.
    const coapHex = '40 01 12 34 B3 66 6F 6F 03 62 61 72';
    const tickMsg = (tick, seq, kind, msg) =>
      `TEST src=SENSOR_A tick=${String(tick).padStart(3, '0')} seq=${String(seq).padStart(4, '0')} kind=${kind} msg="${msg}"`;

    for (let tick = 1; tick <= 5; tick++) {
      await sendUdpLine(udpPort, tickMsg(tick, 1, 'sync', `SENSOR_A synchronized step ${String(tick).padStart(3, '0')}`));
      if (tick === 4) {
        await sendUdpLine(udpPort, tickMsg(tick, 2, 'coap-demo', `coap rx: frame AA 55 payload ${coapHex}`));
      }
    }

    const coapLine = await waitForLineContaining(page, 'SENSOR_A', 'kind=coap-demo');
    await expect(coapLine).toContainText('AA 55 payload');
    await coapLine.hover();
    await expect(page.locator('#plugin-tooltip')).toBeVisible();
    await expect(page.locator('#plugin-tooltip')).toContainText('CoAP');
    await expect(page.locator('#plugin-tooltip')).toContainText('Type:');
    await expect(page.locator('#plugin-tooltip')).toContainText('Message ID:');
    expect(errors).toEqual([]);
  } finally {
    await terminateChild(server);
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});
