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
      type: 'config', session: { id: 'events', symbols: { A: 'symbols/A.elf' } },
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

  // Session-wide counts can change while the picker is open.
  await badgeA.click();
  await page.keyboard.press('Tab');
  const action = page.locator('#pane-A .events-boot-filter').first();
  await expect(action).toBeFocused();
  socket().send(JSON.stringify({ type: 'source_events', session_id: 'events', counts: { A: { boot: 5, crash: 2 }, B: { boot: 1, crash: 0 } } }));
  await expect(badgeA).toHaveText('⟳ 5✖ 2');
  await expect(page.locator('#pane-A .events-note')).toHaveText('4 earlier events are older than the loaded lines');
  await expect(action).toBeFocused();
  await page.keyboard.press('Escape');

  // A merged pane adds up its sources.
  await page.getByRole('button', { name: 'Merged' }).click();
  await expect(page.locator('#pane-ALL .pane-events-btn')).toHaveText('⟳ 6✖ 2');
});

test('right pane pickers stay on screen when the two-pane view narrows', async ({ page }) => {
  await page.setViewportSize({ width: 1100, height: 700 });
  await serve(page, {
    counts: { A: { boot: 2, crash: 1 }, B: { boot: 1, crash: 0 } },
    levels: { A: null, B: { levels: [{ id: 'inf', label: 'Info' }] } },
  });
  const pane = page.locator('#pane-B');
  const badge = pane.locator('.pane-events-btn');
  const events = pane.getByRole('group', { name: 'Device restarts and crashes' });
  const assertOnScreen = async locator => {
    await expect(locator).toBeInViewport({ ratio: 1 });
    const { x, y, width, height } = await locator.boundingBox();
    expect(x).toBeGreaterThanOrEqual(0);
    expect(y).toBeGreaterThanOrEqual(0);
    expect(x + width).toBeLessThanOrEqual(page.viewportSize().width);
    expect(y + height).toBeLessThanOrEqual(page.viewportSize().height);
  };

  await expect(page.locator('#pane-A')).toBeVisible();
  expect((await pane.boundingBox()).x).toBeGreaterThan(0);
  await badge.focus();
  await page.keyboard.press('Enter');
  await expect(badge).toHaveAttribute('aria-expanded', 'true');
  await assertOnScreen(events);
  await expect(events.getByRole('button', { name: /Restart/ })).toBeFocused();

  await page.setViewportSize({ width: 375, height: 400 });
  await assertOnScreen(events);
  await page.keyboard.press('Escape');
  await expect(badge).toHaveAttribute('aria-expanded', 'false');
  await expect(badge).toBeFocused();

  const levels = pane.getByRole('button', { name: 'Levels' });
  await levels.click();
  await assertOnScreen(pane.getByRole('group', { name: 'Visible log levels' }));
  await page.keyboard.press('Escape');
  await expect(levels).toBeFocused();

  await badge.click();
  await page.keyboard.press('Tab');
  const only = events.getByRole('button', { name: 'Only this boot' });
  await expect(only).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(events).toBeHidden();
  await expect(badge).toBeFocused();
  await expect(pane.locator('.boot-label')).toContainText('boot 1/1');
  await expect(page.locator('#pane-A .boot-chip')).toBeHidden();
  for (const name of ['Previous boot', 'Next boot', 'Show all boots']) {
    const control = pane.getByRole('button', { name, exact: true });
    await assertOnScreen(control);
    const { width, height } = await control.boundingBox();
    expect(Math.min(width, height)).toBeGreaterThanOrEqual(30);
  }
  await pane.getByRole('button', { name: 'Show all boots' }).click();
  await expect(pane.locator('.boot-chip')).toBeHidden();
  await expect(badge).toBeFocused();
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

test('a crash is decoded with the firmware ELF in a dialog', async ({ page }) => {
  let requested = null;
  await page.route('**/api/sessions/events/crashes/**', route => {
    requested = new URL(route.request().url());
    return route.fulfill({ contentType: 'application/json', body: JSON.stringify({ ok: true, report: {
      source: 'A', sequence: 60, elf: 'symbols/A.elf',
      lines: ['<err> os: ***** MPU FAULT *****', '<err> os: Faulting instruction address (r15/pc): 0x0000102a'],
      addresses: [
        { label: 'pc', address: 0x102a, is_return: false, line: '', frames: [
          { function: 'scale_sample', file: '/src/app/src/sensor.c', line: 7, inlined: true },
          { function: 'process_sample', file: '/src/app/src/sensor.c', line: 12, inlined: false }] },
        { label: 'lr', address: 0x104b, is_return: true, line: '', frames: [
          { function: 'sensor_task', file: '/src/app/src/sensor.c', line: 20, inlined: false }] },
        { label: 'frame 1', address: 0x8000000, is_return: true, line: '', frames: [] },
      ] } }) });
  });
  await serve(page, { counts: { A: { boot: 2, crash: 1 } } });
  await page.locator('#pane-A .pane-events-btn').click();
  await expect(page.locator('#pane-A .events-decode')).toHaveCount(1);
  await expect(page.locator('#pane-B .events-decode')).toHaveCount(0);
  await page.locator('#pane-A .events-decode').click();
  const dialog = page.getByRole('dialog', { name: 'Crash on Main MCU' });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole('button', { name: 'Close' })).toBeFocused();
  expect(requested.pathname).toBe('/api/sessions/events/crashes/60');
  expect(requested.searchParams.get('source')).toBe('A');
  await expect(dialog.locator('.crash-title')).toHaveText('Crash on Main MCU');
  await expect(dialog.locator('.crash-meta')).toContainText('ELF symbols/A.elf');
  const rows = dialog.locator('.crash-frames tbody tr');
  await expect(rows).toHaveCount(4);
  await expect(rows.nth(0)).toContainText('pc');
  await expect(rows.nth(0)).toContainText('0x0000102a');
  await expect(rows.nth(0)).toContainText('scale_sample');
  await expect(rows.nth(0)).toContainText('inlined');
  await expect(rows.nth(0)).toContainText('src/sensor.c:7');
  await expect(rows.nth(1)).toContainText('process_sample');
  await expect(rows.nth(2)).toContainText('sensor_task');
  await expect(rows.nth(3)).toContainText('?? (not in this ELF)');
  await expect(page.locator('#log-A .log-line.event-crash')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(dialog).toBeHidden();
  await expect(page.locator('#pane-A .pane-events-btn')).toBeFocused();
});

test('a pane can show one boot at a time', async ({ page }) => {
  await serve(page, { counts: { A: { boot: 2, crash: 1 } } });
  const visibleSequences = () => page.evaluate(async () => {
    const { visibleRowIndices } = await import('/lines.js');
    const { state } = await import('/state.js');
    return visibleRowIndices('A').map(i => {
      const entry = state.rawLines.A[i];
      return Array.isArray(entry) ? entry[3].sequence : entry.sequence;
    });
  });
  await page.locator('#pane-A .pane-events-btn').click();
  // Oldest restart is listed last.
  await page.locator('#pane-A .events-boot-filter').last().click();
  const chip = page.locator('#pane-A .boot-chip');
  await expect(chip).toBeVisible();
  await expect(chip.locator('.boot-label')).toContainText('boot 1/2');
  let seq = await visibleSequences();
  expect(seq[0]).toBe(1);
  expect(seq.at(-1)).toBe(61);
  await expect(chip.locator('.boot-prev')).toBeDisabled();

  await chip.locator('.boot-next').click();
  await expect(chip.locator('.boot-label')).toContainText('boot 2/2');
  await expect(chip.locator('.boot-label')).toContainText('now');
  seq = await visibleSequences();
  expect(seq[0]).toBe(62);
  expect(seq.at(-1)).toBe(119);

  await chip.locator('.boot-clear').click();
  await expect(chip).toBeHidden();
  expect((await visibleSequences()).length).toBe(119);
});


test('event navigation reveals a crash outside the selected boot', async ({ page }) => {
  await serve(page, { counts: { A: { boot: 2, crash: 1 } } });
  await page.locator('#pane-A .pane-events-btn').click();
  await page.locator('#pane-A .events-boot-filter').first().click();
  await expect(page.locator('#pane-A .boot-label')).toContainText('boot 2/2');
  await expect(page.locator('#log-A .event-crash')).toHaveCount(0);
  await page.locator('#pane-A .pane-events-btn').click();
  await page.locator('#pane-A .events-item.events-crash').click();
  await expect(page.locator('#log-A .event-crash')).toBeVisible();
  expect(await page.evaluate(async () => (await import('/state.js')).state.highlightedIdx.A)).toBe(59);
  await expect(page.locator('#pane-A .boot-label')).toContainText('boot 2/2');
  // Explicitly selecting another boot removes the temporary exception.
  await page.locator('#pane-A .boot-prev').click();
  await page.locator('#pane-A .boot-next').click();
  await expect(page.locator('#log-A .event-crash')).toHaveCount(0);
});
