import { expect, test } from '@playwright/test';
import { spawn } from 'node:child_process';
import dgram from 'node:dgram';
import fs from 'node:fs';
import http from 'node:http';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { waitForLineContaining } from './helpers.js';

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

function sendUdpLine(port, text) {
  return new Promise((resolve, reject) => {
    const socket = dgram.createSocket('udp4');
    const payload = Buffer.from(text + '\n', 'utf-8');
    socket.send(payload, port, '127.0.0.1', err => {
      socket.close();
      if (err) reject(err);
      else resolve();
    });
  });
}

test('throwing line plugin is isolated and raw log rendering continues', async ({ page }) => {
  const httpPort = await getFreeTcpPort();
  const udpPort = await getFreeUdpPort();
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'embed-log-plugin-isolation-'));
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

  const child = spawn('uv', ['run', 'python', '-m', 'backend.server', 'run', '--config', configPath, '--no-open-browser'], {
    cwd: repoRoot,
    stdio: ['ignore', 'pipe', 'pipe'],
    env: { ...process.env, PYTHONUNBUFFERED: '1' },
  });

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
    child.kill('SIGTERM');
    if (child.exitCode === null) {
      await new Promise(resolve => child.once('exit', resolve));
    }
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});
