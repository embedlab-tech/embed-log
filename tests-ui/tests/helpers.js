import { expect } from '@playwright/test';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

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


