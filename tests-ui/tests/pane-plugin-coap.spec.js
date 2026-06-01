import { expect, test } from '@playwright/test';
import { spawn } from 'node:child_process';
import dgram from 'node:dgram';
import fs from 'node:fs';
import http from 'node:http';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { collectPageErrors } from './helpers.js';

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, '../..');
const coapHex = '40011234B3666F6F03626172';

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

// Scenario: A pane-local line plugin decodes CoAP hex embedded inside noisy UDP text.
//   Given a UDP pane configured with the built-in hex-coap plugin
//   When  log lines contain extra words/spaces plus arbitrary leading hex bytes before the packet
//   Then  the frontend still finds the CoAP message inside the line and annotates only matching lines.
test('live pane plugin decodes CoAP hex strings inside noisy UDP lines', async ({ page }) => {
  const httpPort = await getFreeTcpPort();
  const udpPort = await getFreeUdpPort();
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'embed-log-pane-plugin-'));
  const configPath = path.join(tmpDir, 'embed-log.yml');
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
sources:
  - name: COAP_UDP
    label: COAP
    type: udp
    port: ${udpPort}
tabs:
  - label: CoAP
    panes:
      - source: COAP_UDP
        plugins:
          - name: hex-coap
`, 'utf-8');

  const child = spawn('uv', ['run', 'python', '-m', 'backend.server', 'run', '--config', configPath, '--no-open-browser'], {
    cwd: repoRoot,
    stdio: ['ignore', 'pipe', 'pipe'],
    env: { ...process.env, PYTHONUNBUFFERED: '1' },
  });
  const output = [];
  child.stdout.on('data', chunk => output.push(String(chunk)));
  child.stderr.on('data', chunk => output.push(String(chunk)));

  try {
    await waitForServer(baseUrl);
    await page.goto(baseUrl);
    await expect(page.locator('#pane-COAP_UDP')).toBeVisible();

    await sendUdpLine(udpPort, `prefix words AABBCC ${coapHex} suffix words`);
    await sendUdpLine(udpPort, `prefix bytes 99 88 77 66 40 01 12 34 B3 66 6F 6F 03 62 61 72 suffix`);
    await sendUdpLine(udpPort, 'plain log without any packet');

    const lines = page.locator('#log-COAP_UDP .log-line');
    await expect(lines).toHaveCount(3);

    const compactMatch = lines.filter({ hasText: 'AABBCC' }).first();
    await compactMatch.hover();
    await expect(page.locator('#plugin-tooltip')).toBeVisible();
    await expect(page.locator('#plugin-tooltip')).toContainText('CoAP — GET /foo/bar');
    await expect(page.locator('#plugin-tooltip')).toContainText('Type: CON');
    await expect(page.locator('#plugin-tooltip')).toContainText('Message ID: 0x1234');

    const spacedMatch = lines.filter({ hasText: '99 88 77 66' }).first();
    await spacedMatch.hover();
    await expect(page.locator('#plugin-tooltip')).toContainText('CoAP — GET /foo/bar');
    await expect(page.locator('#plugin-tooltip')).toContainText('Uri-Path: foo');
    await expect(page.locator('#plugin-tooltip')).toContainText('Uri-Path: bar');

    const plainLine = lines.filter({ hasText: 'plain log without any packet' }).first();
    await plainLine.hover();
    await expect(page.locator('#plugin-tooltip')).toBeHidden();

    expect(errors).toEqual([]);
  } finally {
    child.kill('SIGTERM');
    if (child.exitCode === null) {
      await new Promise(resolve => child.once('exit', resolve));
    }
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});
