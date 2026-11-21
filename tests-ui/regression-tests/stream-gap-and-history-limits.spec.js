import { expect, test } from '@playwright/test';

const config = session => ({
  type: 'config', session: { id: session },
  tabs: [{ label: 'Logs', panes: ['A'] }], pane_kinds: { A: 'uart' },
});
const record = (session, sequence, message) => ({
  type: 'rx', data: message, message, source_id: 'A', source_kind: 'uart',
  timestamp: '12:00:00', sequence, session_id: session, line_idx: sequence,
});

test('a stream gap clears on rotation or a replay rebase, but a new gap can still warn', async ({ page }) => {
  let socket;
  await page.routeWebSocket('**/ws', ws => {
    socket = ws;
    ws.send(JSON.stringify(config('gap')));
    ws.send(JSON.stringify(record('gap', 1, 'before rotation')));
  });
  await page.goto('/');
  const log = page.locator('#log-A');
  const notice = page.locator('#stream-gap');
  await expect(log).toContainText('before rotation');
  await expect(notice).toHaveCount(0);
  socket.send(JSON.stringify({ type: 'stream_gap', skipped: 1200, session_id: 'gap' }));
  await expect(notice).toHaveText('⚠ Log panes may be incomplete · Reload recent logs');
  await expect(notice).toHaveAttribute('title', /older missing lines may not be recovered/);

  socket.send(JSON.stringify({ type: 'session_rotated', session: { id: 'next' } }));
  socket.send(JSON.stringify({ type: 'stream_gap', skipped: 1, session_id: 'gap' }));
  socket.send(JSON.stringify(record('next', 1, 'after rotation')));
  await expect(log).toContainText('after rotation');
  await expect(notice).toBeHidden();
  socket.send(JSON.stringify({ type: 'stream_gap', skipped: 1, session_id: 'next' }));
  await expect(notice).toBeVisible();

  // A lagged client can receive fresh config instead of the dropped rotation frame.
  socket.send(JSON.stringify(config('rebased')));
  socket.send(JSON.stringify(record('rebased', 1, 'after rebase')));
  await expect(log).toContainText('after rebase');
  await expect(log).not.toContainText('after rotation');
  await expect(notice).toBeHidden();
  socket.send(JSON.stringify({ type: 'stream_gap', skipped: 1, session_id: 'rebased' }));
  await expect(notice).toBeVisible();
});

for (const recovery of ['reconnect', 'reload']) {
  test(`fresh replay after ${recovery} clears a stream gap and rebuilds Highlight`, async ({ page }) => {
    const session = 'interior-gap';
    const rows = [1, 2, 3].map(i => record(session, i, `needle ${i}`));
    let socket;
    let connections = 0;
    let releaseHistory;
    const historyReady = new Promise(resolve => { releaseHistory = resolve; });
    await page.route(`**/api/sessions/${session}/history*`, async route => {
      if (connections > 1) await historyReady;
      await route.fulfill({ body: rows.map(JSON.stringify).join('\n') + '\n' });
    });
    await page.routeWebSocket('**/ws', ws => {
      socket = ws;
      if (++connections !== 1) return;
      ws.send(JSON.stringify(config(session)));
      ws.send(JSON.stringify(rows[0]));
      ws.send(JSON.stringify(rows[2]));
    });
    await page.goto('/');
    const lines = page.locator('#log-A .log-line');
    const notice = page.locator('#stream-gap');
    const mode = page.locator('#pane-A .search-mode');
    const input = page.locator('.filter-input[data-pane="A"]');
    const counter = page.locator('#pane-A .search-count');
    const next = page.locator('#pane-A .search-next');
    await expect(lines).toHaveCount(2);
    await mode.click();
    await input.fill('needle');
    await expect(counter).toHaveText('0 / 2');
    socket.send(JSON.stringify({ type: 'stream_gap', skipped: 1, session_id: session }));
    socket.send(JSON.stringify({ ...config(session), app_name: 'Runtime update' }));
    await expect(page.locator('#toolbar .app-name')).toHaveText('Runtime update');
    await expect(notice).toBeVisible();
    await expect(counter).toHaveText('Results incomplete');
    await expect(next).toBeDisabled();
    await expect(lines).toHaveCount(2);

    if (recovery === 'reload') await notice.click();
    else await socket.close({ code: 1000, reason: 'test reconnect' });
    await expect.poll(() => connections).toBe(2);
    if (recovery === 'reconnect') {
      await expect(notice).toBeVisible();
      await expect(counter).toHaveText('Results incomplete');
    }
    socket.send(JSON.stringify(config(session)));
    if (recovery === 'reload') {
      await expect(mode).toBeVisible();
      await expect(lines).toHaveCount(0); // don't restore the gapped cache
    }
    for (const row of rows.slice(1)) socket.send(JSON.stringify(row));
    await expect(lines).toHaveText([/needle 2/, /needle 3/]);
    await expect(notice).toBeHidden();
    if (recovery === 'reload') {
      await mode.click();
      await input.fill('needle');
    }
    releaseHistory();
    await expect(lines).toHaveText([/needle 1/, /needle 2/, /needle 3/]);
    await expect(counter).toHaveText('0 / 3');
    await expect(next).toBeEnabled();
    await next.click();
    await expect(counter).toHaveText('1 / 3');
  });
}

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
