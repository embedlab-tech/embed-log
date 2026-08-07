import { expect, test } from '@playwright/test';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { openHtmlFile } from './helpers.js';

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, '../..');
function runRecordedExport({ tmpDir, logPath, htmlPath, firstLogAt = null }) {
  const logsDir = path.join(tmpDir, 'sessions');
  const sessionId = 'relative-replay';
  const sessionDir = path.join(logsDir, sessionId);
  fs.mkdirSync(sessionDir, { recursive: true });
  fs.writeFileSync(path.join(sessionDir, 'manifest.json'), JSON.stringify({
    session_id: sessionId,
    session_dir: sessionDir,
    timestamp_mode: 'relative',
    first_log_at: firstLogAt,
    tabs: [{ label: 'Demo', panes: ['SENSOR_A'] }],
    pane_labels: { SENSOR_A: 'SENSOR' },
    source_files: { SENSOR_A: logPath },
    combined_file: path.join(sessionDir, 'combined.jsonl'),
  }, null, 2));
  execFileSync('cargo', [
    'run', '--quiet', '--package', 'embed-log-cli', '--bin', 'embed-log', '--',
    'sessions', 'export', sessionId, '--dir', logsDir,
    '--format', 'html', '--output', htmlPath,
  ], { cwd: repoRoot });
}

// Scenario: Merged static replay cycles relative, hidden, and absolute timestamps
//   Given a static replay with merged log data and a known absolute origin
//   When  the user clicks the timestamp mode toggle
//   Then  timestamps cycle relative → hidden → absolute → relative
//
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

  runRecordedExport({
    tmpDir,
    logPath,
    htmlPath,
    firstLogAt: '2026-01-01T12:00:00.000+00:00',
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
    await expect(page.locator('#btn-timestamp-mode')).toHaveText('No time');
    await expect(page.locator('#log-SENSOR_A .log-line').first().locator('.ts')).toBeHidden();

    await page.locator('#btn-timestamp-mode').click();
    await expect(page.locator('#btn-timestamp-mode')).toHaveText('Absolute');
    await expect(page.locator('#log-SENSOR_A .log-line').first().locator('.ts')).toHaveText('01-01 12:00:00.000');
    await expect(page.locator('#log-SENSOR_A .log-line').nth(1).locator('.ts')).toHaveText('01-01 12:00:01.250');

    await page.locator('#btn-timestamp-mode').click();
    await expect(page.locator('#btn-timestamp-mode')).toHaveText('Relative');
    await expect(page.locator('#log-SENSOR_A .log-line').first().locator('.ts')).toHaveText('T+00:00:00.000');
  } finally {
    await page.close();
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});

// Scenario: Relative-only static replay can hide timestamps but cannot switch to absolute
//   Given a static replay with relative timestamps but no --first-log-at origin
//   When  the user switches to No time
//   Then  the button is disabled with a title explaining absolute mode is unavailable
//
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

  runRecordedExport({ tmpDir, logPath, htmlPath });

  const page = await openHtmlFile(browser, htmlPath);
  try {
    await expect(page.locator('#btn-timestamp-mode')).toHaveText('Relative');
    await expect(page.locator('#btn-timestamp-mode')).toBeEnabled();
    await page.locator('#btn-timestamp-mode').click();
    await expect(page.locator('#btn-timestamp-mode')).toHaveText('No time');
    await expect(page.locator('#btn-timestamp-mode')).toBeDisabled();
    await expect(page.locator('#btn-timestamp-mode')).toHaveAttribute('title', 'absolute timestamps are unavailable for the current data');
  } finally {
    await page.close();
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});
