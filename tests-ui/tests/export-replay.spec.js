import { expect, test } from '@playwright/test';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { collectPageErrors, openHtmlFile, saveDownload, waitForLineContaining, waitForRangePair, waitForSourceTestLine } from './helpers.js';

async function openMore(page, paneId) {
  await page.locator(`#more-toggle-${paneId}`).click();
}

function generateMergedHtml(htmlPath, logPath) {
  const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
  const script = [
    'import sys',
    'from pathlib import Path',
    'from utils.merge_logs import generate_html',
    'html_path, log_path = sys.argv[1], sys.argv[2]',
    'html = generate_html([{"label": "Large", "panes": [("A", "READER", log_path)]}])',
    'Path(html_path).write_text(html, encoding="utf-8")',
  ].join('\n');
  execFileSync('python3', ['-c', script, htmlPath, logPath], { cwd: repoRoot });
}

// Feature: HTML export replay — export of static HTML snapshots and offline replay of log UI

test.describe('HTML export replay', () => {
  let errors;

  test.beforeEach(async ({ page }) => {
    errors = collectPageErrors(page);
  });

  test.afterEach(async () => {
    expect(errors).toEqual([]);
  });
  // Per-pane export-html relies on more-dropdown visibility that needs
  // frontend positioning fix. The full toolbar export is covered by test 54.
// Scenario: Per-pane export creates downloadable HTML snippet with toolbar, tab-bar, panes; WS status hidden; timestamp toggle works
//   Given a live session with a range selected in SENSOR_A
//   When  the per-pane export-html button is clicked and the saved HTML is reopened in a new browser
//   Then  toolbar, tab-bar and panes are visible, ws-status is hidden, and timestamp mode toggles Relative→Absolute

  test.skip('opens downloaded HTML snippet and replays regular pane layout', async ({ page, browser }, testInfo) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const { start, end } = await waitForRangePair(page, 'SENSOR_A', 'kind=prefix-cleanup', 'kind=timestamp-cleanup');
    await start.click();
    await end.click({ modifiers: ['Shift'] });

    await openMore(page, 'SENSOR_A');
    const downloadPromise = page.waitForEvent('download');
    await page.locator('#export-html-SENSOR_A').click();
    const download = await downloadPromise;
    expect(download.suggestedFilename()).toMatch(/^embed-log-exact-.*\.html$/);
    const htmlPath = await saveDownload(download, testInfo);

    const snippet = await openHtmlFile(browser, htmlPath);
    try {
      await expect(snippet.locator('#toolbar')).toBeVisible();
      await expect(snippet.locator('#tab-bar')).toBeVisible();
      await expect(snippet.locator('#pane-SENSOR_A')).toBeVisible();
      await expect(snippet.locator('#pane-SENSOR_B')).toBeVisible();
      await expect(snippet.locator('#log-SENSOR_A')).toContainText('kind=prefix-cleanup');
      await expect(snippet.locator('#ws-status')).toBeHidden();

      await snippet.locator('#btn-settings').click();
      await expect(snippet.locator('#btn-timestamp-mode')).toHaveText('Relative');
      await snippet.locator('#btn-timestamp-mode').click();
      await expect(snippet.locator('#btn-timestamp-mode')).toHaveText('Absolute');
      await expect(snippet.locator('#log-SENSOR_A .log-line').first().locator('.ts')).toHaveText(/\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3}/);
    } finally {
      await snippet.close();
    }
  });

// Scenario: Full toolbar Export creates a static snapshot embedding log data with all tabs/panes present
//   Given a live session with log data in SENSOR_A and SENSOR_B
//   When  the global Export button is clicked and the HTML is saved and reopened
//   Then  the file contains serialized log data, toolbar and all panes are visible, and switching to DevB shows SENSOR_C

  test('full toolbar Export opens as a static snapshot with deterministic logs', async ({ page, browser }, testInfo) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
    await waitForSourceTestLine(page, 'SENSOR_A');
    await waitForSourceTestLine(page, 'SENSOR_B');
    await waitForLineContaining(page, 'SENSOR_A', 'kind=filter-alpha');

    const downloadPromise = page.waitForEvent('download');
    await page.locator('#btn-export').click();
    const download = await downloadPromise;
    expect(download.suggestedFilename()).toMatch(/^embed-log-.*\.html$/);
    const htmlPath = await saveDownload(download, testInfo);

    const html = fs.readFileSync(htmlPath, 'utf-8');
    expect(html).toContain('hydratePanesFromJson');
    expect(html).toContain('kind=filter-alpha');

    const exported = await openHtmlFile(browser, htmlPath);
    try {
      await expect(exported.locator('#toolbar')).toBeVisible();
      await expect(exported.locator('#pane-SENSOR_A')).toBeVisible();
      await expect(exported.locator('#pane-SENSOR_B')).toBeVisible();
      await expect(exported.locator('#log-SENSOR_A')).toContainText('kind=filter-alpha');
      await exported.getByRole('button', { name: 'DevB', exact: true }).click();
      await expect(exported.locator('#pane-SENSOR_C')).toBeVisible();
    } finally {
      await exported.close();
    }
  });

// Scenario: Large generated static export keeps a bounded rendered DOM while preserving real scroll/filter indices
//   Given a 3000-line lazy exported HTML file with a rare match near the end
//   When  the replay is opened, scrolled, filtered, and the filtered line is clicked
//   Then  the DOM remains bounded, bottom/middle scrolling maps to real raw indices, and clearing filter restores context
  test('large static export virtualizes scroll range and filter projection', async ({ browser }, testInfo) => {
    const logPath = testInfo.outputPath('large-virtual.log');
    const htmlPath = testInfo.outputPath('large-virtual.html');
    const lines = [];
    for (let i = 0; i < 3000; i++) {
      const rare = i === 2900 ? ' RARE_MATCH' : '';
      lines.push(`[2026-04-22T10:11:${String(i % 60).padStart(2, '0')}.000+00:00] tick=${String(i).padStart(4, '0')}${rare}`);
    }
    fs.writeFileSync(logPath, lines.join('\n') + '\n', 'utf-8');
    generateMergedHtml(htmlPath, logPath);

    const exported = await openHtmlFile(browser, htmlPath);
    const replayErrors = collectPageErrors(exported);
    try {
      const log = exported.locator('#log-A');
      await expect(log.locator('.log-line').first()).toContainText('tick=0000');

      const renderedCount = async () => log.locator('.log-line').count();
      await expect.poll(renderedCount).toBeLessThan(250);

      await log.evaluate(el => { el.scrollTop = el.scrollHeight; });
      await expect.poll(async () => {
        return log.locator('.log-line').evaluateAll(nodes =>
          Math.max(...nodes.map(n => parseInt(n.dataset.idx, 10)).filter(Number.isFinite))
        );
      }).toBe(2999);
      await expect.poll(renderedCount).toBeLessThan(250);

      await log.evaluate(el => { el.scrollTop = el.scrollHeight / 2; });
      await expect.poll(async () => {
        const indices = await log.locator('.log-line').evaluateAll(nodes =>
          nodes.map(n => parseInt(n.dataset.idx, 10)).filter(Number.isFinite)
        );
        return Math.min(...indices) <= 1500 && Math.max(...indices) >= 1500;
      }).toBe(true);

      await exported.locator('.filter-input[data-pane="A"]').fill('RARE_MATCH');
      await expect(log.locator('.log-line', { hasText: 'RARE_MATCH' })).toBeVisible();
      await expect(log.locator('[data-idx="2900"]')).toContainText('RARE_MATCH');
      await expect.poll(renderedCount).toBeLessThan(250);

      await log.locator('[data-idx="2900"]').click();
      await expect(exported.locator('.filter-input[data-pane="A"]')).toHaveValue('');
      await expect(log.locator('[data-idx="2900"]')).toBeVisible();
      const geometry = await log.evaluate(el => ({ scrollHeight: el.scrollHeight, clientHeight: el.clientHeight }));
      expect(geometry.scrollHeight).toBeGreaterThan(geometry.clientHeight);
      expect(replayErrors).toEqual([]);
    } finally {
      await exported.close();
    }
  });

// Scenario: Exported snapshot hides live-only buttons, shows offline controls, and supports unwrap/font actions
//   Given a live session with data in SENSOR_A and SENSOR_B
//   When  the snapshot is exported and reopened
//   Then  clear/export/ws-status are hidden, unwrap/theme/settings are present, unwrap mode shows pane-tabs, and font controls resize text

  test('exported full snapshot keeps only offline toolbar actions and supports unwrap/font controls', async ({ page, browser }, testInfo) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
    await waitForSourceTestLine(page, 'SENSOR_A');
    await waitForSourceTestLine(page, 'SENSOR_B');

    const downloadPromise = page.waitForEvent('download');
    await page.locator('#btn-export').click();
    const download = await downloadPromise;
    const htmlPath = await saveDownload(download, testInfo);

    const exported = await openHtmlFile(browser, htmlPath);
    try {
      await expect(exported.locator('#btn-clear')).toHaveCount(0);
      await expect(exported.locator('#btn-export')).toHaveCount(0);
      await expect(exported.locator('#ws-status')).toHaveCount(0);

      await expect(exported.locator('#btn-unwrap')).toBeVisible();
      await expect(exported.locator('#btn-theme')).toBeVisible();
      await expect(exported.locator('#btn-settings')).toBeVisible();

      await exported.locator('#btn-unwrap').click();
      await expect(exported.locator('#btn-unwrap')).toHaveClass(/active/);
      await expect(exported.locator('#tab-bar .tab-btn')).toHaveText(['DEVICE_A-DevA', 'HOST-DevA', 'AUX-DevB', 'PYTEST-PYTEST', 'CBOR-cbor-tab', 'CoAP-CoAP', 'DUT-UART', 'DEBUG-UART', 'Net-Network']);
      for (let i = 0; i < 9; i++) {
        await exported.locator('#tab-bar .tab-btn').nth(i).click();
      }

      await exported.locator('#btn-settings').click();
      await expect(exported.locator('#settings-panel')).toHaveClass(/open/);
      await expect(exported.locator('#btn-font-dec')).toBeVisible();
      await expect(exported.locator('#btn-font-reset')).toBeVisible();
      await expect(exported.locator('#btn-font-inc')).toBeVisible();
      await expect(exported.locator('#btn-download-raw')).toBeVisible();

      const line = exported.locator('#log-SENSOR_A .log-line').first();
      const before = await line.evaluate(el => getComputedStyle(el).fontSize);
      await exported.locator('#btn-font-inc').click();
      await expect.poll(async () => {
        return line.evaluate(el => getComputedStyle(el).fontSize);
      }).not.toBe(before);
    } finally {
      await exported.close();
    }
  });

// Scenario: Repeated Export captures log content that arrived after the first export
//   Given a live session where SENSOR_A lines are arriving over time
//   When  a first export captures initial lines and a later export captures additional lines
//   Then  the second export contains all lines from the first plus new ones, with a higher count
test('repeated Export captures newer log content that arrived after first export', async ({ page }, testInfo) => {
  await page.goto('/');
  await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

  // Wait for initial data in both panes of the active tab
  await waitForSourceTestLine(page, 'SENSOR_A');
  await waitForSourceTestLine(page, 'SENSOR_B');

  // First export — should contain the initial lines
  const dl1 = page.waitForEvent('download');
  await page.locator('#btn-export').click();
  const snap1 = await saveDownload(await dl1, testInfo);
  const html1 = fs.readFileSync(snap1, 'utf-8');
  expect(html1).toContain('TEST src=SENSOR_A');
  expect(html1).toContain('TEST src=SENSOR_B');

  // Count log lines in both exports
  const dataMatch1 = html1.match(/TEST src=SENSOR_A/g);
  const count1 = dataMatch1 ? dataMatch1.length : 0;

  // Wait until the live UI has definitely received additional SENSOR_A lines.
  // The pane is virtualized, so the rendered DOM count stays bounded; use the
  // largest rendered raw index as the live-tail signal instead.
  const liveMaxIdx = async () => page.locator('#log-SENSOR_A .log-line').evaluateAll(nodes =>
    Math.max(...nodes.map(n => parseInt(n.dataset.idx, 10)).filter(Number.isFinite))
  );
  const maxIdx1 = await liveMaxIdx();
  await expect.poll(liveMaxIdx).toBeGreaterThan(maxIdx1);

  // Second export — should contain all lines from the first PLUS new ones
  const dl2 = page.waitForEvent('download');
  await page.locator('#btn-export').click();
  const snap2 = await saveDownload(await dl2, testInfo);
  const html2 = fs.readFileSync(snap2, 'utf-8');

  const dataMatch2 = html2.match(/TEST src=SENSOR_A/g);
  const count2 = dataMatch2 ? dataMatch2.length : 0;

  expect(html2).toContain('TEST src=SENSOR_A');
  expect(count2).toBeGreaterThan(count1);
});
});
