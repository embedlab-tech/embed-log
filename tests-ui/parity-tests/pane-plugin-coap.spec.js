import { expect, test } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';

import {
  collectPageErrors,
  getFreeTcpPort,
  getFreeUdpPort,
  makeTempDir,
  openHtmlFile,
  saveDownload,
  sendUdpLine,
  spawnRustServer,
  terminateChild,
  waitForServer,
} from './helpers.js';

const coapHex = '40011234B3666F6F03626172';
const inlineSummary = 'v:1 t:CON c:GET i:1234 {} [Uri-Path:foo, Uri-Path:bar] :: data len 0';

async function readClipboard(page) {
  return page.evaluate(() => window.__lastCopiedText || '');
}

// Scenario: A pane-local CoAP plugin keeps raw lines by default, but can render parsed summaries inline from the pane plugin indicator.
//   Given a UDP pane configured with the built-in hex-coap plugin
//   When  log lines contain extra words/spaces plus arbitrary leading hex bytes before the packet
//   Then  hover still shows the decoded tooltip, and enabling All logs rewrites matching lines to compact CoAP summaries.
test('live pane plugin decodes CoAP hex strings and can switch to inline all-logs mode', async ({ page }) => {
  const httpPort = await getFreeTcpPort();
  const udpPort = await getFreeUdpPort();
  const tmpDir = makeTempDir('embed-log-pane-plugin-');
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

  const child = spawnRustServer(configPath);

  try {
    await page.addInitScript(() => {
      window.__lastCopiedText = '';
      Object.defineProperty(navigator, 'clipboard', {
        configurable: true,
        value: {
          writeText: async text => {
            window.__lastCopiedText = String(text);
          },
          readText: async () => window.__lastCopiedText,
        },
      });
    });
    await waitForServer(baseUrl);
    await page.goto(baseUrl);
    await page.setViewportSize({ width: 520, height: 900 });
    await expect(page.locator('#pane-COAP_UDP')).toBeVisible();

    await sendUdpLine(udpPort, `prefix words AABBCC ${coapHex} suffix words`);
    await sendUdpLine(udpPort, `prefix bytes 99 88 77 66 40 01 12 34 B3 66 6F 6F 03 62 61 72 suffix`);
    await sendUdpLine(udpPort, 'plain log without any packet');

    const lines = page.locator('#log-COAP_UDP .log-line');
    await expect(lines).toHaveCount(3);

    const firstLine = lines.nth(0);
    const secondLine = lines.nth(1);
    const thirdLine = lines.nth(2);

    await expect(firstLine).toContainText('AABBCC');
    await expect(secondLine).toContainText('99 88 77 66');
    await expect(thirdLine).toContainText('plain log without any packet');

    await firstLine.hover();
    await expect(page.locator('#plugin-tooltip')).toBeVisible();
    await expect(page.locator('#plugin-tooltip')).toContainText('CoAP — GET /foo/bar');
    await expect(page.locator('#plugin-tooltip')).toContainText('Type: CON');
    await expect(page.locator('#plugin-tooltip')).toContainText('Message ID: 0x1234');
    await expect(page.locator('#plugin-tooltip')).toContainText('Options: Uri-Path:foo, Uri-Path:bar');
    await expect(page.locator('#plugin-tooltip')).toContainText('Data len: 0');

    await thirdLine.hover();
    await expect(page.locator('#plugin-tooltip')).toBeHidden();
    const hoverCard = page.locator('#pane-plugin-hover-card');
    const pluginIndicator = page.locator('#plugin-indicator-COAP_UDP');
    const tooltip = page.locator('#plugin-tooltip');

    // Hover icon → hover card shows current state (disabled)
    await pluginIndicator.hover();
    await expect(hoverCard).toBeVisible();
    await expect(hoverCard).toContainText('CoAP');
    await expect(hoverCard).toContainText('All logs');
    await expect(hoverCard).toContainText('Disabled');
    await expect(hoverCard).toContainText('Render decoded CoAP summaries inline for matching lines in this pane.');

    // Move cursor onto hover card and enable all-logs
    await hoverCard.hover();
    const allLogsCheckbox = hoverCard.locator('input[type="checkbox"]').first();
    await allLogsCheckbox.check();

    // Move cursor away to dismiss
    await page.mouse.move(10, 10);
    await expect(hoverCard).toBeHidden();
    // Now verify all-logs mode is active
    await expect(firstLine).toContainText(inlineSummary);
    await expect(secondLine).toContainText(inlineSummary);
    await expect(firstLine).toContainText('AABBCC');
    await expect(secondLine).toContainText('99 88 77 66');
    await expect(thirdLine).toContainText('plain log without any packet');

    // All-logs mode disables the line tooltip
    await firstLine.hover();
    await expect(tooltip).toBeHidden();

    // Hover icon → hover card shows "Enabled"
    await pluginIndicator.hover();
    await expect(hoverCard).toBeVisible();
    await expect(hoverCard).toContainText('Enabled');
    // Dismiss hover card before clicking line (card overlays log area)
    await pluginIndicator.blur();
    await page.evaluate(() => window.__embedLogHidePluginOverlays?.());
    await expect(hoverCard).toBeHidden();
    // Selection dismisses hover card
    await firstLine.click();
    await expect(hoverCard).toBeHidden();
    await expect(page.locator('#copy-COAP_UDP')).toBeVisible();
    await page.locator('#copy-COAP_UDP').click();
    const copied = await readClipboard(page);
    expect(copied).toContain(inlineSummary);
    expect(copied).not.toContain(coapHex);
    expect(copied).toContain('AABBCC');



    // Hover indicator and disable all-logs
    await pluginIndicator.hover();
    await expect(hoverCard).toBeVisible();
    await hoverCard.hover();
    const disableCheckbox = hoverCard.locator('input[type="checkbox"]').first();
    await disableCheckbox.uncheck();
    await page.mouse.move(10, 10);
    await expect(hoverCard).toBeHidden();
    await expect(firstLine).toContainText('AABBCC');
    await expect(secondLine).toContainText('99 88 77 66');
    await expect(thirdLine).toContainText('plain log without any packet');

    expect(errors).toEqual([]);
  } finally {
    await terminateChild(child);
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});

test('exported snapshot preserves CoAP all-logs pane mode', async ({ page, browser }, testInfo) => {
  const httpPort = await getFreeTcpPort();
  const udpPort = await getFreeUdpPort();
  const tmpDir = makeTempDir('embed-log-pane-plugin-export-');
  const configPath = path.join(tmpDir, 'embed-log.yml');
  const baseUrl = `http://127.0.0.1:${httpPort}/`;

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

  const child = spawnRustServer(configPath);

  try {
    await waitForServer(baseUrl);
    await page.goto(baseUrl);
    await expect(page.locator('#pane-COAP_UDP')).toBeVisible();

    await sendUdpLine(udpPort, coapHex);
    await sendUdpLine(udpPort, 'plain log without any packet');

    const lines = page.locator('#log-COAP_UDP .log-line');
    await expect(lines).toHaveCount(2);

    const indicator = page.locator('#plugin-indicator-COAP_UDP');
    await indicator.hover();
    const liveHoverCard = page.locator('#pane-plugin-hover-card');
    await expect(liveHoverCard).toBeVisible();
    await liveHoverCard.hover();
    const liveCheckbox = liveHoverCard.locator('input[type="checkbox"]').first();
    await liveCheckbox.check();
    await page.mouse.move(10, 10);
    await expect(lines.nth(0)).toContainText(inlineSummary);

    const downloadPromise = page.waitForEvent('download');
    await page.locator('#btn-export').click();
    const download = await downloadPromise;
    const htmlPath = await saveDownload(download, testInfo);
    const html = fs.readFileSync(htmlPath, 'utf-8');
    expect(html).toContain('window.__embedLogInitialPanePluginUiState');

    const exported = await openHtmlFile(browser, htmlPath);
    try {
      const exportedLines = exported.locator('#log-COAP_UDP .log-line');
      await expect(exportedLines.nth(0)).toContainText(inlineSummary);
      await expect(exportedLines.nth(1)).toContainText('plain log without any packet');

      const exportedIndicator = exported.locator('#plugin-indicator-COAP_UDP');
      await exportedIndicator.hover();
      const exportedHoverCard = exported.locator('#pane-plugin-hover-card');
      await expect(exportedHoverCard).toBeVisible();
      await exportedHoverCard.hover();
      const exportedCheckbox = exportedHoverCard.locator('input[type="checkbox"]').first();
      await expect(exportedCheckbox).toBeChecked();
    } finally {
      await exported.close();
    }
  } finally {
    await terminateChild(child);
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});
