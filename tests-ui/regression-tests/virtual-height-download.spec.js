import { expect, test } from '@playwright/test';
import fs from 'node:fs';

// The session is part of the mocked config so that a WebSocket reconnect (which
// re-sends config) cannot reset window.__embedLogSession in the middle of a test.
const SAVED_SESSION = { id: 'saved', source_files: { SENSOR_A: 'A.log' } };

async function setup(page, session = null) {
  await page.routeWebSocket('**/ws', ws => ws.send(JSON.stringify({
    type: 'config', tabs: [{ label: 'Main', panes: ['SENSOR_A'] }, { label: 'Hidden', panes: ['SENSOR_B'] }],
    pane_kinds: { SENSOR_A: 'udp', SENSOR_B: 'udp' },
    ...(session ? { session } : {}),
  })));
  await page.goto('/');
  await expect(page.locator('#ws-status')).toContainText(/connected/i);
  await page.evaluate(async () => {
    const { appendLineBatch } = await import('/lines.js');
    appendLineBatch(Array.from({ length: 2000 }, (_, i) => ({
      paneId: 'SENSOR_A', ts: '12:00:00', rawText: i % 7 === 0 ? `row ${i} ${'long '.repeat(100)}` : `short ${i}`, isTx: false,
    })).concat([{ paneId: 'SENSOR_B', ts: '12:00:00', rawText: 'hidden café record', isTx: false }]));
  });
}

async function geometry(area) {
  return area.evaluate(el => {
    const bounds = el.getBoundingClientRect();
    const rows = [...el.querySelectorAll('.log-line')];
    const visible = rows.filter(row => { const r = row.getBoundingClientRect(); return r.bottom > bounds.top && r.top < bounds.bottom; });
    return { visible: visible.length, rendered: rows.length, tail: el.scrollHeight - Math.max(...rows.map(row => row.offsetTop + row.offsetHeight)) };
  });
}

test('variable-height scrolling and wrap toggles leave neither blank rows nor a blank tail', async ({ page }) => {
  await setup(page);
  const area = page.locator('#log-SENSOR_A');
  await expect(area).not.toHaveClass(/wrap/);
  for (const width of [1100, 720]) {
    await page.setViewportSize({ width, height: 800 });
    for (const wrapped of [true, false, true, false]) {
      await page.locator('#pane-SENSOR_A .pane-wrap-btn').click();
      await expect(page.locator('#pane-SENSOR_A .pane-wrap-btn')).toHaveAttribute('aria-pressed', String(wrapped));
      for (const fraction of [0, 0.2, 0.8, 1]) {
        await area.evaluate((el, f) => { el.scrollTop = (el.scrollHeight - el.clientHeight) * f; }, fraction);
        await expect.poll(async () => (await geometry(area)).visible).toBeGreaterThan(1);
        expect((await geometry(area)).rendered).toBeLessThan(300);
      }
      await expect.poll(async () => Math.abs((await geometry(area)).tail)).toBeLessThan(10);
      if (!wrapped) {
        const gaps = await area.evaluate(el => {
          const rows = [...el.querySelectorAll('.log-line')].sort((a, b) => a.offsetTop - b.offsetTop);
          return rows.slice(1).map((r, i) => Math.abs(r.offsetTop - rows[i].offsetTop - rows[i].offsetHeight));
        });
        expect(Math.max(...gaps)).toBeLessThan(2);
      }
    }
  }
  await page.evaluate(() => {
    document.documentElement.style.setProperty('--font-size', '24px');
    window.__embedLogInvalidateVirtualMetrics();
  });
  await area.evaluate(el => { el.scrollTop = el.scrollHeight; });
  await expect.poll(async () => (await geometry(area)).visible).toBeGreaterThan(1);
  await expect(area).toHaveCSS('scrollbar-gutter', 'stable');
});

for (const savedSession of [false, true]) {
test(`per-pane ZIP includes hidden tabs and ${savedSession ? 'saved history' : 'all loaded records'}`, async ({ page }, testInfo) => {
  if (savedSession) {
    await page.route('**/sessions/saved/A.log', route => route.fulfill({ body: 'older saved record\nlatest saved record\n' }));
  }
  await setup(page, savedSession ? SAVED_SESSION : null);
  await page.locator('#btn-settings').click();
  await page.locator('#btn-download-raw').click();
  const pending = page.waitForEvent('download');
  await page.locator('#btn-download-split').click();
  const download = await pending;
  expect(download.suggestedFilename()).toBe('embed-log-panes.zip');
  const file = testInfo.outputPath('panes.zip');
  await download.saveAs(file);
  const zip = fs.readFileSync(file), files = {};
  let offset = 0;
  while (zip.readUInt32LE(offset) === 0x04034b50) {
    expect(zip.readUInt16LE(offset + 8)).toBe(0);
    const size = zip.readUInt32LE(offset + 18), nameLen = zip.readUInt16LE(offset + 26);
    const name = zip.toString('utf8', offset + 30, offset + 30 + nameLen);
    const start = offset + 30 + nameLen;
    files[name] = zip.toString('utf8', start, start + size);
    offset = start + size;
  }
  expect(zip.readUInt32LE(offset)).toBe(0x02014b50);
  expect(Object.keys(files).sort()).toEqual(['SENSOR_A.log', 'SENSOR_B.log']);
  if (savedSession) expect(files['SENSOR_A.log']).toBe('older saved record\nlatest saved record\n');
  else expect(files['SENSOR_A.log'].trimEnd().split('\n')).toHaveLength(2000);
  expect(files['SENSOR_B.log']).toContain('hidden café record');
});
}

test('a failed saved-log fetch cannot silently produce an incomplete ZIP', async ({ page }) => {
  await page.route('**/sessions/saved/A.log', route => route.fulfill({ status: 404 }));
  await setup(page, SAVED_SESSION);
  const downloads = [];
  page.on('download', d => downloads.push(d));
  const alert = page.waitForEvent('dialog');
  await page.locator('#btn-settings').click();
  await page.locator('#btn-download-raw').click();
  await page.locator('#btn-download-split').click();
  const dialog = await alert;
  expect(dialog.message()).toContain('HTTP 404');
  await dialog.dismiss();
  expect(downloads).toHaveLength(0);
});
