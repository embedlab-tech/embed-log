import { expect, test } from '@playwright/test';
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import dgram from 'node:dgram';

import { collectPageErrors, waitForLineContaining } from './helpers.js';

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, '../..');

function getFreeTcpPort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.on('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address();
      server.close(err => err ? reject(err) : resolve(port));
    });
  });
}

function getFreeUdpPort() {
  return new Promise((resolve, reject) => {
    const socket = dgram.createSocket('udp4');
    socket.on('error', reject);
    socket.bind(0, '127.0.0.1', () => {
      const { port } = socket.address();
      socket.close();
      resolve(port);
    });
  });
}

function waitForServer(url, timeoutMs = 30_000) {
  const deadline = Date.now() + timeoutMs;
  return new Promise((resolve, reject) => {
    const tryOnce = () => {
      const req = http.get(url, res => {
        res.resume();
        if (res.statusCode && res.statusCode < 500) {
          resolve();
          return;
        }
        if (Date.now() >= deadline) {
          reject(new Error(`server did not become ready: ${url}`));
          return;
        }
        setTimeout(tryOnce, 250);
      });
      req.on('error', () => {
        if (Date.now() >= deadline) {
          reject(new Error(`server did not become ready: ${url}`));
          return;
        }
        setTimeout(tryOnce, 250);
      });
    };
    tryOnce();
  });
}

function terminateChild(child) {
  if (!child) return Promise.resolve();
  child.kill('SIGTERM');
  if (child.exitCode !== null) return Promise.resolve();
  return new Promise(resolve => child.once('exit', resolve));
}

// Scenario: Deterministic demo traffic generates a readable CoAP hex line for the plugin-enabled pane.
//   Given a pane configured with the built-in hex-coap plugin
//   When  deterministic_demo_traffic emits its coap-demo message for SENSOR_A
//   Then  the frontend annotates that line with the decoded CoAP request summary.
test('deterministic demo traffic drives the CoAP pane plugin', async ({ page }) => {
  const httpPort = await getFreeTcpPort();
  const udpPort = await getFreeUdpPort();
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'embed-log-det-demo-'));
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

  const server = spawn('uv', ['run', 'python', 'backend/server.py', 'run', '--config', configPath, '--no-open-browser'], {
    cwd: repoRoot,
    stdio: ['ignore', 'pipe', 'pipe'],
    env: { ...process.env, PYTHONUNBUFFERED: '1' },
  });
  const traffic = spawn('uv', ['run', 'python', 'utils/deterministic_demo_traffic.py', '--content', 'test', '--udp', `SENSOR_A=127.0.0.1:${udpPort}`, '--tick-ms', '50', '--cycles', '0'], {
    cwd: repoRoot,
    stdio: ['ignore', 'pipe', 'pipe'],
    env: { ...process.env, PYTHONUNBUFFERED: '1' },
  });

  try {
    await waitForServer(baseUrl);
    await page.goto(baseUrl);
    await expect(page.locator('#pane-SENSOR_A')).toBeVisible();

    const coapLine = await waitForLineContaining(page, 'SENSOR_A', 'kind=coap-demo');
    await expect(coapLine).toContainText('AA 55 payload');
    await coapLine.hover();
    await expect(page.locator('#plugin-tooltip')).toBeVisible();
    await expect(page.locator('#plugin-tooltip')).toContainText('CoAP');
    await expect(page.locator('#plugin-tooltip')).toContainText('Type:');
    await expect(page.locator('#plugin-tooltip')).toContainText('Message ID:');
    expect(errors).toEqual([]);
  } finally {
    await terminateChild(traffic);
    await terminateChild(server);
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});
