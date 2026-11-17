import { expect, test } from '@playwright/test';

// Restart / crash detection: the server marks records with `event` and sends
// session-wide counts as `source_events`; panes show a counter and a list.

const record = (sequence, source_id, message, event = null) => ({
  type: 'rx', data: message, message, source_id, source_kind: 'uart', session_id: 'events',
  timestamp: `12:00:${String(sequence).padStart(2, '0')}.000`, sequence, line_idx: sequence,
  ...(event ? { event } : {}),
  ...(message.includes('<err>') ? { level: 'err' } : message.includes('<inf>') ? { level: 'inf' } : {}),
});

async function serve(page, { counts, levels = { A: null, B: null } }) {
  let socket;
  await page.routeWebSocket('**/ws', ws => {
    socket = ws;
    ws.send(JSON.stringify({
      type: 'config', session: { id: 'events' },
      tabs: [{ label: 'Devices', panes: ['A', 'B'] }, { label: 'Merged', panes: ['ALL'] }],
      pane_labels: { A: 'Main MCU', B: 'Network core' }, pane_kinds: { A: 'uart', B: 'uart' },
      merges: [{ name: 'ALL', of: ['A', 'B'] }], source_levels: levels,
    }));
    if (counts) ws.send(JSON.stringify({ type: 'source_events', session_id: 'events', counts }));
    const rows = [record(1, 'A', '*** Booting Zephyr OS build v3.7.0 ***', 'boot')];
    for (let i = 2; i < 60; i++) rows.push(record(i, 'A', `<inf> main: step ${i}`));
    rows.push(record(60, 'A', '<err> os: ***** MPU FAULT *****', 'crash'));
    rows.push(record(61, 'A', '<err> os: >>> ZEPHYR FATAL ERROR 19: Unknown error on CPU 0'));
    rows.push(record(62, 'A', '*** Booting Zephyr OS build v3.7.0 ***', 'boot'));
    for (let i = 63; i < 120; i++) rows.push(record(i, 'A', `<inf> main: step ${i}`));
    rows.push(record(120, 'B', '*** Booting nRF Connect SDK v2.7.0 ***', 'boot'));
    for (const row of rows) ws.send(JSON.stringify(row));
  });
  await page.goto('/');
  await expect.poll(() => page.evaluate(async () => (await import('/state.js')).state.rawLines.A?.length ?? 0)).toBe(119);
  return () => socket;
}

test('panes count restarts and crashes and jump to them', async ({ page }) => {
  const socket = await serve(page, { counts: { A: { boot: 2, crash: 1 }, B: { boot: 1, crash: 0 } } });
  const badgeA = page.locator('#pane-A .pane-events-btn');
  await expect(badgeA).toHaveText('⟳ 2✖ 1');
  await expect(badgeA).toHaveClass(/has-crash/);
  await expect(page.locator('#pane-B .pane-events-btn')).toHaveText('⟳ 1');

  await badgeA.click();
  const items = page.locator('#pane-A .events-item');
  await expect(items).toHaveCount(3);
  await expect(items.locator('.events-kind')).toHaveText(['Restart', 'Crash', 'Restart']);
  await expect(items.nth(1)).toContainText('MPU FAULT');
  await items.nth(1).click();
  await expect(page.locator('#pane-A .events-menu')).toBeHidden();
  const crash = page.locator('#log-A .log-line.event-crash');
  await expect(crash).toBeVisible();
  await expect(crash).toContainText('MPU FAULT');
  await expect(page.locator('#log-A .log-line.event-boot').first()).toContainText('Booting');

  // Counts are session-wide: the server may know events older than the loaded lines.
  socket().send(JSON.stringify({ type: 'source_events', session_id: 'events', counts: { A: { boot: 5, crash: 2 }, B: { boot: 1, crash: 0 } } }));
  await expect(badgeA).toHaveText('⟳ 5✖ 2');
  await badgeA.click();
  await expect(page.locator('#pane-A .events-note')).toHaveText('4 earlier events are older than the loaded lines');
  await page.keyboard.press('Escape');

  // A merged pane adds up its sources.
  await page.getByRole('button', { name: 'Merged' }).click();
  await expect(page.locator('#pane-ALL .pane-events-btn')).toHaveText('⟳ 6✖ 2');
});

test('without server counts (exported report) events are counted from the lines', async ({ page }) => {
  await serve(page, { counts: null });
  await expect(page.locator('#pane-A .pane-events')).toBeHidden(); // live server reported none
  await page.evaluate(() => {
    delete window.__embedLogSourceEvents;
    window.__embedLogRefreshEventBadges();
  });
  await expect(page.locator('#pane-A .pane-events-btn')).toHaveText('⟳ 2✖ 1');
  await expect(page.locator('#pane-B .pane-events-btn')).toHaveText('⟳ 1');
});

for (const hiddenBy of ['text', 'level', 'both']) {
  test(`event navigation reveals the exact crash hidden by ${hiddenBy} filters`, async ({ page }) => {
    await serve(page, { counts: { A: { boot: 2, crash: 1 } }, levels: {
      A: { levels: [{ id: 'inf', label: 'Info' }, { id: 'err', label: 'Error' }] }, B: null,
    } });
    const input = page.locator('.filter-input[data-pane="A"]');
    if (hiddenBy !== 'level') {
      await input.fill('main: step');
      await input.press('Enter');
    }
    if (hiddenBy !== 'text') await page.evaluate(async () => {
      (await import('/levels.js')).setLevelVisible('A', 'err', false);
      (await import('/lines.js')).rerenderPane('A');
    });
    await expect(page.locator('#log-A .event-crash')).toHaveCount(0);
    await page.locator('#pane-A .pane-events-btn').click();
    await page.locator('#pane-A .events-item.events-crash').click();
    await expect(page.locator('#log-A .event-crash')).toBeVisible();
    expect(await page.evaluate(async () => (await import('/state.js')).state.highlightedIdx.A)).toBe(59);
    // Preserve the user's filter, with only the requested event revealed.
    if (hiddenBy !== 'level') await expect(input).toHaveValue('main: step');
    const visible = await page.evaluate(async () => (await import('/lines.js')).visibleRowIndices('A'));
    expect(visible).toContain(59);
    if (hiddenBy !== 'text') expect(visible).not.toContain(60);
    // A later filter edit removes the temporary reveal.
    await input.fill('no such row');
    await input.press('Enter');
    await expect(page.locator('#log-A .log-line')).toHaveCount(0);
  });
}

test('event reveal follows history prepends and is removed on clear', async ({ page }) => {
  await serve(page, { counts: { A: { boot: 2, crash: 1 } } });
  const result = await page.evaluate(async () => {
    const { state } = await import('/state.js');
    const { revealRawLine, prependPaneHistory, visibleRowIndices, rerenderPane, clearPane, appendLineBatch } = await import('/lines.js');
    state.filters.A = /does-not-match/;
    rerenderPane('A');
    revealRawLine('A', 59);
    prependPaneHistory('A', [['11:59:59', 'older row', false, { sequence: 0 }]]);
    const shifted = visibleRowIndices('A');
    clearPane('A');
    appendLineBatch([{ paneId: 'A', ts: '12:01:00', rawText: 'unrelated', isTx: false }]);
    return { shifted, afterClear: visibleRowIndices('A') };
  });
  expect(result.shifted).toEqual([60]);
  expect(result.afterClear).toEqual([]);
});
