import { expect, test } from '@playwright/test';

const config = session => ({
  type: 'config', session: { id: session },
  tabs: [{ label: 'Logs', panes: ['A'] }], pane_kinds: { A: 'uart' },
});
const record = (session, sequence, message) => ({
  type: 'rx', data: message, message, source_id: 'A', source_kind: 'uart',
  timestamp: '12:00:00', sequence, session_id: session, line_idx: sequence,
});

test('a stream gap from the server is shown until the page resyncs', async ({ page }) => {
  let socket;
  await page.routeWebSocket('**/ws', ws => {
    socket = ws;
    ws.send(JSON.stringify(config('gap')));
    ws.send(JSON.stringify(record('gap', 1, 'before gap')));
  });
  await page.goto('/');
  await expect(page.locator('#log-A .log-line')).toHaveCount(1);
  await expect(page.locator('#stream-gap')).toHaveCount(0);
  socket.send(JSON.stringify({ type: 'stream_gap', skipped: 1200 }));
  socket.send(JSON.stringify({ type: 'stream_gap', skipped: 34 }));
  await expect(page.locator('#stream-gap')).toBeVisible();
  await expect(page.locator('#stream-gap')).toHaveText('⚠ 1,234 lines skipped');
  await expect(page.locator('#log-A .log-line')).toHaveCount(1);
});

test('pane byte statistics count UTF-8 bytes without encoding', async ({ page }) => {
  await page.routeWebSocket('**/ws', ws => ws.send(JSON.stringify(config('bytes'))));
  await page.goto('/');
  const result = await page.evaluate(async () => {
    const { utf8ByteLength } = await import('/lines.js');
    const samples = ['', 'ascii', 'zażółć', '€ 100', '😀 emoji', 'a\ud800b', '\x1b[31mred\x1b[0m', '中文\u0000'];
    const encoder = new TextEncoder();
    return samples.map(text => [utf8ByteLength(text), encoder.encode(text).length]);
  });
  for (const [got, expected] of result) expect(got).toBe(expected);
});

test('highlight loads history for the pane sources before the replay and reports failures', async ({ page }) => {
  const requests = [];
  let fail = true;
  await page.route('**/api/sessions/limits/history*', route => {
    requests.push(new URL(route.request().url()));
    if (fail) return route.fulfill({ status: 500, body: 'boom' });
    const rows = [record('limits', 3, 'needle old'), record('limits', 9, 'needle older')].map(row => JSON.stringify(row));
    // The last line has no newline yet: a record still being written is ignored.
    return route.fulfill({ contentType: 'application/x-ndjson', body: rows.join('\n') + '\n{"partial' });
  });
  await page.routeWebSocket('**/ws', ws => {
    ws.send(JSON.stringify(config('limits')));
    ws.send(JSON.stringify(record('limits', 10, 'needle in replay')));
  });
  await page.goto('/');
  await expect(page.locator('#log-A .log-line')).toHaveCount(1);
  await page.locator('#pane-A .search-mode').click();
  const input = page.locator('.filter-input[data-pane="A"]');
  await input.fill('needle');
  await input.press('Enter');
  const counter = page.locator('#pane-A .search-count');
  await expect(counter).toHaveText('History unavailable');
  await expect(counter).toHaveAttribute('title', /HTTP 500/);
  expect(requests[0].searchParams.get('sources')).toBe('A');
  expect(requests[0].searchParams.get('before')).toBe('10');

  // Turning Highlight off and on retries.
  fail = false;
  await page.locator('#pane-A .search-mode').click();
  await page.locator('#pane-A .search-mode').click();
  await expect(counter).toHaveText('0 / 3');
  await expect.poll(() => page.evaluate(async () => (await import('/state.js')).state.rawLines.A.length)).toBe(3);
});
