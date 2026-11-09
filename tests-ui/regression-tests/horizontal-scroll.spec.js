import { expect, test } from '@playwright/test';

for (const repeatSetup of [false, true]) {
test(`horizontal scrolling and wrap work with ${repeatSetup ? 'repeated' : 'initial'} pane setup`, async ({ page }) => {
  await page.routeWebSocket('**/ws', ws => ws.send(JSON.stringify({
    type: 'config', tabs: [{ label: 'Logs', panes: ['SENSOR_A', 'SENSOR_B'] }],
    pane_kinds: { SENSOR_A: 'udp', SENSOR_B: 'udp' },
  })));
  await page.goto('/');
  await expect(page.locator('#ws-status')).toContainText(/connected/i);
  await expect(page.locator('#log-SENSOR_A')).toHaveClass(/wrap/);
  await expect(page.locator('#pane-SENSOR_A .pane-wrap-btn')).toHaveClass(/active/);
  await expect(page.locator('#pane-SENSOR_A .pane-wrap-btn')).toHaveAttribute('aria-pressed', 'true');
  await page.locator('#pane-SENSOR_A .pane-wrap-btn').click();
  await page.evaluate(async repeat => {
    const { clearPane, appendLine, _linesSetupPane } = await import('/lines.js');
    if (repeat) _linesSetupPane('SENSOR_A');
    clearPane('SENSOR_A');
    for (let i = 0; i < 200; i++) appendLine('SENSOR_A', '12:00:00', i === 1 ? 'short' : `${i} ${'long-text '.repeat(300)} END`, false);
  }, repeatSetup);
  const area = page.locator('#log-SENSOR_A');
  await expect.poll(() => area.evaluate(el => el.scrollWidth > el.clientWidth + 500)).toBe(true);
  await area.evaluate(el => { el.scrollLeft = 500; });
  await expect.poll(() => area.evaluate(el => el.scrollLeft)).toBe(500);
  await expect(page.locator('#log-SENSOR_B')).toHaveJSProperty('scrollLeft', 0);
  await area.evaluate(el => { el.scrollTop = 0; });
  await expect.poll(() => area.locator('.log-line').first().getAttribute('data-idx')).toBe('0');
  await expect(area).toHaveJSProperty('scrollLeft', 500);
  await area.evaluate(el => { el.scrollLeft = 2000; });
  const hit = await area.evaluate(el => {
    const bounds = el.getBoundingClientRect();
    const row = el.querySelector('[data-idx="1"]');
    const y = row.getBoundingClientRect().top + row.offsetHeight / 2;
    const x = bounds.right - 30;
    return { x, y, row: document.elementFromPoint(x, y)?.closest('.log-line')?.dataset.idx };
  });
  expect(hit.row).toBe('1');
  await page.mouse.move(hit.x, hit.y);
  await expect(area.locator('[data-idx="1"]')).toHaveCSS('background-color', /^(?!rgba\(0, 0, 0, 0\)).+/);
  await page.mouse.click(hit.x, hit.y);
  await expect(area.locator('[data-idx="1"]')).toHaveClass(/selected/);
  await page.locator('#pane-SENSOR_A .pane-wrap-btn').click();
  await expect(area).toHaveClass(/wrap/);
  await expect.poll(() => area.evaluate(el => el.scrollWidth <= el.clientWidth)).toBe(true);
  await expect(area).toHaveJSProperty('scrollLeft', 0);
  await expect(page.locator('#pane-SENSOR_A .pane-wrap-btn')).toHaveClass(/active/);
  await expect(page.locator('#pane-SENSOR_A .pane-wrap-btn')).toHaveAttribute('aria-pressed', 'true');
  await expect.poll(() => area.locator('.log-line').first().evaluate(el => el.offsetHeight)).toBeGreaterThan(40);
  if (repeatSetup) {
    await page.evaluate(async () => (await import('/tabcreate.js')).rebuildLayout());
    await expect(area).toHaveClass(/wrap/);
    await expect(page.locator('#pane-SENSOR_A .pane-wrap-btn')).toHaveClass(/active/);
    await expect(page.locator('#pane-SENSOR_A .pane-wrap-btn')).toHaveAttribute('aria-pressed', 'true');
  }
  await page.locator('#pane-SENSOR_A .pane-wrap-btn').click();
  await expect(page.locator('#pane-SENSOR_A .pane-wrap-btn')).not.toHaveClass(/active/);
  await expect(page.locator('#pane-SENSOR_A .pane-wrap-btn')).toHaveAttribute('aria-pressed', 'false');
  await expect.poll(() => area.evaluate(el => el.scrollWidth > el.clientWidth + 500)).toBe(true);
  await page.evaluate(async () => {
    const { clearPane, appendLine } = await import('/lines.js');
    clearPane('SENSOR_A');
    for (let i = 0; i < 200; i++) appendLine('SENSOR_A', '12:00:00', i === 0 ? 'long-text '.repeat(300) : 'short', false);
  });
  await expect(area.locator('[data-idx="0"]')).toHaveCount(0);
  await expect.poll(() => area.evaluate(el => el.scrollWidth > el.clientWidth + 2000)).toBe(true);
  await page.evaluate(async () => {
    const { clearPane, appendLine } = await import('/lines.js');
    clearPane('SENSOR_A');
    appendLine('SENSOR_A', '12:00:00', 'new short session', false);
  });
  await expect.poll(() => area.evaluate(el => el.scrollWidth <= el.clientWidth)).toBe(true);
  await page.setViewportSize({ width: 900, height: 720 });
  await expect.poll(() => area.evaluate(el => el.scrollWidth <= el.clientWidth)).toBe(true);
});
}
