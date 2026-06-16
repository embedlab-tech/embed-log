import { expect } from '@playwright/test';
import { spawn } from 'node:child_process';
import dgram from 'node:dgram';
import fs from 'node:fs';
import http from 'node:http';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL, fileURLToPath } from 'node:url';

export function tickText(tick) {
  return `tick=${String(tick).padStart(3, '0')}`;
}

export async function waitForTick(page, paneId, tick, timeout = 30_000) {
  const text = tickText(tick);
  return waitForLineContaining(page, paneId, text, timeout);
}

export async function waitForLineContaining(page, paneId, text, timeout = 30_000) {
  const line = page.locator(`#log-${paneId} .log-line`, { hasText: text }).first();
  await expect(line, `${paneId} should contain ${text}`).toBeVisible({ timeout });
  return line;
}

export async function waitForSourceTestLine(page, paneId, timeout = 30_000) {
  return waitForLineContaining(page, paneId, `TEST src=${paneId}`, timeout);
}

export async function lineTick(locator) {
  const text = await locator.textContent();
  const m = text?.match(/tick=(\d{3})/);
  if (!m) throw new Error(`line has no tick: ${text}`);
  return m[1];
}

export async function waitForRangePair(page, paneId, startText, endText, timeout = 30_000) {
  await waitForLineContaining(page, paneId, startText, timeout);
  await expect.poll(async () => {
    return page.locator(`#log-${paneId} .log-line`).evaluateAll((nodes, args) => {
      const [start, end] = args;
      const startIdx = nodes.findIndex(n => n.textContent.includes(start));
      if (startIdx < 0) return false;
      return nodes.slice(startIdx + 1).some(n => n.textContent.includes(end));
    }, [startText, endText]);
  }, { timeout }).toBe(true);

  const lines = page.locator(`#log-${paneId} .log-line`);
  const indices = await lines.evaluateAll((nodes, args) => {
    const [start, end] = args;
    const startIdx = nodes.findIndex(n => n.textContent.includes(start));
    const endRel = nodes.slice(startIdx + 1).findIndex(n => n.textContent.includes(end));
    return [startIdx, startIdx + 1 + endRel];
  }, [startText, endText]);
  return { start: lines.nth(indices[0]), end: lines.nth(indices[1]), indices };
}

export async function visiblePaneNames(page) {
  return page.locator('.tab-content:visible .pane-name').evaluateAll(nodes =>
    nodes.map(n => n.textContent.trim())
  );
}

export async function selectedLineTicks(page, paneId) {
  return page.locator(`#log-${paneId} .log-line.selected`).evaluateAll(nodes =>
    nodes.map(n => {
      const m = n.textContent.match(/tick=(\d{3})/);
      return m ? m[1] : null;
    }).filter(Boolean)
  );
}

export async function saveDownload(download, testInfo, filename) {
  const out = testInfo.outputPath(filename || download.suggestedFilename());
  await download.saveAs(out);
  return out;
}

export async function openHtmlFile(browser, filePath) {
  const page = await browser.newPage({ acceptDownloads: true });
  await page.goto(pathToFileURL(path.resolve(filePath)).href);
  return page;
}

export function collectPageErrors(page) {
  const errors = [];
  page.on('pageerror', err => errors.push(String(err)));
  page.on('console', msg => {
    if (msg.type() === 'error') errors.push(msg.text());
  });
  return errors;
}

const _here = path.dirname(fileURLToPath(import.meta.url));
const _repoRoot = path.resolve(_here, '../..');

export function getFreeTcpPort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.on('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address();
      server.close(err => err ? reject(err) : resolve(port));
    });
  });
}

export function getFreeUdpPort() {
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

export function waitForServer(url, timeoutMs = 30_000) {
  const deadline = Date.now() + timeoutMs;
  return new Promise((resolve, reject) => {
    const tryOnce = () => {
      const req = http.get(url, res => {
        res.resume();
        if (res.statusCode && res.statusCode < 500) { resolve(); return; }
        if (Date.now() >= deadline) { reject(new Error(`server did not become ready: ${url}`)); return; }
        setTimeout(tryOnce, 250);
      });
      req.on('error', () => {
        if (Date.now() >= deadline) { reject(new Error(`server did not become ready: ${url}`)); return; }
        setTimeout(tryOnce, 250);
      });
    };
    tryOnce();
  });
}

export function sendUdpLine(port, text) {
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

export function spawnRustServer(configPath, { httpPort } = {}) {
  const args = [
    'run', '--quiet', '--package', 'embed-log-cli', '--bin', 'embed-log', '--',
    'run', '--config', configPath,
    '--frontend-dir', 'frontend',
    '--no-open-browser',
  ];
  return spawn('cargo', args, {
    cwd: _repoRoot,
    stdio: ['ignore', 'pipe', 'pipe'],
    env: { ...process.env, RUST_LOG: process.env.RUST_LOG || 'warn' },
  });
}

export function terminateChild(child) {
  if (!child) return Promise.resolve();
  child.kill('SIGTERM');
  if (child.exitCode !== null) return Promise.resolve();
  return new Promise(resolve => child.once('exit', resolve));
}

export function makeTempDir(prefix = 'embed-log-test-') {
  return fs.mkdtempSync(path.join(os.tmpdir(), prefix));
}

export function writeConfig(configPath, yaml) {
  fs.writeFileSync(configPath, yaml, 'utf-8');
}


