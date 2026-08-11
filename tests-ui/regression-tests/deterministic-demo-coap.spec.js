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

// Scenario: UDP traffic is decoded by the source-attached Rust hex-coap parser.
//   Given a source configured with parser.type hex-coap
//   When a textual hexadecimal CoAP request arrives
//   Then every client receives the backend-decoded readable log line.
test('deterministic demo traffic uses backend textual CoAP parsing', async ({ page }) => {
  const httpPort = await getFreeTcpPort();
  const udpPort = await getFreeUdpPort();
  const tmpDir = makeTempDir('embed-log-det-demo-');
  const configPath = path.join(tmpDir, 'embed-log.yml');
  const logDir = path.join(tmpDir, 'logs');
  const baseUrl = `http://127.0.0.1:${httpPort}/`;
  const errors = collectPageErrors(page);

  fs.writeFileSync(configPath, `version: 2
server:
  listen: 127.0.0.1:${httpPort}
logs:
  dir: ${logDir}
sources:
  SENSOR_A:
    label: DEVICE_A
    type: udp
    port: ${udpPort}
    parser:
      type: hex-coap
`, 'utf-8');

  const server = spawnRustServer(configPath, { httpPort });

  try {
    await waitForServer(baseUrl);
    await page.goto(baseUrl);
    await expect(page.locator('#pane-SENSOR_A')).toBeVisible();

    const coapHex = '40 01 12 34 B3 66 6F 6F 03 62 61 72';
    await sendUdpLine(udpPort, `coap rx: frame AA 55 payload ${coapHex}`);

    const coapLine = await waitForLineContaining(page, 'SENSOR_A', '[CoAP]');
    await expect(coapLine).toContainText('coap rx: frame AA 55 payload');
    await expect(coapLine).toContainText('[CoAP] t:CON c:GET i:1234');
    await expect(coapLine).toContainText('Uri-Path: foo');
    await expect(coapLine).toContainText('Uri-Path: bar');
    await expect(coapLine).not.toContainText('40 01 12 34');
    expect(errors).toEqual([]);
  } finally {
    await terminateChild(server);
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});
