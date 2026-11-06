import fs from 'node:fs/promises';
import { expect, test } from '@playwright/test';

test('pane download uses complete saved bytes and follows session rotation', async ({ page }, testInfo) => {
  let socket;
  const bytes = Buffer.from('history before connection\r\n\x1b[31mred\x1b[0m  \r\n');
  for (const id of ['session-1', 'session-2']) {
    const dir = new URL(`../.tmp/logs/${id}/`, import.meta.url);
    await fs.mkdir(dir, { recursive: true });
    await fs.writeFile(new URL('original.log', dir), bytes);
  }
  await page.routeWebSocket('**/ws', ws => {
    socket = ws;
    ws.send(JSON.stringify({
      type: 'config', tabs: [{ label: 'Logs', panes: ['DUT'] }],
      session: { id: 'session-1', source_files: { DUT: 'C:\\logs\\original.log' } },
    }));
  });
  await page.goto('/');
  const button = page.locator('#pane-DUT .pane-download-btn');
  await expect(button).toHaveAttribute('title', /full saved source/);
  for (const id of ['session-1', 'session-2']) {
    if (id === 'session-2') {
      socket.send(JSON.stringify({ type: 'session_rotated', session: { id, source_files: { DUT: '/logs/original.log' } } }));
      await expect.poll(() => page.evaluate(() => window.__embedLogSession?.id)).toBe(id);
    }
    const downloadEvent = page.waitForEvent('download');
    await button.click();
    const download = await downloadEvent;
    const file = testInfo.outputPath(`${id}.log`);
    await download.saveAs(file);
    expect(await fs.readFile(file)).toEqual(bytes);
    expect(new URL(download.url()).pathname).toBe(`/sessions/${id}/original.log`);
  }
});

test('panes without a saved source retain the loaded-log download', async ({ page }, testInfo) => {
  await page.routeWebSocket('**/ws', ws => ws.send(JSON.stringify({
    type: 'config', tabs: [{ label: 'Logs', panes: ['DUT'] }],
  })));
  await page.goto('/');
  await expect(page.locator('#pane-DUT .pane-download-btn')).toBeVisible();
  await page.evaluate(async () => {
    const { appendLine } = await import('/lines.js');
    appendLine('DUT', '12:00:00', 'loaded log', false);
  });
  const event = page.waitForEvent('download');
  await page.locator('#pane-DUT .pane-download-btn').click();
  const download = await event;
  const file = testInfo.outputPath('loaded.log');
  await download.saveAs(file);
  expect(await fs.readFile(file, 'utf8')).toContain('loaded log');
});
