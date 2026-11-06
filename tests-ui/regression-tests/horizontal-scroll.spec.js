import { expect, test } from '@playwright/test';

test('long lines scroll horizontally without moving other panes; wrap restores the left edge', async ({ page }) => {
  await page.routeWebSocket('**/ws', ws => ws.send(JSON.stringify({
    type: 'config', tabs: [{ label: 'Logs', panes: ['SENSOR_A', 'SENSOR_B'] }],
    pane_kinds: { SENSOR_A: 'udp', SENSOR_B: 'udp' },
  })));
  await page.goto('/');
  await expect(page.locator('#ws-status')).toContainText(/connected/i);
  await page.evaluate(async () => {
    const { clearPane, appendLine } = await import('/lines.js');
    clearPane('SENSOR_A');
    for (let i = 0; i < 200; i++) appendLine('SENSOR_A', '12:00:00', `${i} ${'long-text '.repeat(80)} END`, false);
  });
  const area = page.locator('#log-SENSOR_A');
  await expect.poll(() => area.evaluate(el => el.scrollWidth > el.clientWidth + 500)).toBe(true);
  await area.evaluate(el => { el.scrollLeft = 500; });
  await expect.poll(() => area.evaluate(el => el.scrollLeft)).toBe(500);
  await expect(page.locator('#log-SENSOR_B')).toHaveJSProperty('scrollLeft', 0);
  await area.evaluate(el => { el.scrollTop = 0; });
  await expect.poll(() => area.locator('.log-line').first().getAttribute('data-idx')).toBe('0');
  await expect(area).toHaveJSProperty('scrollLeft', 500);
  await page.locator('#pane-SENSOR_A .pane-wrap-btn').click();
  await expect(area).toHaveClass(/wrap/);
  await expect.poll(() => area.evaluate(el => el.scrollWidth <= el.clientWidth)).toBe(true);
  await expect(area).toHaveJSProperty('scrollLeft', 0);
  await page.locator('#pane-SENSOR_A .pane-wrap-btn').click();
  await expect.poll(() => area.evaluate(el => el.scrollWidth > el.clientWidth + 500)).toBe(true);
});
