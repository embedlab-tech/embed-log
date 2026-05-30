import { expect, test } from '@playwright/test';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { openHtmlFile } from './helpers.js';

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, '../..');
const mergeScript = path.join(repoRoot, 'utils', 'merge_logs.py');

test('merged static replay toggles between relative and absolute timestamps', async ({ browser }) => {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'embed-log-relative-'));
  const logPath = path.join(tmpDir, 'sensor.log');
  const htmlPath = path.join(tmpDir, 'relative.html');

  fs.writeFileSync(
    logPath,
    [
      '[T+00:00:00.000] boot ok',
      '[T+00:00:01.250] tick=001 kind=alpha',
      '[T+00:00:02.500] [TX::UI] ping',
    ].join('\n') + '\n',
    'utf-8',
  );

  execFileSync('python3', [
    mergeScript,
    '--timestamp-mode', 'relative',
    '--first-log-at', '2026-01-01T12:00:00.000+00:00',
    '--tab', 'Demo', 'SENSOR_A=READER', logPath,
    '--output', htmlPath,
  ], {
    cwd: repoRoot,
  });

  const page = await openHtmlFile(browser, htmlPath);
  try {
    await expect(page.locator('#pane-SENSOR_A')).toBeVisible();
    await expect(page.locator('#log-SENSOR_A .log-line').first().locator('.ts')).toHaveText('T+00:00:00.000');
    await expect(page.locator('#log-SENSOR_A')).toContainText('tick=001 kind=alpha');
    await expect(page.locator('#log-SENSOR_A')).toContainText('[TX::UI] ping');

    await page.locator('#btn-settings').click();
    await expect(page.locator('#settings-panel')).toHaveClass(/open/);
    await expect(page.locator('#btn-timestamp-mode')).toHaveText('Relative');

    await page.locator('#btn-timestamp-mode').click();
    await expect(page.locator('#btn-timestamp-mode')).toHaveText('Absolute');
    await expect(page.locator('#log-SENSOR_A .log-line').first().locator('.ts')).toHaveText('01-01 12:00:00.000');
    await expect(page.locator('#log-SENSOR_A .log-line').nth(1).locator('.ts')).toHaveText('01-01 12:00:01.250');

    await page.locator('#btn-timestamp-mode').click();
    await expect(page.locator('#log-SENSOR_A .log-line').first().locator('.ts')).toHaveText('T+00:00:00.000');
  } finally {
    await page.close();
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});

test('relative-only static replay shows hint when absolute origin is unavailable', async ({ browser }) => {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'embed-log-relative-no-origin-'));
  const logPath = path.join(tmpDir, 'sensor.log');
  const htmlPath = path.join(tmpDir, 'relative-no-origin.html');

  fs.writeFileSync(
    logPath,
    [
      '[T+00:00:00.000] boot ok',
      '[T+00:00:01.250] tick=001 kind=alpha',
    ].join('\n') + '\n',
    'utf-8',
  );

  execFileSync('python3', [
    mergeScript,
    '--timestamp-mode', 'relative',
    '--tab', 'Demo', 'SENSOR_A=READER', logPath,
    '--output', htmlPath,
  ], {
    cwd: repoRoot,
  });

  const page = await openHtmlFile(browser, htmlPath);
  try {
    await expect(page.locator('#btn-timestamp-mode')).toBeDisabled();
    await expect(page.locator('#btn-timestamp-mode')).toHaveAttribute('title', 'absolute timestamps are unavailable for the current data');
  } finally {
    await page.close();
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});
