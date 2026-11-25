import { expect, test } from '@playwright/test';

// The pane cache in localStorage is saved after appends pause. Live panes
// append every frame, so it must also be saved while lines keep streaming,
// and saving must not turn compact rows into full line objects.

async function setup(page) {
  await page.routeWebSocket('**/ws', ws => ws.send(JSON.stringify({
    type: 'config', session: { id: 'persist-stream' }, tabs: [{ label: 'Logs', panes: ['A'] }], pane_kinds: { A: 'uart' },
  })));
  await page.goto('/');
  await expect(page.locator('#ws-status')).toContainText(/connected/i);
  await page.evaluate(() => window.__embedLogClearCache?.());
}

function savedLines(page) {
  return page.evaluate(() => {
    for (let i = 0; i < localStorage.length; i++) {
      const key = localStorage.key(i);
      if (key?.startsWith('embed-log:session:persist-stream')) {
        return JSON.parse(localStorage.getItem(key)).lines?.A ?? [];
      }
    }
    return null;
  });
}

test('the pane cache is saved while lines keep streaming', async ({ page }) => {
  await setup(page);
  await page.evaluate(async () => {
    const { appendLineBatch } = await import('/lines.js');
    let seq = 0;
    const batch = count => Array.from({ length: count }, () => {
      seq += 1;
      return { paneId: 'A', ts: '12:00:00', rawText: `line ${seq}`, isTx: false,
        meta: { sequence: seq, sessionId: 'persist-stream', lineIdx: seq - 1, absNum: seq, relNum: seq } };
    });
    appendLineBatch(batch(1000));
    // Keep appending more often than the debounce delay.
    window.__streamTimer = setInterval(() => appendLineBatch(batch(5)), 100);
  });

  await expect.poll(async () => (await savedLines(page))?.length ?? 0, { timeout: 4_000 }).toBeGreaterThan(1000);
  const saved = await savedLines(page);
  expect(saved.at(-1)).toMatchObject({ ts: '12:00:00', isTx: false, sessionId: 'persist-stream' });
  expect(saved.at(-1).text).toBe(`line ${saved.at(-1).sequence}`);
  expect(saved.at(-1).lineIdx).toBe(saved.at(-1).sequence - 1);

  const stillCompact = await page.evaluate(async () => {
    clearInterval(window.__streamTimer);
    const { state } = await import('/state.js');
    // Row 10 is far above the rendered window and was only touched by saving.
    return Array.isArray(state.rawLines.A[10]);
  });
  expect(stillCompact).toBe(true);
});
