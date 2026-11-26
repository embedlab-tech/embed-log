import { expect, test } from '@playwright/test';

// Review follow-ups for the large-log audit work: markers identify a line by its
// server line index, a short reconnect keeps what is loaded, and the byte counter
// counts the logged message, not the colour the server adds for display.

const SESSION = 'review';
const config = markers => ({
  type: 'config', session: { id: SESSION },
  tabs: [{ label: 'Logs', panes: ['A'] }], pane_kinds: { A: 'uart' }, markers,
});
const record = (n, extra = {}) => ({
  type: 'rx', data: `msg${n}`, message: `msg${n}`, source_id: 'A', source_kind: 'uart',
  timestamp: '12:00:00', sequence: n, session_id: SESSION, line_idx: n - 1, ...extra,
});
const marker = lineIdx => ({
  paneId: 'A', lineIdx, endIdx: lineIdx, numTs: 1, description: 'look here', createdAt: 'now',
});

// Serves each connection from a list of frames; the test can drop the socket.
async function mockServer(page, connections) {
  let current;
  let count = 0;
  await page.routeWebSocket('**/ws', ws => {
    current = ws;
    (connections[count++] || []).forEach(frame => ws.send(JSON.stringify(frame)));
  });
  return { drop: () => current.close(), count: () => count };
}

const markedRows = page => page.locator('#log-A .has-marker');

test('a marker stays on its own line after a reconnect', async ({ page }) => {
  const server = await mockServer(page, [
    [config([marker(0)]), record(1), record(2), record(3)],
    // The server only replays the recent records, not msg1.
    [config([marker(0)]), record(2), record(3)],
  ]);
  await page.goto('/');
  await expect(page.locator('#log-A')).toContainText('msg3');
  await expect(markedRows(page)).toHaveCount(1);
  await expect(markedRows(page)).toContainText('msg1');

  server.drop();
  await expect.poll(server.count, { timeout: 10_000 }).toBe(2);
  await expect(page.locator('#ws-status')).toContainText(/connected/i);
  // Nothing was dropped, and the marker did not slide to msg2.
  for (const n of [1, 2, 3]) await expect(page.locator('#log-A')).toContainText(`msg${n}`);
  await expect(markedRows(page)).toHaveCount(1);
  await expect(markedRows(page)).toContainText('msg1');
});

test('a marker is not drawn on another line while its line is not loaded', async ({ page }) => {
  // The replay starts at the second line; the marker belongs to the first.
  await mockServer(page, [[config([marker(0)]), record(2), record(3)]]);
  await page.goto('/');
  await expect(page.locator('#log-A')).toContainText('msg3');
  await expect(markedRows(page)).toHaveCount(0);
});

test('a short reconnect keeps older history, and a real gap is reported', async ({ page }) => {
  const server = await mockServer(page, [
    [config([]), record(1), record(2), record(3)],
    [config([]), record(3), record(4)], // contiguous: replay overlaps what we have
    [config([]), record(9)], // records 5..8 were missed and cannot be replayed
  ]);
  await page.goto('/');
  await expect(page.locator('#log-A')).toContainText('msg3');
  const notice = page.locator('#stream-gap');

  server.drop();
  await expect.poll(server.count, { timeout: 10_000 }).toBe(2);
  await expect(page.locator('#log-A')).toContainText('msg4');
  for (const n of [1, 2, 3, 4]) await expect(page.locator('#log-A')).toContainText(`msg${n}`);
  await expect(page.locator('[data-pane-stats="A"]')).toContainText('4 lines');
  await expect(notice).toHaveCount(0);

  server.drop();
  await expect.poll(server.count, { timeout: 10_000 }).toBe(3);
  await expect(notice).toBeVisible();
});

test('the live byte counter counts the logged message, not the added colour', async ({ page }) => {
  const colored = n => record(n, { data: `\u001b[32mmsg${n}\u001b[0m` });
  await mockServer(page, [[config([]), colored(1), colored(2), colored(3)]]);
  await page.goto('/');
  await expect(page.locator('#log-A')).toContainText('msg3');
  // Three records of four bytes each.
  await expect(page.locator('[data-pane-stats="A"]')).toContainText('3 lines · 12 B');
  await expect(page.locator('#toolbar-stats')).toContainText('3 lines · 12 B');
});
