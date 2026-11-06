import { expect, test } from '@playwright/test';

test('live configuration shows effective source, parser and server settings', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('#ws-status')).toContainText(/connected/i);
  await page.locator('#btn-settings').click();
  const button = page.locator('#btn-runtime-config');
  await expect(button).toBeEnabled();
  await button.click();
  const dialog = page.getByRole('dialog', { name: /Runtime configuration/ });
  await expect(dialog).toBeVisible();
  const summary = JSON.parse(await dialog.locator('pre').textContent());
  expect(summary.server.port).toBe(10800);
  expect(summary.logs_dir).toContain('logs');
  expect(summary.sources.find(source => source.name === 'SENSOR_A')).toMatchObject({
    type: 'udp', port: 6000, parser: { type: 'hex-coap' },
  });
  expect(summary).not.toHaveProperty('frontend_plugins');
  await page.keyboard.press('Escape');
  await expect(dialog).not.toBeVisible();
  await expect(button).toBeFocused();
});

test('configuration is text-only, updates on config messages, and tolerates older servers', async ({ page }) => {
  let socket;
  const config = { type: 'config', tabs: [{ label: 'Logs', panes: ['DUT'] }] };
  await page.routeWebSocket('**/ws', ws => {
    socket = ws;
    ws.send(JSON.stringify({ ...config, runtime_config: { port: '<img src=x onerror="window.injected=true">', baudrate: 115200 } }));
  });
  await page.goto('/');
  await page.locator('#btn-settings').click();
  const button = page.locator('#btn-runtime-config');
  await button.click();
  const dialog = page.getByRole('dialog', { name: /Runtime configuration/ });
  await expect(dialog.locator('pre')).toContainText('<img');
  await expect(dialog.locator('img')).toHaveCount(0);
  expect(await page.evaluate(() => window.injected)).toBeUndefined();
  socket.send(JSON.stringify({ ...config, runtime_config: { baudrate: 9600 } }));
  await expect(dialog.locator('pre')).toContainText('9600');
  socket.send(JSON.stringify(config));
  await expect(dialog.locator('pre')).toContainText('unavailable');
  await dialog.getByRole('button', { name: 'Close' }).click();
  await expect(button).toBeDisabled();
});
